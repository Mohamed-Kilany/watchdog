package app.watchdog.remote

import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
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
import app.watchdog.remote.camera.OutputConfig
import app.watchdog.remote.camera.OutputConfigValidator
import app.watchdog.remote.camera.OutputConfigs
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

    @Volatile
    private var pipelineConfig = PipelineConfig()

    private val outputs = OutputCoordinator()

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
            cameraStatus = snapshot?.status ?: CameraStatus.CLOSED,
            fps = fpsEstimate.takeIf { it > ZERO_FPS },
            width = pipelineConfig.width,
            height = pipelineConfig.height,
            jpegQuality = pipelineConfig.jpegQuality,
            droppedFrames = pipeline?.stats()?.dropped ?: 0L,
            batteryPercent = readouts.batteryPercent(),
            storageFreeMb = readouts.storageFreeMb(),
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
        val descriptor = capabilities?.optJSONObject(cameraId) ?: JSONObject()
        val store = ControlStateStore(descriptor)
        controlState = store
        val output = outputs.start(cameraId, store)
        if (output != null) {
            DeviceLog.i(TAG, "camera $cameraId opening (${output.streamSize})")
        }
    }

    private fun stopCamera() {
        camera?.release()
        camera = null
        pipeline?.stop()
        pipeline = null
        controlState = null
        outputs.reset()
        pipelineConfig = PipelineConfig()
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
        if (controlState == null) {
            replyNotReady(command.id)
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
        val structuralIndices = assignments.indices.filter { assignments[it].id in OutputConfigs.CONTROL_IDS }
        val structuralResults = HashMap<Int, JSONObject>()
        var cameraSwitched = false
        var structuralChanged = false
        if (structuralIndices.isNotEmpty()) {
            val applied = outputs.applyStructural(assignments, structuralIndices, structuralResults)
            structuralChanged = applied.changed
            cameraSwitched = applied.cameraSwitched
        }
        val store = controlState
        if (store == null) {
            replyNotReady(command.id)
            return
        }
        val outcome = applyBatch(store, assignments, structuralIndices, structuralResults, structuralChanged)
        reply(command.id, outcome.results, outcome.error)
        sendState(full = cameraSwitched)
    }

    private fun replyNotReady(id: String) {
        reply(
            id,
            listOf(
                JsonMessages.controlResult(
                    null,
                    false,
                    errorCode = "CAMERA_NOT_READY",
                    message = "camera is not open",
                ),
            ),
        )
    }

    /** Result of applying the non-structural part of a batch. */
    private data class BatchOutcome(val results: List<JSONObject>, val error: String?)

    private fun applyBatch(
        store: ControlStateStore,
        assignments: List<ControlAssignment>,
        structuralIndices: List<Int>,
        structuralResults: Map<Int, JSONObject>,
        structuralChanged: Boolean,
    ): BatchOutcome {
        val structuralSet = structuralIndices.toHashSet()
        val regular = assignments.filterIndexed { index, _ -> index !in structuralSet }
        val regularResults = mutableListOf<JSONObject>()
        val toApply = LinkedHashMap<String, Any>()
        val triggers = LinkedHashMap<String, Any>()
        val manualSensor = validateAssignments(store, regular, regularResults, toApply, triggers)
        val autoControls =
            if (structuralChanged) {
                outputs.reconcileFps(store, regular, regularResults, toApply)
            } else {
                emptyMap()
            }
        implications.apply(store, toApply, regularResults, triggers, manualSensor)
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
        val results = mergeResults(assignments, structuralResults, regular, regularResults, autoControls)
        return BatchOutcome(results, if (outcome.ok) null else outcome.error)
    }

    private fun mergeResults(
        assignments: List<ControlAssignment>,
        structuralResults: Map<Int, JSONObject>,
        regular: List<ControlAssignment>,
        regularResults: List<JSONObject>,
        autoControls: Map<String, Any>,
    ): List<JSONObject> {
        val results = ArrayList<JSONObject>(assignments.size + regularResults.size + autoControls.size)
        var regularIndex = 0
        assignments.indices.forEach { index ->
            val structural = structuralResults[index]
            if (structural != null) {
                results += structural
            } else if (regularIndex < regular.size) {
                results += regularResults[regularIndex++]
            }
        }
        results += regularResults.drop(regularIndex)
        autoControls.forEach { (id, value) ->
            results += JsonMessages.controlResult(id, true, applied = value)
        }
        return results
    }

    /**
     * M4-T09 output configuration: owns the structural controls, the live
     * pipeline and the capture-session reconfiguration they trigger. It stays
     * inside the service because applying a configuration spans the pipeline,
     * controller and canonical state, whose ordering the service coordinates.
     */
    private inner class OutputCoordinator {
        @Volatile
        var config: OutputConfig? = null
            private set

        fun start(cameraId: String, store: ControlStateStore): OutputConfig? {
            val descriptor = capabilities?.optJSONObject(cameraId) ?: JSONObject()
            val output = OutputConfigs.defaults(cameraId, descriptor)
            config = output
            val pipelineSettings = pipelineConfigFor(output)
            pipelineConfig = pipelineSettings
            val framePipeline = FramePipeline(pipelineSettings) { frame -> sendFrame(frame) }
            framePipeline.start()
            val surface = framePipeline.surface
            if (surface == null) {
                DeviceLog.e(TAG, "frame pipeline produced no capture surface")
                framePipeline.stop()
                return null
            }
            val controller = Camera2Controller(this@CaptureService, cameraListener)
            camera = controller
            pipeline = framePipeline
            recordState(store, output, emptySet(), OutputConfigs.CONTROL_IDS)
            controller.open(cameraId, listOf(surface))
            controller.setPreviewSurface(previewSurface)
            applyDefaultFps(store, output, controller)
            controller.startStreaming()
            return output
        }

        fun reset() {
            config = null
        }

        /** Outcome of applying the structural part of a batch. */
        data class StructuralOutcome(val changed: Boolean, val cameraSwitched: Boolean)

        /**
         * Validate the output-configuration assignments as one atomic batch and
         * rebuild the pipeline/session when needed. Every structural assignment
         * gets a result, whether the batch applied or not.
         */
        fun applyStructural(
            assignments: List<ControlAssignment>,
            indices: List<Int>,
            results: MutableMap<Int, JSONObject>,
        ): StructuralOutcome {
            val current = config
            if (current == null) {
                indices.forEach { index ->
                    results[index] =
                        controlError(assignments[index].id, "CAMERA_NOT_READY", "camera is not open")
                }
                return StructuralOutcome(false, false)
            }
            val validation =
                OutputConfigValidator.validate(
                    current,
                    indices.map { assignments[it].id to assignments[it].value },
                    capabilities ?: JSONObject(),
                )
            if (!validation.ok) {
                indices.forEach { index ->
                    val id = assignments[index].id
                    val error = validation.errors[id]
                    results[index] =
                        if (error != null) {
                            controlError(id, error.code, error.message)
                        } else {
                            controlError(id, "ROLLED_BACK", "output configuration rolled back")
                        }
                }
                return StructuralOutcome(false, false)
            }
            val outcome = applyOutputConfig(current, validation.config, validation.applied.keys)
            if (!outcome.ok) {
                indices.forEach { index ->
                    results[index] =
                        controlError(
                            assignments[index].id,
                            "APPLY_FAILED",
                            outcome.error ?: "output configuration failed",
                        )
                }
                return StructuralOutcome(false, false)
            }
            indices.forEach { index ->
                val id = assignments[index].id
                results[index] = JsonMessages.controlResult(id, true, applied = validation.applied[id])
            }
            val changed =
                current.cameraId != validation.config.cameraId ||
                    current.streamSize != validation.config.streamSize
            return StructuralOutcome(changed, current.cameraId != validation.config.cameraId)
        }

        /**
         * Rebuild the frame pipeline and capture session for a new output
         * configuration. Quality/rotation only need the pipeline's per-frame
         * config; video and still settings are recorded for M5.
         */
        private fun applyOutputConfig(previous: OutputConfig, next: OutputConfig, applied: Set<String>): ApplyOutcome {
            val controller = camera ?: return ApplyOutcome(false, error = "camera is not open")
            val cameraChanged = previous.cameraId != next.cameraId
            val sizeChanged = previous.streamSize != next.streamSize
            val physicalChanged = previous.physicalId != next.physicalId
            val qualityChanged = previous.jpegQuality != next.jpegQuality
            val rotationChanged = previous.rotation != next.rotation
            if (cameraChanged) {
                val descriptor = capabilities?.optJSONObject(next.cameraId) ?: JSONObject()
                controlState = ControlStateStore(descriptor)
                implications.reset()
                lastSensorTimestampNs = null
                fpsEstimate = ZERO_FPS
            }
            val store = controlState ?: return ApplyOutcome(false, error = "camera is not open")
            if (cameraChanged || sizeChanged) {
                val pipelineSettings = pipelineConfigFor(next)
                val framePipeline = FramePipeline(pipelineSettings) { frame -> sendFrame(frame) }
                framePipeline.start()
                val surface = framePipeline.surface
                if (surface == null) {
                    framePipeline.stop()
                    return ApplyOutcome(false, error = "frame pipeline produced no capture surface")
                }
                val previousPipeline = pipeline
                pipeline = framePipeline
                pipelineConfig = pipelineSettings
                controller.reconfigure(next.cameraId, listOf(surface), next.physicalId) {
                    previousPipeline?.stop()
                }
            } else if (physicalChanged) {
                val surfaces = pipeline?.surface?.let { listOf(it) } ?: emptyList()
                controller.reconfigure(next.cameraId, surfaces, next.physicalId)
            } else if (qualityChanged || rotationChanged) {
                pipelineConfig = pipelineConfig.copy(jpegQuality = next.jpegQuality, rotation = next.rotation)
                pipeline?.updateConfig(pipelineConfig)
            }
            config = next
            val auto =
                OutputConfigs.CONTROL_IDS
                    .filter { id ->
                        id !in applied && outputValue(previous, id)?.toString() != outputValue(next, id)?.toString()
                    }.toSet()
            recordState(store, next, applied, auto)
            return ApplyOutcome(true)
        }

        private fun pipelineConfigFor(output: OutputConfig): PipelineConfig =
            PipelineConfig(
                width = output.streamSize.width,
                height = output.streamSize.height,
                jpegQuality = output.jpegQuality,
                rotation = output.rotation,
                targetFps = OutputConfigs.PREFERRED_MAX_FPS,
            )

        /**
         * Write the output configuration into the control state. Explicitly
         * requested values are sourced `host`; dependent values that changed as
         * a consequence are sourced `auto`, so the host can tell them apart.
         */
        private fun recordState(
            store: ControlStateStore,
            output: OutputConfig,
            applied: Set<String>,
            auto: Set<String>,
        ) {
            OutputConfigs.CONTROL_IDS.forEach { id ->
                val value = outputValue(output, id) ?: return@forEach
                when (id) {
                    in applied -> store.setStructural(id, value, ControlSource.HOST)
                    in auto -> store.setStructural(id, value, ControlSource.AUTO)
                    else -> store.setStructural(id, value)
                }
            }
        }

        private fun outputValue(output: OutputConfig, id: String): Any? =
            when (id) {
                OutputConfigs.CAMERA_ID -> output.cameraId
                OutputConfigs.PHYSICAL -> output.physicalId ?: OutputConfigs.LOGICAL
                OutputConfigs.STREAM_SIZE -> output.streamSize.toJson()
                OutputConfigs.JPEG_QUALITY -> output.jpegQuality
                OutputConfigs.STREAM_ROTATION -> output.rotation
                OutputConfigs.STILL_FORMAT -> output.stillFormat
                OutputConfigs.STILL_SIZE -> output.stillSize.toJson()
                OutputConfigs.VIDEO_SIZE -> output.videoSize.toJson()
                OutputConfigs.VIDEO_BITRATE -> output.videoBitrate
                OutputConfigs.VIDEO_CODEC -> output.videoCodec
                OutputConfigs.VIDEO_CONTAINER -> output.videoContainer
                OutputConfigs.AUDIO -> output.audio
                else -> null
            }

        private fun applyDefaultFps(store: ControlStateStore, output: OutputConfig, controller: CameraController) {
            val descriptor = capabilities?.optJSONObject(output.cameraId) ?: return
            val range = OutputConfigs.defaultFpsRange(descriptor, output.streamSize) ?: return
            val validation = store.setRequested(FPS_CONTROL, range, ControlSource.AUTO)
            if (!validation.ok) {
                return
            }
            validation.applied?.let { controller.applyControls(mapOf(FPS_CONTROL to it)) }
        }

        /**
         * Keep the FPS range valid after a stream-size or camera change. An
         * explicitly requested FPS range is re-checked by [validateAssignments];
         * here the previous range is kept when it still fits, otherwise the best
         * supported range is applied and sourced `auto`.
         */
        fun reconcileFps(
            store: ControlStateStore,
            regular: List<ControlAssignment>,
            results: MutableList<JSONObject>,
            toApply: MutableMap<String, Any>,
        ): Map<String, Any> {
            val output = config ?: return emptyMap()
            val descriptor = capabilities?.optJSONObject(output.cameraId) ?: return emptyMap()
            val current = store.requestedValues()[FPS_CONTROL]
            if (current != null && OutputConfigs.isFpsSupported(descriptor, output.streamSize, current)) {
                return emptyMap()
            }
            val explicit = regular.any { it.id == FPS_CONTROL }
            val rejected = results.any { it.optString("control") == FPS_CONTROL && !it.optBoolean("ok") }
            if (explicit && !rejected) {
                return emptyMap()
            }
            val range = OutputConfigs.defaultFpsRange(descriptor, output.streamSize) ?: return emptyMap()
            val validation = store.setRequested(FPS_CONTROL, range, ControlSource.AUTO)
            if (!validation.ok) {
                return emptyMap()
            }
            val applied = validation.applied ?: return emptyMap()
            toApply[FPS_CONTROL] = applied
            return mapOf(FPS_CONTROL to applied)
        }

        fun fpsFitsCurrent(value: Any?): Boolean {
            val output = config ?: return true
            val descriptor = capabilities?.optJSONObject(output.cameraId) ?: return true
            return OutputConfigs.isFpsSupported(descriptor, output.streamSize, value)
        }

        private fun controlError(id: String, code: String?, message: String?): JSONObject =
            JsonMessages.controlResult(id, false, errorCode = code, message = message)
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
            val validation = store.validateControl(assignment.id, assignment.value)
            if (!validation.ok) {
                results +=
                    JsonMessages.controlResult(
                        assignment.id,
                        false,
                        errorCode = validation.errorCode,
                        message = validation.message,
                    )
                return@forEach
            }
            if (assignment.id == FPS_CONTROL && !outputs.fpsFitsCurrent(validation.applied)) {
                val size = outputs.config?.streamSize
                results +=
                    JsonMessages.controlResult(
                        assignment.id,
                        false,
                        errorCode = "UNSUPPORTED_COMBINATION",
                        message =
                            "FPS range ${validation.applied} cannot sustain the current " +
                                "stream size $size; lower the FPS or choose a smaller stream_size",
                    )
                return@forEach
            }
            val recorded = store.setRequested(assignment.id, validation.applied, ControlSource.HOST)
            if (!recorded.ok) {
                results +=
                    JsonMessages.controlResult(
                        assignment.id,
                        false,
                        errorCode = recorded.errorCode,
                        message = recorded.message,
                    )
                return@forEach
            }
            results += JsonMessages.controlResult(assignment.id, true, applied = recorded.applied)
            recorded.applied?.let { toApply[assignment.id] = it }
            if (assignment.id in MANUAL_SENSOR_CONTROLS) {
                manualSensor = true
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
        private const val FPS_CONTROL = "ae_fps_range"
        private val MANUAL_SENSOR_CONTROLS = setOf("iso", "exposure_time_ns", "frame_duration_ns")
        private val TRIGGERS = setOf("ae_precapture", "af_trigger")

        const val ACTION_START = "app.watchdog.remote.action.START"
        const val ACTION_STOP = "app.watchdog.remote.action.STOP"
        const val ACTION_STATUS = "app.watchdog.remote.action.STATUS"
    }
}
