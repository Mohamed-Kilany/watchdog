package app.watchdog.remote.camera

/**
 * M4-T08 face-box math shared with the host overlay: sensor active-array
 * pixels to normalized `[x, y, w, h]`. Kept free of Android types so it is
 * covered by JVM unit tests on every CI platform.
 */
internal object FaceMath {
    private const val UNIT_MAX = 1.0
    private const val BOX_SIZE = 4
    private const val LEFT = 0
    private const val TOP = 1
    private const val WIDTH = 2
    private const val HEIGHT = 3

    /**
     * Normalize a box, clamping it to the active array; `null` for an invalid
     * array. [box] is `[left, top, right, bottom]` and [active] is
     * `[left, top, width, height]`, both in sensor pixels.
     */
    fun normalize(box: IntArray, active: IntArray): DoubleArray? {
        if (box.size < BOX_SIZE || active.size < BOX_SIZE) {
            return null
        }
        val activeWidth = active[WIDTH]
        val activeHeight = active[HEIGHT]
        if (activeWidth <= 0 || activeHeight <= 0) {
            return null
        }
        val x = ((box[LEFT] - active[LEFT]).toDouble() / activeWidth).coerceIn(0.0, UNIT_MAX)
        val y = ((box[TOP] - active[TOP]).toDouble() / activeHeight).coerceIn(0.0, UNIT_MAX)
        val width = ((box[WIDTH] - box[LEFT]).toDouble() / activeWidth).coerceIn(0.0, UNIT_MAX - x)
        val height = ((box[HEIGHT] - box[TOP]).toDouble() / activeHeight).coerceIn(0.0, UNIT_MAX - y)
        return doubleArrayOf(
            ControlValues.roundToMillis(x),
            ControlValues.roundToMillis(y),
            ControlValues.roundToMillis(width),
            ControlValues.roundToMillis(height),
        )
    }
}
