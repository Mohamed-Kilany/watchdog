package app.watchdog.remote.camera

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * M4-T07 legacy zoom fallback: `SCALER_CROP_REGION` math used on cameras
 * without `CONTROL_ZOOM_RATIO_RANGE`.
 */
class ZoomMathTest {
    @Test
    fun ratioOneUsesTheWholeActiveArray() {
        assertEquals(CropRegion(0, 0, 4000, 3000), ZoomMath.cropRegion(0, 0, 4000, 3000, 1.0))
    }

    @Test
    fun ratioTwoCentresAHalfSizeCrop() {
        assertEquals(CropRegion(1000, 750, 3000, 2250), ZoomMath.cropRegion(0, 0, 4000, 3000, 2.0))
    }

    @Test
    fun oddDimensionsStayInsideTheActiveArray() {
        assertEquals(CropRegion(1000, 750, 3001, 2251), ZoomMath.cropRegion(0, 0, 4001, 3001, 2.0))
    }

    @Test
    fun offsetActiveArrayKeepsItsOrigin() {
        assertEquals(CropRegion(1008, 756, 3008, 2256), ZoomMath.cropRegion(8, 6, 4000, 3000, 2.0))
    }

    @Test
    fun extremeRatiosClampToOnePixel() {
        assertEquals(CropRegion(1999, 1499, 2000, 1500), ZoomMath.cropRegion(0, 0, 4000, 3000, 100_000.0))
    }

    @Test
    fun ratiosBelowOneDoNotEnlargeTheCrop() {
        assertEquals(CropRegion(0, 0, 4000, 3000), ZoomMath.cropRegion(0, 0, 4000, 3000, 0.5))
    }

    @Test
    fun invalidInputsReturnNull() {
        assertNull(ZoomMath.cropRegion(0, 0, 0, 3000, 2.0))
        assertNull(ZoomMath.cropRegion(0, 0, 4000, 0, 2.0))
        assertNull(ZoomMath.cropRegion(0, 0, 4000, 3000, 0.0))
        assertNull(ZoomMath.cropRegion(0, 0, 4000, 3000, -2.0))
        assertNull(ZoomMath.cropRegion(0, 0, 4000, 3000, Double.NaN))
        assertNull(ZoomMath.cropRegion(0, 0, 4000, 3000, Double.POSITIVE_INFINITY))
    }

    @Test
    fun ratioFromCropInvertsTheMath() {
        assertEquals(2.0, ZoomMath.ratioFromCrop(4000, 2000)!!, 0.0)
        assertEquals(1.0, ZoomMath.ratioFromCrop(4000, 4000)!!, 0.0)
        assertNull(ZoomMath.ratioFromCrop(4000, 0))
        assertNull(ZoomMath.ratioFromCrop(4000, 5000))
        assertNull(ZoomMath.ratioFromCrop(0, 100))
    }
}
