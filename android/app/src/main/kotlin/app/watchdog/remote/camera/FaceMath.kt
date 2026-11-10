package app.watchdog.remote.camera

/**
 * M4-T08 face-box math shared with the host overlay: sensor active-array
 * pixels to normalized `[x, y, w, h]`. Kept free of Android types so it is
 * covered by JVM unit tests on every CI platform.
 */
internal object FaceMath {
    private const val UNIT_MAX = 1.0

    /** Normalize a box, clamping it to the active array; `null` for an invalid array. */
    fun normalize(
        left: Int,
        top: Int,
        right: Int,
        bottom: Int,
        activeLeft: Int,
        activeTop: Int,
        activeWidth: Int,
        activeHeight: Int,
    ): DoubleArray? {
        if (activeWidth <= 0 || activeHeight <= 0) {
            return null
        }
        val x = ((left - activeLeft).toDouble() / activeWidth).coerceIn(0.0, UNIT_MAX)
        val y = ((top - activeTop).toDouble() / activeHeight).coerceIn(0.0, UNIT_MAX)
        val width = ((right - left).toDouble() / activeWidth).coerceIn(0.0, UNIT_MAX - x)
        val height = ((bottom - top).toDouble() / activeHeight).coerceIn(0.0, UNIT_MAX - y)
        return doubleArrayOf(
            ControlValues.roundToMillis(x),
            ControlValues.roundToMillis(y),
            ControlValues.roundToMillis(width),
            ControlValues.roundToMillis(height),
        )
    }
}
