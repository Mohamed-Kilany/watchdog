package app.watchdog.remote.camera

import android.content.Context
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.os.Build
import app.watchdog.remote.log.DeviceLog

/** Enumerated camera identity, used in `HELLO_ACK` and the on-device UI. */
data class CameraInfo(
    val id: String,
    val facing: String,
    val hardwareLevel: String,
    val logicalMultiCamera: Boolean,
    val physicalIds: List<String>,
)

/** Enumerates cameras without opening them (M3-T04, descriptor in M4-T01). */
object CameraCatalog {
    private const val TAG = "WatchdogCamera"

    fun list(context: Context): List<CameraInfo> {
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        return try {
            manager.cameraIdList.mapNotNull { id -> info(manager, id) }
        } catch (e: CameraAccessException) {
            DeviceLog.e(TAG, "cannot enumerate cameras: ${e.message}")
            emptyList()
        }
    }

    private fun info(manager: CameraManager, id: String): CameraInfo? =
        try {
            val chars = manager.getCameraCharacteristics(id)
            val physicalIds = logicalIds(chars)
            CameraInfo(
                id = id,
                facing = facingName(chars.get(CameraCharacteristics.LENS_FACING)),
                hardwareLevel = hardwareLevelName(chars.get(CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL)),
                logicalMultiCamera = physicalIds.isNotEmpty(),
                physicalIds = physicalIds,
            )
        } catch (e: CameraAccessException) {
            DeviceLog.w(TAG, "camera $id unavailable: ${e.message}")
            null
        }

    private fun logicalIds(chars: CameraCharacteristics): List<String> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            chars.get(CameraCharacteristics.LOGICAL_MULTI_CAMERA)?.physicalCameraIds?.sorted() ?: emptyList()
        } else {
            emptyList()
        }

    private fun facingName(facing: Int?): String =
        when (facing) {
            CameraCharacteristics.LENS_FACING_BACK -> "back"
            CameraCharacteristics.LENS_FACING_FRONT -> "front"
            CameraCharacteristics.LENS_FACING_EXTERNAL -> "external"
            else -> "unknown"
        }

    private fun hardwareLevelName(level: Int?): String =
        when (level) {
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LEGACY -> "LEGACY"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LIMITED -> "LIMITED"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_FULL -> "FULL"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_3 -> "LEVEL_3"
            else -> "UNKNOWN"
        }
}
