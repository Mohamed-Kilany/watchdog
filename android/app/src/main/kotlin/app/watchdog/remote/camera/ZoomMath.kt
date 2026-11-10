package app.watchdog.remote.camera

import kotlin.math.roundToInt

/**
 * Digital zoom math shared by the Camera2 request builder (M4-T07).
 *
 * Kept free of Android types so the legacy `SCALER_CROP_REGION` fallback is
 * covered by plain JVM unit tests on every CI platform. Coordinates are sensor
 * active-array pixels; the crop is always centred like `CONTROL_ZOOM_RATIO`.
 */
internal object ZoomMath {
    /** Crop region `[left, top, right, bottom]` for [ratio] inside the active array. */
    fun cropRegion(
        activeLeft: Int,
        activeTop: Int,
        activeWidth: Int,
        activeHeight: Int,
        ratio: Double,
    ): IntArray? {
        if (activeWidth <= 0 || activeHeight <= 0 || ratio.isNaN() || ratio <= 0.0) {
            return null
        }
        val effective = ratio.coerceAtLeast(1.0)
        val width = (activeWidth / effective).roundToInt().coerceIn(1, activeWidth)
        val height = (activeHeight / effective).roundToInt().coerceIn(1, activeHeight)
        val left = activeLeft + (activeWidth - width) / 2
        val top = activeTop + (activeHeight - height) / 2
        return intArrayOf(left, top, left + width, top + height)
    }

    /** Zoom ratio implied by a crop region width, or `null` for an invalid crop. */
    fun ratioFromCrop(activeWidth: Int, cropWidth: Int): Double? {
        if (activeWidth <= 0 || cropWidth <= 0 || cropWidth > activeWidth) {
            return null
        }
        return activeWidth.toDouble() / cropWidth
    }
}
