package app.watchdog.remote

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.StatFs
import androidx.core.app.NotificationCompat
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
    private val stateHandler = Handler(Looper.getMainLooper())
    private val stateSeq = AtomicLong()
    private val stateLock = Any()

    @Volatile
    private var stateRateHz = DEFAULT_STATE_RATE_HZ

    @Volatile
    private var stateTimerRunning = false

    private var lastSensorTimestampNs: Long? = null
    private var fpsEstimate = 0.0

    @Volatile
    private var cachedBatteryPercent = 0

    @Volatile
    private var lastBatteryQueryMs = 0L

    @Volatile
    private var cachedStorageFreeMb = 0L

    @Volatile
    private var lastStorageQueryMs = 0L

    @Volatile
    private var activeToken: String? = null

    @Volatile
    private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

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
        createNotificationChannel()
        val notification = buildNotification(token)
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA,
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
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
    }

    private val transportListener =
        object : TransportServer.Listener {
            override fun onSessionOpen(hostName: String?) {
                DeviceLog.i(TAG, "host session open: ${hostName ?: "unknown"}")
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
        val framePipeline = FramePipeline(PipelineConfig()) { frame -> sendFrame(frame) }
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
        controller.startStreaming()
        DeviceLog.i(TAG, "camera $cameraId opening")
    }

    private fun stopCamera() {
        camera?.release()
        camera = null
        pipeline?.stop()
        pipeline = null
        controlState = null
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
        val manualSensor = validateAssignments(store, assignments, results, toApply)
        if (manualSensor) {
            implyAeOff(store, toApply, results)
        }
        val outcome = camera?.applyControls(toApply) ?: ApplyOutcome(false, error = "camera is not open")
        reply(command.id, results, if (outcome.ok) null else outcome.error)
        sendState(full = false)
    }

    /**
     * Validate one batch against the descriptor, recording per-control results
     * in wire order and collecting the accepted values. Returns true when a
     * manual sensor control was accepted.
     */
    private fun validateAssignments(
        store: ControlStateStore,
        assignments: List<ControlAssignment>,
        results: MutableList<JSONObject>,
        toApply: MutableMap<String, Any>,
    ): Boolean {
        var manualSensor = false
        assignments.forEach { assignment ->
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

    /**
     * Manual ISO/exposure only take effect with AE off. Apply it atomically in
     * the same batch and report it so the host sees the effective state.
     */
    private fun implyAeOff(
        store: ControlStateStore,
        toApply: MutableMap<String, Any>,
        results: MutableList<JSONObject>,
    ) {
        if ("ae_mode" in toApply || !store.isSupported("ae_mode")) {
            return
        }
        val validation = store.setRequested("ae_mode", CaptureRequest.CONTROL_AE_MODE_OFF, ControlSource.HOST)
        if (validation.ok) {
            validation.applied?.let { toApply["ae_mode"] = it }
            results += JsonMessages.controlResult("ae_mode", true, applied = validation.applied)
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
        store.setReadout("battery_percent", batteryPercent())
        val dropped = pipeline?.stats()?.dropped ?: 0L
        synchronized(stateLock) {
            val seq = stateSeq.incrementAndGet()
            val payload =
                if (full) {
                    store.fullState(seq, System.currentTimeMillis(), currentCameraId(), dropped, storageFreeMb())
                } else {
                    store.deltaState(seq, System.currentTimeMillis(), currentCameraId(), dropped, storageFreeMb())
                        ?: return
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

    private fun batteryPercent(): Int {
        val now = System.currentTimeMillis()
        if (lastBatteryQueryMs == 0L || now - lastBatteryQueryMs >= BATTERY_QUERY_INTERVAL_MS) {
            val manager = getSystemService(Context.BATTERY_SERVICE) as BatteryManager
            cachedBatteryPercent = manager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
            lastBatteryQueryMs = now
        }
        return cachedBatteryPercent
    }

    private fun storageFreeMb(): Long {
        val now = System.currentTimeMillis()
        if (lastStorageQueryMs == 0L || now - lastStorageQueryMs >= STORAGE_QUERY_INTERVAL_MS) {
            cachedStorageFreeMb = StatFs(filesDir.absolutePath).availableBytes / BYTES_PER_MB
            lastStorageQueryMs = now
        }
        return cachedStorageFreeMb
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
        if (wakeLock == null) {
            val power = getSystemService(Context.POWER_SERVICE) as PowerManager
            wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG)
            wakeLock?.setReferenceCounted(false)
        }
        // Held only between START and STOP; released in stopCapture/onDestroy.
        wakeLock?.acquire()
    }

    private fun releaseWakeLock() {
        wakeLock?.let { lock ->
            if (lock.isHeld) {
                lock.release()
            }
        }
    }

    private fun createNotificationChannel() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.notification_channel_capture),
                NotificationManager.IMPORTANCE_LOW,
            )
        channel.description = getString(R.string.notification_channel_capture_description)
        channel.setShowBadge(false)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(token: String): Notification {
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val stop =
            PendingIntent.getService(
                this,
                1,
                Intent(this, CaptureService::class.java)
                    .setAction(ACTION_STOP)
                    .putExtra(SessionToken.EXTRA, token),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        return NotificationCompat
            .Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_camera)
            .setContentTitle(getString(R.string.notification_capture_title))
            .setContentText(getString(R.string.notification_capture_text))
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .addAction(0, getString(R.string.notification_stop), stop)
            .build()
    }

    companion object {
        private const val TAG = "WatchdogCapture"
        private const val CHANNEL_ID = "watchdog-capture"
        private const val NOTIFICATION_ID = 1001
        private const val WAKE_LOCK_TAG = "watchdog:capture"
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
        private const val BYTES_PER_MB = 1024L * 1024L
        private const val BATTERY_QUERY_INTERVAL_MS = 5_000L
        private const val STORAGE_QUERY_INTERVAL_MS = 5_000L
        private val MANUAL_SENSOR_CONTROLS = setOf("iso", "exposure_time_ns", "frame_duration_ns")

        const val ACTION_START = "app.watchdog.remote.action.START"
        const val ACTION_STOP = "app.watchdog.remote.action.STOP"
        const val ACTION_STATUS = "app.watchdog.remote.action.STATUS"
    }
}
