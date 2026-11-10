package app.watchdog.remote.camera

import kotlin.math.roundToInt

/** Pixel crop inside the sensor active array. */
internal data class CropRegion(val left: Int, val top: Int, val right: Int, val bottom: Int)

/**
 * Digital zoom math shared by the Camera2 request builder (M4-T07).
 *
 * Kept free of Android types so the legacy `SCALER_CROP_REGION` fallback is
 * covered by plain JVM unit tests on every CI platform. Coordinates are sensor
 * active-array pixels; the crop is always centred like `CONTROL_ZOOM_RATIO`.
 */
internal object ZoomMath {
    /** Crop region for [ratio] inside the active array. */
    fun cropRegion(activeLeft: Int, activeTop: Int, activeWidth: Int, activeHeight: Int, ratio: Double): CropRegion? {
        if (activeWidth <= 0 || activeHeight <= 0) {
            return null
        }
        if (!ratio.isFinite() || ratio <= 0.0) {
            return null
        }
        val effective = ratio.coerceAtLeast(1.0)
        val width = (activeWidth / effective).roundToInt().coerceIn(1, activeWidth)
        val height = (activeHeight / effective).roundToInt().coerceIn(1, activeHeight)
        val left = activeLeft + (activeWidth - width) / 2
        val top = activeTop + (activeHeight - height) / 2
        return CropRegion(left, top, left + width, top + height)
    }

    /** Zoom ratio implied by a crop region width, or `null` for an invalid crop. */
    fun ratioFromCrop(activeWidth: Int, cropWidth: Int): Double? {
        if (activeWidth <= 0 || cropWidth <= 0 || cropWidth > activeWidth) {
            return null
        }
        return activeWidth.toDouble() / cropWidth
    }
}
