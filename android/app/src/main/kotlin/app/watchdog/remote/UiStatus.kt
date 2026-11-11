package app.watchdog.remote

import app.watchdog.remote.camera.CameraStatus

/**
 * Live values for the on-device status panels (M3-T08). [CaptureService]
 * builds one on demand so the activity never touches camera or transport
 * internals; defaults describe the unprovisioned state.
 */
data class UiStatus(
    val hostConnected: Boolean = false,
    val hostName: String? = null,
    val cameraId: String? = null,
    val cameraFacing: String? = null,
    val cameraStatus: CameraStatus = CameraStatus.CLOSED,
    val fps: Double? = null,
    val width: Int = 0,
    val height: Int = 0,
    val jpegQuality: Int = 0,
    val droppedFrames: Long = 0L,
    val batteryPercent: Int = 0,
    val storageFreeMb: Long = 0L,
    val temperatureC: Double? = null,
)
