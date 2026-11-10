package app.watchdog.remote.camera

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** M4-T08 face-box normalization shared with the host overlay. */
class FaceMathTest {
    @Test
    fun normalizesAFullFrameBox() {
        val box = FaceMath.normalize(intArrayOf(0, 0, 4000, 3000), intArrayOf(0, 0, 4000, 3000))
        assertArrayEquals(doubleArrayOf(0.0, 0.0, 1.0, 1.0), box!!, 0.0)
    }

    @Test
    fun normalizesACentredBox() {
        val box = FaceMath.normalize(intArrayOf(1000, 750, 3000, 2250), intArrayOf(0, 0, 4000, 3000))
        assertArrayEquals(doubleArrayOf(0.25, 0.25, 0.5, 0.5), box!!, 0.0)
    }

    @Test
    fun keepsTheActiveArrayOrigin() {
        val box = FaceMath.normalize(intArrayOf(1008, 756, 3008, 2256), intArrayOf(8, 6, 4000, 3000))
        assertArrayEquals(doubleArrayOf(0.25, 0.25, 0.5, 0.5), box!!, 0.0)
    }

    @Test
    fun clampsBoxesToTheActiveArray() {
        val box = FaceMath.normalize(intArrayOf(-100, -100, 5000, 4000), intArrayOf(0, 0, 4000, 3000))
        assertArrayEquals(doubleArrayOf(0.0, 0.0, 1.0, 1.0), box!!, 0.0)
    }

    @Test
    fun keepsZeroSizeBoxes() {
        val box = FaceMath.normalize(intArrayOf(1000, 750, 1000, 750), intArrayOf(0, 0, 4000, 3000))
        assertArrayEquals(doubleArrayOf(0.25, 0.25, 0.0, 0.0), box!!, 0.0)
    }

    @Test
    fun invalidActiveArrayReturnsNull() {
        assertNull(FaceMath.normalize(intArrayOf(0, 0, 10, 10), intArrayOf(0, 0, 0, 3000)))
        assertNull(FaceMath.normalize(intArrayOf(0, 0, 10, 10), intArrayOf(0, 0, 4000, 0)))
        assertNull(FaceMath.normalize(intArrayOf(0, 0, 10), intArrayOf(0, 0, 4000, 3000)))
    }
}
