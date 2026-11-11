package app.watchdog.remote

import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.view.Surface
import androidx.core.content.ContextCompat
import app.watchdog.remote.camera.ApplyOutcome
import app.watchdog.remote.camera.Camera2Controller
import app.watchdog.remote.camera.CameraCatalog
import app.watchdog.remote.camera.CameraController
import app.watchdog.remote.camera.CameraControls
import app.watchdog.remote.camera.CameraSnapshot
import app.watchdog.remote.camera.CameraStatus
import app.watchdog.remote.camera.CapabilityDescriptor
import app.watchdog.remote.camera.ControlSource
import app.watchdog.remote.camera.ControlStateStore
import app.watchdog.remote.log.DeviceLog
import app.watchdog.remote.pipeline.Frame
import app.watchdog.remote.pipeline.FramePipeline
import app.watchdog.remote.pipeline.PipelineConfig
import app.watchdog.remote.protocol.CommandMessage
import app.watchdog.remote.protocol.ControlAssignment
import app.watchdog.remote.protocol.FramePayload
import app.watchdog.remote.protocol.JsonMessages
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.PROTOCOL_VERSION
import app.watchdog.remote.protocol.Packet
import app.watchdog.remote.protocol.ProtocolException
import app.watchdog.remote.transport.HelloAckBuilder
import app.watchdog.remote.transport.LogStreamer
import app.watchdog.remote.transport.TransportServer
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.round

/**
 * Foreground service that owns the camera, the frame pipeline and the host
 * transport. It outlives [MainActivity] and keeps streaming with the screen
 * off; the wake lock is held only while a capture session is active.
 *
 * Exported entry points are token protected: the host passes the same
 * [SessionToken] it later presents in `HOST_HELLO`. See `docs/android-app.md`.
 *
 * This service is the single coordinator for the camera lifecycle, transport,
 * control state and the command pipeline; splitting it would hide the strict
 * ordering between those pieces.
 */
@Suppress("TooManyFunctions")
class CaptureService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null
    private var transport: TransportServer? = null

    @Volatile
    private var camera: Camera2Controller? = null

    @Volatile
    private var pipeline: FramePipeline? = null

    private val logStreamer = LogStreamer()

    private var lastCameraStatus: CameraStatus? = null
    private var streamedBefore = false

    @Volatile
    private var capabilities: JSONObject? = null

    @Volatile
    private var controlState: ControlStateStore? = null
    private val implications = ControlImplications()
    private val readouts = DeviceReadouts(this)
    private val stateHandler = Handler(Looper.getMainLooper())
    private val stateSeq = AtomicLong()
    private val stateLock = Any()

    @Volatile
    private var stateRateHz = DEFAULT_STATE_RATE_HZ

    @Volatile
    private var stateTimerRunning = false

    private var lastSensorTimestampNs: Long? = null

    @Volatile
    private var fpsEstimate = 0.0

    @Volatile
    private var activeToken: String? = null

    /** Last authenticated host name, kept across short CLI sessions. */
    @Volatile
    private var lastHostName: String? = null

    /** `elapsedRealtime` when the last session closed; 0 while one is open. */
    @Volatile
    private var lastSessionClosedAtMs = 0L

    @Volatile
    private var running = false

    @Volatile
    private var previewSurface: Surface? = null

    private val pipelineConfig = PipelineConfig()
    private val binder = LocalBinder()

    /** Local binding for the on-device UI; the host never uses this path. */
    inner class LocalBinder : Binder() {
        fun service(): CaptureService = this@CaptureService
    }

    override fun onBind(intent: Intent?): IBinder = binder

    /**
     * Attach or detach the [MainActivity] preview surface. Safe before the
     * camera opens: the controller remembers the surface and includes it in
     * the first session.
     */
    fun setPreviewSurface(surface: Surface?) {
        previewSurface = surface
        camera?.setPreviewSurface(surface)
    }

    /** Values for the on-device status panels (`docs/android-app.md`). */
    fun uiStatus(): UiStatus {
        val snapshot = camera?.snapshot
        val sessionOpen = transport?.hasSession == true
        // Short-lived CLI commands open and close a session per invocation.
        // Keep reporting the host as connected briefly so the panel does not
        // flicker between "connected" and "waiting for host" during a burst.
        val recentlyConnected =
            !sessionOpen &&
                lastSessionClosedAtMs != 0L &&
                SystemClock.elapsedRealtime() - lastSessionClosedAtMs < HOST_STATUS_GRACE_MS
        val hostConnected = sessionOpen || recentlyConnected
        return UiStatus(
            hostConnected = hostConnected,
            hostName = if (hostConnected) transport?.hostName ?: lastHostName else null,
            cameraId = snapshot?.cameraId,
            cameraFacing =
                CameraCatalog.facingName(
                    camera?.cameraCharacteristics?.get(CameraCharacteristics.LENS_FACING),
                ),
            cameraStatus = snapshot?.status ?: CameraStatus.CLOSED,
            fps = fpsEstimate.takeIf { it > ZERO_FPS },
            width = pipelineConfig.width,
            height = pipelineConfig.height,
            jpegQuality = pipelineConfig.jpegQuality,
            droppedFrames = pipeline?.stats()?.dropped ?: 0L,
            batteryPercent = readouts.batteryPercent(),
            storageFreeMb = readouts.storageFreeMb(),
            temperatureC = readouts.temperatureC(),
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        val token = intent?.getStringExtra(SessionToken.EXTRA)
        if (token == null || !SessionToken.isValid(token)) {
            DeviceLog.w(TAG, "rejected action=$action: missing or malformed session token")
            return START_NOT_STICKY
        }
        when (action) {
            ACTION_START -> startCapture(token)
            ACTION_STOP -> stopCapture("host_request")
            ACTION_STATUS -> DeviceLog.i(TAG, "status: running=$running session=${transport?.hasSession == true}")
            else -> DeviceLog.w(TAG, "rejected unknown action=$action")
        }
        // Redeliver the token-bearing intent if Android kills the process, so a
        // host-side `am start-foreground-service` is not the only recovery path.
        return START_REDELIVER_INTENT
    }

    private fun startCapture(token: String) {
        when {
            running -> {
                if (token == activeToken) {
                    DeviceLog.i(TAG, "capture already running; ignoring duplicate start")
                } else {
                    // Re-provisioning issues a fresh token; rebind the transport
                    // without restarting the camera so the new host can connect.
                    DeviceLog.i(TAG, "session token changed; restarting transport")
                    stopTransport()
                    startTransport(token)
                    activeToken = token
                }
            }
            !hasCameraPermission() -> {
                DeviceLog.e(TAG, "CAMERA permission not granted; provisioning must grant it before start")
                stopSelf()
            }
            !tryStartForeground(token) -> stopSelf()
            else -> {
                acquireWakeLock()
                running = true
                activeToken = token
                startTransport(token)
                startCamera()
                DeviceLog.i(TAG, "capture service started (session token accepted)")
            }
        }
    }

    private fun hasCameraPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, android.Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    private fun tryStartForeground(token: String): Boolean {
        CaptureNotifications.createChannel(this)
        val notification = CaptureNotifications.build(this, token)
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    CaptureNotifications.NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA,
                )
            } else {
                startForeground(CaptureNotifications.NOTIFICATION_ID, notification)
            }
            true
        } catch (e: SecurityException) {
            DeviceLog.e(TAG, "startForeground denied: ${e.message}", e)
            false
        } catch (e: IllegalStateException) {
            DeviceLog.e(TAG, "startForeground not allowed: ${e.message}", e)
            false
        }
    }

    private fun stopCapture(reason: String) {
        if (running) {
            DeviceLog.i(TAG, "stopping capture: $reason")
            running = false
            activeToken = null
            stopCamera()
            stopTransport()
            releaseWakeLock()
            stopForeground(STOP_FOREGROUND_REMOVE)
        }
        stopSelf()
    }

    private fun startTransport(token: String) {
        if (transport != null) {
            return
        }
        // Snapshot controls once per transport start: camera characteristics are
        // stable for the session and re-enumerating them on every handshake
        // would stall reconnects.
        val descriptors = CapabilityDescriptor.all(this)
        capabilities = descriptors
        val server =
            TransportServer(
                sessionName = SOCKET_PREFIX + token,
                sessionToken = token,
                helloAck = { version -> HelloAckBuilder.build(this, version, descriptors) },
                listener = transportListener,
            )
        try {
            server.start()
            transport = server
        } catch (e: IOException) {
            DeviceLog.e(TAG, "transport server failed to start: ${e.message}", e)
        }
    }

    private fun stopTransport() {
        logStreamer.detach()
        stopStateTimer()
        transport?.stop()
        transport = null
        lastSessionClosedAtMs = 0L
    }

    private val transportListener =
        object : TransportServer.Listener {
            override fun onSessionOpen(hostName: String?) {
                DeviceLog.i(TAG, "host session open: ${hostName ?: "unknown"}")
                hostName?.let { lastHostName = it }
                lastSessionClosedAtMs = 0L
                transport?.let(logStreamer::attach)
                // Full resync on every (re)connect: the host may have missed
                // any number of deltas while disconnected.
                controlState?.resetBaseline()
                sendState(full = true)
                startStateTimer()
            }

            override fun onSessionClosed(reason: String) {
                DeviceLog.i(TAG, "host session closed: $reason")
                // A takeover registers the replacement before evicting the old
                // session, but a session that closes on its own thread may report
                // after the replacement is already open. Keep streaming state
                // while any authenticated host remains.
                if (transport?.hasSession != true) {
                    lastSessionClosedAtMs = SystemClock.elapsedRealtime()
                    logStreamer.detach()
                    stopStateTimer()
                }
            }

            override fun onPacket(packet: Packet) {
                if (packet.type == MessageType.COMMAND) {
                    handleCommand(packet)
                } else {
                    DeviceLog.d(TAG, "unhandled packet ${packet.type}")
                }
            }
        }

    private fun startCamera() {
        if (camera != null) {
            return
        }
        val cameraId = defaultCameraId()
        if (cameraId == null) {
            DeviceLog.e(TAG, "no camera available")
            sendEvent("error", JSONObject().put("message", "no camera available"))
            return
        }
        controlState = ControlStateStore(capabilities?.optJSONObject(cameraId) ?: JSONObject())
        val framePipeline = FramePipeline(pipelineConfig) { frame -> sendFrame(frame) }
        framePipeline.start()
        val surface = framePipeline.surface
        if (surface == null) {
            DeviceLog.e(TAG, "frame pipeline produced no capture surface")
            framePipeline.stop()
            return
        }
        val controller = Camera2Controller(this, cameraListener)
        camera = controller
        pipeline = framePipeline
        controller.open(cameraId, listOf(surface))
        controller.setPreviewSurface(previewSurface)
        controller.startStreaming()
        DeviceLog.i(TAG, "camera $cameraId opening")
    }

    private fun stopCamera() {
        camera?.release()
        camera = null
        pipeline?.stop()
        pipeline = null
        controlState = null
        implications.reset()
        lastSensorTimestampNs = null
        fpsEstimate = ZERO_FPS
        lastCameraStatus = null
        streamedBefore = false
    }

    private fun defaultCameraId(): String? {
        val cameras = CameraCatalog.list(this)
        return (cameras.firstOrNull { it.facing == "back" } ?: cameras.firstOrNull())?.id
    }

    private fun sendFrame(frame: Frame) {
        val server = transport ?: return
        if (!server.hasSession) {
            return
        }
        val payload =
            FramePayload(
                sequence = frame.sequence,
                sensorTimestampNs = frame.sensorTimestampNs,
                width = frame.width,
                height = frame.height,
                rotation = frame.rotation,
                flags = 0,
                jpeg = frame.jpeg,
            ).encode()
        server.send(Packet(MessageType.FRAME, 0, payload))
    }

    private fun sendEvent(kind: String, data: JSONObject? = null) {
        val server = transport ?: return
        server.send(Packet(MessageType.EVENT, 0, JsonMessages.encodeEvent(kind, data = data)))
    }

    private val cameraListener =
        object : CameraController.Listener {
            override fun onStateChanged(snapshot: CameraSnapshot) {
                val previous = lastCameraStatus
                lastCameraStatus = snapshot.status
                DeviceLog.i(TAG, "camera ${snapshot.status} (was ${previous ?: "none"})")
                when (snapshot.status) {
                    CameraStatus.RECOVERING ->
                        if (previous != CameraStatus.RECOVERING) {
                            sendEvent("camera_disconnected", cameraData(snapshot))
                        }
                    CameraStatus.STREAMING -> {
                        if (streamedBefore) {
                            sendEvent("camera_reopened", cameraData(snapshot))
                        } else {
                            streamedBefore = true
                        }
                        // The camera id is only known now; resync the host with
                        // a full snapshot for this camera.
                        controlState?.resetBaseline()
                        sendState(full = true)
                    }
                    CameraStatus.ERROR ->
                        if (previous != CameraStatus.ERROR) {
                            sendEvent(
                                "error",
                                cameraData(snapshot).put("message", snapshot.lastError ?: "camera error"),
                            )
                        }
                    else -> Unit
                }
            }

            override fun onCaptureResult(result: TotalCaptureResult) {
                val store = controlState ?: return
                val chars = camera?.cameraCharacteristics ?: return
                CameraControls.extract(result, chars).forEach { (id, value) ->
                    store.setActual(id, value)
                    implications.onCaptureResult(id, value)
                }
                updateFps(result, store)
            }

            private fun cameraData(snapshot: CameraSnapshot): JSONObject =
                JSONObject().put("camera_id", snapshot.cameraId ?: "").put("recoveries", snapshot.recoveries)
        }

    private fun handleCommand(packet: Packet) {
        val command =
            try {
                JsonMessages.parseCommand(packet.payload)
            } catch (e: ProtocolException) {
                sendError("INVALID_REQUEST", e.message ?: "invalid COMMAND")
                return
            }
        when (command.command) {
            "set_controls" -> handleSetControls(command)
            "get_state" -> {
                if (controlState == null) {
                    reply(
                        command.id,
                        listOf(
                            JsonMessages.controlResult(
                                null,
                                false,
                                errorCode = "CAMERA_NOT_READY",
                                message = "camera is not open",
                            ),
                        ),
                    )
                } else {
                    sendState(full = true)
                    reply(command.id, listOf(JsonMessages.controlResult(null, true)))
                }
            }
            "get_capabilities" -> {
                resendHelloAck()
                reply(command.id, listOf(JsonMessages.controlResult(null, true)))
            }
            "set_state_rate" -> handleSetStateRate(command)
            "ping" -> reply(command.id, listOf(JsonMessages.controlResult(null, true)))
            "bye" -> reply(command.id, listOf(JsonMessages.controlResult(null, true)))
            else ->
                reply(
                    command.id,
                    listOf(
                        JsonMessages.controlResult(
                            null,
                            false,
                            errorCode = "UNSUPPORTED_COMMAND",
                            message = "'${command.command}' is not available in this build",
                        ),
                    ),
                )
        }
    }

    private fun handleSetControls(command: CommandMessage) {
        val store = controlState
        if (store == null) {
            reply(
                command.id,
                listOf(
                    JsonMessages.controlResult(
                        null,
                        false,
                        errorCode = "CAMERA_NOT_READY",
                        message = "camera is not open",
                    ),
                ),
            )
            return
        }
        val assignments =
            try {
                JsonMessages.parseControlAssignments(command.params)
            } catch (e: ProtocolException) {
                reply(
                    command.id,
                    listOf(
                        JsonMessages.controlResult(
                            null,
                            false,
                            errorCode = "INVALID_REQUEST",
                            message = e.message,
                        ),
                    ),
                )
                return
            }
        val results = mutableListOf<JSONObject>()
        val toApply = LinkedHashMap<String, Any>()
        val triggers = LinkedHashMap<String, Any>()
        val manualSensor = validateAssignments(store, assignments, results, toApply, triggers)
        implications.apply(store, toApply, results, triggers, manualSensor)
        val controller = camera
        var outcome = controller?.applyControls(toApply) ?: ApplyOutcome(false, error = "camera is not open")
        if (outcome.ok) {
            for ((id, value) in triggers) {
                val triggerOutcome = controller?.trigger(id, value)
                if (triggerOutcome != null && !triggerOutcome.ok) {
                    outcome = triggerOutcome
                    break
                }
            }
        }
        reply(command.id, results, if (outcome.ok) null else outcome.error)
        sendState(full = false)
    }

    /**
     * Validate one batch against the descriptor, recording per-control results
     * in wire order and collecting the accepted values. Persistent controls go
     * to [toApply]; one-shot triggers go to [triggers]. Returns true when a
     * manual sensor control was accepted.
     */
    private fun validateAssignments(
        store: ControlStateStore,
        assignments: List<ControlAssignment>,
        results: MutableList<JSONObject>,
        toApply: MutableMap<String, Any>,
        triggers: MutableMap<String, Any>,
    ): Boolean {
        var manualSensor = false
        assignments.forEach { assignment ->
            if (assignment.id in TRIGGERS) {
                validateTrigger(store, assignment, results, triggers)
                return@forEach
            }
            val validation = store.setRequested(assignment.id, assignment.value, ControlSource.HOST)
            if (!validation.ok) {
                results +=
                    JsonMessages.controlResult(
                        assignment.id,
                        false,
                        errorCode = validation.errorCode,
                        message = validation.message,
                    )
            } else {
                results += JsonMessages.controlResult(assignment.id, true, applied = validation.applied)
                validation.applied?.let { toApply[assignment.id] = it }
                if (assignment.id in MANUAL_SENSOR_CONTROLS) {
                    manualSensor = true
                }
            }
        }
        return manualSensor
    }

    private fun validateTrigger(
        store: ControlStateStore,
        assignment: ControlAssignment,
        results: MutableList<JSONObject>,
        triggers: MutableMap<String, Any>,
    ) {
        val validation = store.validateControl(assignment.id, assignment.value)
        if (!validation.ok) {
            results +=
                JsonMessages.controlResult(
                    assignment.id,
                    false,
                    errorCode = validation.errorCode,
                    message = validation.message,
                )
        } else {
            results += JsonMessages.controlResult(assignment.id, true, applied = validation.applied)
            validation.applied?.let { triggers[assignment.id] = it }
        }
    }

    private fun handleSetStateRate(command: CommandMessage) {
        val hz = command.params.optDouble("hz", DEFAULT_STATE_RATE_HZ)
        if (hz < MIN_STATE_RATE_HZ || hz > MAX_STATE_RATE_HZ) {
            reply(
                command.id,
                listOf(
                    JsonMessages.controlResult(
                        null,
                        false,
                        errorCode = "OUT_OF_RANGE",
                        message = "state rate must be between $MIN_STATE_RATE_HZ and $MAX_STATE_RATE_HZ Hz",
                    ),
                ),
            )
            return
        }
        stateRateHz = hz
        if (stateTimerRunning) {
            startStateTimer()
        }
        reply(command.id, listOf(JsonMessages.controlResult(null, true, applied = hz)))
    }

    private fun reply(id: String, results: List<JSONObject>, error: String? = null) {
        transport?.send(
            Packet(MessageType.COMMAND_RESULT, 0, JsonMessages.encodeCommandResult(id, results, error = error)),
        )
    }

    private fun sendError(code: String, message: String) {
        transport?.send(Packet(MessageType.ERROR, 0, JsonMessages.encodeError(code, message)))
    }

    private fun resendHelloAck() {
        val descriptors = capabilities ?: return
        transport?.send(
            Packet(MessageType.HELLO_ACK, 0, HelloAckBuilder.build(this, PROTOCOL_VERSION, descriptors)),
        )
    }

    private fun startStateTimer() {
        stateHandler.removeCallbacks(stateRunnable)
        stateTimerRunning = true
        stateHandler.postDelayed(stateRunnable, stateIntervalMs())
    }

    private fun stopStateTimer() {
        stateTimerRunning = false
        stateHandler.removeCallbacks(stateRunnable)
    }

    private fun stateIntervalMs(): Long = (MILLIS_PER_SECOND / stateRateHz).toLong().coerceAtLeast(1L)

    private val stateRunnable =
        object : Runnable {
            override fun run() {
                if (!stateTimerRunning) {
                    return
                }
                sendState(full = false)
                if (stateTimerRunning) {
                    stateHandler.postDelayed(this, stateIntervalMs())
                }
            }
        }

    private fun sendState(full: Boolean) {
        val store = controlState ?: return
        val server = transport ?: return
        if (!server.hasSession) {
            return
        }
        store.setReadout("battery_percent", readouts.batteryPercent())
        val dropped = pipeline?.stats()?.dropped ?: 0L
        synchronized(stateLock) {
            val seq = stateSeq.incrementAndGet()
            val payload =
                if (full) {
                    store.fullState(
                        seq,
                        System.currentTimeMillis(),
                        currentCameraId(),
                        dropped,
                        readouts.storageFreeMb(),
                    )
                } else {
                    store.deltaState(
                        seq,
                        System.currentTimeMillis(),
                        currentCameraId(),
                        dropped,
                        readouts.storageFreeMb(),
                    ) ?: return
                }
            server.send(
                Packet(
                    MessageType.STATE,
                    if (full) 0 else STATE_DELTA_FLAG,
                    payload.toString().toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun currentCameraId(): String = camera?.snapshot?.cameraId ?: ""

    private fun updateFps(result: TotalCaptureResult, store: ControlStateStore) {
        val timestamp = result.get(CaptureResult.SENSOR_TIMESTAMP) ?: return
        val previous = lastSensorTimestampNs
        lastSensorTimestampNs = timestamp
        if (previous == null || timestamp <= previous) {
            return
        }
        val instant = NANOS_PER_SECOND / (timestamp - previous)
        fpsEstimate =
            if (fpsEstimate == ZERO_FPS) {
                instant
            } else {
                fpsEstimate * FPS_SMOOTHING + instant * (1 - FPS_SMOOTHING)
            }
        store.setReadout("fps_actual", round(fpsEstimate * FPS_SCALE) / FPS_SCALE)
    }

    override fun onDestroy() {
        running = false
        stopCamera()
        stopTransport()
        releaseWakeLock()
        DeviceLog.i(TAG, "capture service destroyed")
        super.onDestroy()
    }

    private fun acquireWakeLock() {
        wakeLock = CaptureNotifications.acquire(this, wakeLock)
    }

    private fun releaseWakeLock() {
        CaptureNotifications.release(wakeLock)
    }

    companion object {
        private const val TAG = "WatchdogCapture"
        private const val SOCKET_PREFIX = "watchdog-"
        private const val DEFAULT_STATE_RATE_HZ = 10.0
        private const val MIN_STATE_RATE_HZ = 1.0
        private const val MAX_STATE_RATE_HZ = 30.0
        private const val STATE_DELTA_FLAG = 1
        private const val FPS_SMOOTHING = 0.9
        private const val FPS_SCALE = 100.0
        private const val ZERO_FPS = 0.0
        private const val MILLIS_PER_SECOND = 1000.0
        private const val NANOS_PER_SECOND = 1_000_000_000.0
        private const val HOST_STATUS_GRACE_MS = 5_000L
        private val MANUAL_SENSOR_CONTROLS = setOf("iso", "exposure_time_ns", "frame_duration_ns")
        private val TRIGGERS = setOf("ae_precapture", "af_trigger")

        const val ACTION_START = "app.watchdog.remote.action.START"
        const val ACTION_STOP = "app.watchdog.remote.action.STOP"
        const val ACTION_STATUS = "app.watchdog.remote.action.STATUS"
    }
}
