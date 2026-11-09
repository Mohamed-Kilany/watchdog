package app.watchdog.remote.camera

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.TotalCaptureResult
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
import androidx.core.content.ContextCompat
import app.watchdog.remote.log.DeviceLog
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Result of applying a control batch to the active repeating request. */
data class ApplyOutcome(val ok: Boolean, val applied: Map<String, Any> = emptyMap(), val error: String? = null)

/** Camera lifecycle API used by the service and the frame pipeline. */
interface CameraController {
    val snapshot: CameraSnapshot

    /** Characteristics of the open camera (null before `open`). */
    val cameraCharacteristics: CameraCharacteristics?

    fun open(cameraId: String, surfaces: List<Surface>)

    /**
     * Attach or detach the on-device preview target. The camera session is
     * rebuilt with the same controls and the repeating request resumes, so the
     * host stream only pauses for the reconfiguration. A destroyed surface
     * must be detached before it becomes invalid.
     */
    fun setPreviewSurface(surface: Surface?)

    fun startStreaming()

    fun stopStreaming()

    /**
     * Apply a validated control batch atomically in the next repeating request.
     * Returns the effective requested values or an error; actual values arrive
     * later through `TotalCaptureResult`.
     */
    fun applyControls(controls: Map<String, Any>): ApplyOutcome

    /**
     * Fire a one-shot trigger (`ae_precapture`) without changing the repeating
     * request. Returns an error when the camera is not streaming.
     */
    fun trigger(id: String, value: Any): ApplyOutcome

    fun close()

    fun release()

    /** Callbacks are invoked on the camera handler thread. */
    interface Listener {
        fun onStateChanged(snapshot: CameraSnapshot)

        fun onCaptureResult(result: TotalCaptureResult)
    }
}

/**
 * Camera2 implementation with a single handler thread and strict open/close
 * ordering: the session is closed before the device, and late callbacks from a
 * closed device are ignored by [CameraStateMachine].
 *
 * Interruptions (another app taking the camera, `onDisconnected`, service
 * errors) move the machine to RECOVERING/ERROR and schedule a bounded-backoff
 * reopen unless the user closed the camera.
 */
class Camera2Controller(context: Context, private val listener: CameraController.Listener) : CameraController {
    private val appContext = context.applicationContext
    private val manager = appContext.getSystemService(Context.CAMERA_SERVICE) as CameraManager
    private val thread = HandlerThread("watchdog-camera").apply { start() }
    private val handler = Handler(thread.looper)
    private val machine = CameraStateMachine()

    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var surfaces: List<Surface> = emptyList()
    private var previewSurface: Surface? = null
    private var requestedCameraId: String? = null

    @Volatile
    private var characteristics: CameraCharacteristics? = null
    private val requested = LinkedHashMap<String, Any>()
    private var desiredStreaming = false
    private var closedByUser = true
    private var recoveryScheduled = false

    override val snapshot: CameraSnapshot
        get() = machine.snapshot

    override val cameraCharacteristics: CameraCharacteristics?
        get() = characteristics

    override fun open(cameraId: String, surfaces: List<Surface>) {
        val chars =
            try {
                manager.getCameraCharacteristics(cameraId)
            } catch (e: CameraAccessException) {
                DeviceLog.w(TAG, "characteristics unavailable for $cameraId: ${e.message}")
                null
            }
        handler.post {
            closedByUser = false
            if (requestedCameraId != cameraId) {
                requested.clear()
            }
            requestedCameraId = cameraId
            characteristics = chars
            this.surfaces = surfaces.toList()
            machine.onEvent(CameraEvent.Open(cameraId))
            publish()
            openDevice(cameraId)
        }
    }

    override fun setPreviewSurface(surface: Surface?) {
        handler.post {
            val next = surface?.takeIf { it.isValid }
            if (previewSurface === next) {
                return@post
            }
            previewSurface = next
            val camera = device
            val active = session
            if (camera != null && active != null) {
                DeviceLog.i(
                    TAG,
                    "preview surface ${if (next == null) "detached" else "attached"}; reconfiguring capture session",
                )
                session = null
                active.close()
                createSession(camera)
            }
        }
    }

    override fun startStreaming() {
        handler.post {
            desiredStreaming = true
            val active = session
            if (active != null && machine.snapshot.status == CameraStatus.CONFIGURED) {
                startRepeating(active)
            }
        }
    }

    override fun stopStreaming() {
        handler.post {
            desiredStreaming = false
            val active = session
            if (active != null) {
                try {
                    active.stopRepeating()
                    active.abortCaptures()
                } catch (e: CameraAccessException) {
                    DeviceLog.w(TAG, "stopRepeating failed: ${e.message}")
                } catch (e: IllegalStateException) {
                    DeviceLog.w(TAG, "stopRepeating invalid state: ${e.message}")
                }
            }
            machine.onEvent(CameraEvent.StreamStopped)
            publish()
        }
    }

    override fun applyControls(controls: Map<String, Any>): ApplyOutcome {
        if (controls.isEmpty()) {
            return ApplyOutcome(true)
        }
        return runOnCameraThread("control") { applyOnHandler(controls) }
    }

    private fun runOnCameraThread(label: String, action: () -> ApplyOutcome): ApplyOutcome {
        val latch = CountDownLatch(1)
        var outcome = ApplyOutcome(false, error = "camera thread unavailable")
        val posted =
            handler.post {
                // Assign before counting down. Kotlin runs a `finally` attached
                // to an assignment before storing the result, so counting down
                // from it lets the waiting thread read the initial value even
                // when the action succeeded.
                try {
                    outcome =
                        try {
                            action()
                        } catch (e: CameraAccessException) {
                            ApplyOutcome(false, error = "$label failed: ${e.message}")
                        } catch (e: IllegalStateException) {
                            ApplyOutcome(false, error = "$label invalid state: ${e.message}")
                        } catch (e: IllegalArgumentException) {
                            // A value the HAL rejects must not kill the camera thread.
                            ApplyOutcome(false, error = "invalid $label: ${e.message}")
                        }
                } finally {
                    latch.countDown()
                }
            }
        if (!posted) {
            return ApplyOutcome(false, error = "camera thread unavailable")
        }
        return if (latch.await(APPLY_TIMEOUT_MS, TimeUnit.MILLISECONDS)) {
            outcome
        } else {
            ApplyOutcome(false, error = "camera did not acknowledge the $label")
        }
    }

    private fun applyOnHandler(controls: Map<String, Any>): ApplyOutcome {
        val previous = HashMap(requested)
        requested.putAll(controls)
        val camera = device
        val active = session
        val chars = characteristics
        // Before the session exists the batch is remembered and becomes part of
        // the first repeating request; there is nothing to apply yet.
        if (camera == null || active == null || chars == null) {
            return ApplyOutcome(true, applied = controls)
        }
        // Only replace the repeating request while streaming; a change made
        // between configure and start is picked up by `startRepeating`.
        if (machine.snapshot.status == CameraStatus.STREAMING) {
            try {
                active.setRepeatingRequest(requestBuilder(camera, chars).build(), captureCallback, handler)
            } catch (e: IllegalArgumentException) {
                // Never keep a value the HAL rejected: the next request would
                // fail again and the batch would silently poison the session.
                requested.clear()
                requested.putAll(previous)
                return ApplyOutcome(false, error = "invalid control: ${e.message}")
            }
        }
        return ApplyOutcome(true, applied = controls)
    }

    override fun trigger(id: String, value: Any): ApplyOutcome =
        runOnCameraThread("trigger") { triggerOnHandler(id, value) }

    private fun triggerOnHandler(id: String, value: Any): ApplyOutcome {
        val camera = device
        val active = session
        val chars = characteristics
        if (camera == null || active == null || chars == null) {
            return ApplyOutcome(false, error = "camera is not open")
        }
        if (machine.snapshot.status != CameraStatus.STREAMING) {
            return ApplyOutcome(false, error = "camera is not streaming")
        }
        val (key, trigger) =
            when (id) {
                "ae_precapture" ->
                    CaptureRequest.CONTROL_AE_PRECAPTURE_TRIGGER to
                        if (value.toString() == "start") {
                            CaptureRequest.CONTROL_AE_PRECAPTURE_TRIGGER_START
                        } else {
                            CaptureRequest.CONTROL_AE_PRECAPTURE_TRIGGER_CANCEL
                        }
                "af_trigger" ->
                    CaptureRequest.CONTROL_AF_TRIGGER to
                        if (value.toString() == "start") {
                            CaptureRequest.CONTROL_AF_TRIGGER_START
                        } else {
                            CaptureRequest.CONTROL_AF_TRIGGER_CANCEL
                        }
                else -> return ApplyOutcome(false, error = "unknown trigger '$id'")
            }
        val request =
            requestBuilder(camera, chars)
                .apply { set(key, trigger) }
                .build()
        active.capture(request, captureCallback, handler)
        return ApplyOutcome(true, applied = mapOf(id to value))
    }

    override fun close() {
        handler.post {
            closedByUser = true
            desiredStreaming = false
            handler.removeCallbacks(recoveryRunnable)
            recoveryScheduled = false
            session?.close()
            session = null
            device?.close()
            device = null
            requested.clear()
            characteristics = null
            machine.onEvent(CameraEvent.Close)
            publish()
        }
    }

    override fun release() {
        close()
        handler.post { thread.quitSafely() }
    }

    private fun openDevice(cameraId: String) {
        val permissionGranted =
            ContextCompat.checkSelfPermission(appContext, android.Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        when {
            !permissionGranted -> fail("CAMERA permission not granted")
            else ->
                try {
                    manager.openCamera(cameraId, deviceCallback, handler)
                } catch (e: CameraAccessException) {
                    fail("openCamera($cameraId) failed: ${e.message}")
                } catch (e: SecurityException) {
                    fail("openCamera($cameraId) denied: ${e.message}")
                } catch (e: IllegalArgumentException) {
                    fail("camera $cameraId not available: ${e.message}")
                }
        }
    }

    private fun createSession(camera: CameraDevice) {
        val targets = targets()
        if (targets.isEmpty()) {
            machine.onEvent(CameraEvent.Configured)
            publish()
        } else {
            try {
                camera.createCaptureSession(targets, sessionCallback, handler)
            } catch (e: CameraAccessException) {
                fail("createCaptureSession failed: ${e.message}")
            } catch (e: IllegalStateException) {
                fail("createCaptureSession invalid state: ${e.message}")
            }
        }
    }

    /** Analysis surfaces plus the optional on-device preview surface. */
    private fun targets(): List<Surface> {
        val preview = previewSurface
        return if (preview != null && preview.isValid) surfaces + preview else surfaces
    }

    private fun startRepeating(session: CameraCaptureSession) {
        val camera = device
        val chars = characteristics
        if (camera == null || chars == null || targets().isEmpty()) {
            fail("cannot start streaming: camera or surfaces unavailable")
            return
        }
        try {
            session.setRepeatingRequest(requestBuilder(camera, chars).build(), captureCallback, handler)
            machine.onEvent(CameraEvent.StreamStarted)
            publish()
        } catch (e: CameraAccessException) {
            fail("setRepeatingRequest failed: ${e.message}")
        } catch (e: IllegalStateException) {
            fail("setRepeatingRequest invalid state: ${e.message}")
        } catch (e: IllegalArgumentException) {
            // Never let one rejected value kill the camera thread; drop the
            // remembered batch so the next recovery can stream again.
            requested.clear()
            fail("setRepeatingRequest rejected controls: ${e.message}")
        }
    }

    private fun requestBuilder(camera: CameraDevice, chars: CameraCharacteristics): CaptureRequest.Builder {
        val builder = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
        targets().forEach { builder.addTarget(it) }
        builder.set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
        requested.forEach { (id, value) -> CameraControls.applyTo(builder, chars, id, value) }
        return builder
    }

    private fun scheduleRecovery(reason: String, baseDelayMs: Long = BASE_RECOVERY_DELAY_MS) {
        if (closedByUser || recoveryScheduled) {
            return
        }
        recoveryScheduled = true
        val attempt = machine.snapshot.recoveries + 1
        val delayMs = minOf(MAX_RECOVERY_DELAY_MS, baseDelayMs * attempt)
        DeviceLog.i(TAG, "scheduling recovery in ${delayMs}ms after $reason")
        handler.postDelayed(recoveryRunnable, delayMs)
    }

    private val recoveryRunnable =
        Runnable {
            recoveryScheduled = false
            val cameraId = requestedCameraId
            if (!closedByUser && cameraId != null) {
                machine.onEvent(CameraEvent.Recover)
                publish()
                openDevice(cameraId)
            }
        }

    private val deviceCallback =
        object : CameraDevice.StateCallback() {
            override fun onOpened(camera: CameraDevice) {
                device = camera
                machine.onEvent(CameraEvent.Opened)
                publish()
                createSession(camera)
            }

            override fun onDisconnected(camera: CameraDevice) {
                camera.close()
                device = null
                session = null
                machine.onEvent(CameraEvent.Disconnected)
                publish()
                scheduleRecovery("disconnected")
            }

            override fun onError(camera: CameraDevice, error: Int) {
                camera.close()
                device = null
                session = null
                val reason =
                    when (error) {
                        CameraDevice.StateCallback.ERROR_CAMERA_IN_USE -> "camera in use by another app"
                        CameraDevice.StateCallback.ERROR_MAX_CAMERAS_IN_USE -> "too many cameras in use"
                        CameraDevice.StateCallback.ERROR_CAMERA_DISABLED -> "camera disabled by policy"
                        CameraDevice.StateCallback.ERROR_CAMERA_DEVICE -> "camera device error"
                        CameraDevice.StateCallback.ERROR_CAMERA_SERVICE -> "camera service error"
                        else -> "camera error $error"
                    }
                val inUse =
                    error == CameraDevice.StateCallback.ERROR_CAMERA_IN_USE ||
                        error == CameraDevice.StateCallback.ERROR_MAX_CAMERAS_IN_USE
                machine.onEvent(CameraEvent.Failed(reason, inUse))
                publish()
                val delay = if (inUse) CAMERA_IN_USE_DELAY_MS else BASE_RECOVERY_DELAY_MS
                scheduleRecovery("error $error", delay)
            }

            override fun onClosed(camera: CameraDevice) {
                // The HAL also reports Closed after we close a failed device in
                // onError/onDisconnected; only a user close may reset the state.
                if (closedByUser) {
                    machine.onEvent(CameraEvent.Closed)
                }
                publish()
            }
        }

    private val sessionCallback =
        object : CameraCaptureSession.StateCallback() {
            override fun onConfigured(session: CameraCaptureSession) {
                this@Camera2Controller.session = session
                machine.onEvent(CameraEvent.Configured)
                publish()
                if (desiredStreaming) {
                    startRepeating(session)
                }
            }

            override fun onConfigureFailed(session: CameraCaptureSession) {
                fail("capture session configuration failed")
            }
        }

    private val captureCallback =
        object : CameraCaptureSession.CaptureCallback() {
            override fun onCaptureCompleted(
                session: CameraCaptureSession,
                request: CaptureRequest,
                result: TotalCaptureResult,
            ) {
                listener.onCaptureResult(result)
            }
        }

    private fun fail(reason: String) {
        DeviceLog.e(TAG, reason)
        machine.onEvent(CameraEvent.Failed(reason))
        publish()
    }

    private fun publish() {
        listener.onStateChanged(machine.snapshot)
    }

    companion object {
        private const val TAG = "WatchdogCamera2"
        private const val BASE_RECOVERY_DELAY_MS = 1_000L
        private const val CAMERA_IN_USE_DELAY_MS = 3_000L
        private const val MAX_RECOVERY_DELAY_MS = 10_000L
        private const val APPLY_TIMEOUT_MS = 2_000L
    }
}
