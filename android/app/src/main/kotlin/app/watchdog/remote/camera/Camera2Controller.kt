package app.watchdog.remote.camera

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.TotalCaptureResult
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import androidx.core.content.ContextCompat

/** Camera lifecycle API used by the service and the frame pipeline. */
interface CameraController {
    val snapshot: CameraSnapshot

    fun open(cameraId: String, surfaces: List<Surface>)

    fun startStreaming()

    fun stopStreaming()

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
    private var requestedCameraId: String? = null
    private var desiredStreaming = false
    private var closedByUser = true
    private var recoveryScheduled = false

    override val snapshot: CameraSnapshot
        get() = machine.snapshot

    override fun open(cameraId: String, surfaces: List<Surface>) {
        handler.post {
            closedByUser = false
            requestedCameraId = cameraId
            this.surfaces = surfaces.toList()
            machine.onEvent(CameraEvent.Open(cameraId))
            publish()
            openDevice(cameraId)
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
                    Log.w(TAG, "stopRepeating failed: ${e.message}")
                } catch (e: IllegalStateException) {
                    Log.w(TAG, "stopRepeating invalid state: ${e.message}")
                }
            }
            machine.onEvent(CameraEvent.StreamStopped)
            publish()
        }
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
            machine.onEvent(CameraEvent.Close)
            publish()
        }
    }

    override fun release() {
        close()
        handler.post { thread.quitSafely() }
    }

    private fun openDevice(cameraId: String) {
        when {
            !hasCameraPermission() -> fail("CAMERA permission not granted")
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
        val targets = surfaces
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

    private fun startRepeating(session: CameraCaptureSession) {
        val camera = device
        if (camera == null || surfaces.isEmpty()) {
            fail("cannot start streaming: camera or surfaces unavailable")
        } else {
            try {
                val request =
                    camera
                        .createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                        .apply {
                            surfaces.forEach { addTarget(it) }
                            set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
                        }.build()
                session.setRepeatingRequest(request, captureCallback, handler)
                machine.onEvent(CameraEvent.StreamStarted)
                publish()
            } catch (e: CameraAccessException) {
                fail("setRepeatingRequest failed: ${e.message}")
            } catch (e: IllegalStateException) {
                fail("setRepeatingRequest invalid state: ${e.message}")
            }
        }
    }

    private fun scheduleRecovery(reason: String, baseDelayMs: Long = BASE_RECOVERY_DELAY_MS) {
        if (closedByUser || recoveryScheduled) {
            return
        }
        recoveryScheduled = true
        val attempt = machine.snapshot.recoveries + 1
        val delayMs = minOf(MAX_RECOVERY_DELAY_MS, baseDelayMs * attempt)
        Log.i(TAG, "scheduling recovery in ${delayMs}ms after $reason")
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
                machine.onEvent(CameraEvent.Failed(describeError(error), isCameraInUse(error)))
                publish()
                val delay = if (isCameraInUse(error)) CAMERA_IN_USE_DELAY_MS else BASE_RECOVERY_DELAY_MS
                scheduleRecovery("error $error", delay)
            }

            override fun onClosed(camera: CameraDevice) {
                machine.onEvent(CameraEvent.Closed)
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

    private fun hasCameraPermission(): Boolean =
        ContextCompat.checkSelfPermission(appContext, android.Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    private fun fail(reason: String) {
        Log.e(TAG, reason)
        machine.onEvent(CameraEvent.Failed(reason))
        publish()
    }

    private fun publish() {
        listener.onStateChanged(machine.snapshot)
    }

    private fun describeError(error: Int): String =
        when (error) {
            CameraDevice.StateCallback.ERROR_CAMERA_IN_USE -> "camera in use by another app"
            CameraDevice.StateCallback.ERROR_MAX_CAMERAS_IN_USE -> "too many cameras in use"
            CameraDevice.StateCallback.ERROR_CAMERA_DISABLED -> "camera disabled by policy"
            CameraDevice.StateCallback.ERROR_CAMERA_DEVICE -> "camera device error"
            CameraDevice.StateCallback.ERROR_CAMERA_SERVICE -> "camera service error"
            else -> "camera error $error"
        }

    private fun isCameraInUse(error: Int): Boolean =
        error == CameraDevice.StateCallback.ERROR_CAMERA_IN_USE ||
            error == CameraDevice.StateCallback.ERROR_MAX_CAMERAS_IN_USE

    companion object {
        private const val TAG = "WatchdogCamera2"
        private const val BASE_RECOVERY_DELAY_MS = 1_000L
        private const val CAMERA_IN_USE_DELAY_MS = 3_000L
        private const val MAX_RECOVERY_DELAY_MS = 10_000L
    }
}
