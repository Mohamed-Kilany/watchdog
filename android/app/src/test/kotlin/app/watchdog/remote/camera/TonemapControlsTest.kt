package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** M4-T08 contrast-curve validation. */
class TonemapControlsTest {
    private fun spec(maxPoints: Int = 4): JSONObject = JSONObject().put("supported", true).put("max_points", maxPoints)

    private fun channel(vararg points: Double): JSONArray {
        val out = JSONArray()
        points.forEach { out.put(it) }
        return out
    }

    private fun curve(red: JSONArray, green: JSONArray = red, blue: JSONArray = red): JSONObject =
        JSONObject().put("red", red).put("green", green).put("blue", blue)

    @Test
    fun acceptsAndCanonicalizesAValidCurve() {
        val value =
            curve(
                channel(0.0, 0.0, 0.5, 0.6, 1.0, 1.0),
                channel(0.0, 0.0, 1.0, 1.0),
                channel(0.0, 0.1, 1.0, 0.9),
            )
        val result = TonemapControls.validateCurve(spec(), value)
        assertTrue(result.ok)
        val applied = result.applied as JSONObject
        assertEquals(6, applied.getJSONArray("red").length())
        assertEquals(4, applied.getJSONArray("green").length())
        assertEquals(0.1, applied.getJSONArray("blue").getDouble(1), 0.0)
    }

    @Test
    fun rejectsMissingChannels() {
        val value = JSONObject().put("red", channel(0.0, 0.0, 1.0, 1.0))
        val result = TonemapControls.validateCurve(spec(), value)
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }

    @Test
    fun rejectsNonIncreasingInputs() {
        val result =
            TonemapControls.validateCurve(
                spec(),
                curve(channel(0.0, 0.0, 0.5, 0.5, 0.5, 1.0)),
            )
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }

    @Test
    fun rejectsValuesOutsideTheUnitRange() {
        val result = TonemapControls.validateCurve(spec(), curve(channel(0.0, 0.0, 1.0, 1.2)))
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }

    @Test
    fun rejectsTooManyPoints() {
        val result =
            TonemapControls.validateCurve(
                spec(maxPoints = 2),
                curve(channel(0.0, 0.0, 0.5, 0.5, 1.0, 1.0)),
            )
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }

    @Test
    fun rejectsOddPointArrays() {
        val result = TonemapControls.validateCurve(spec(), curve(channel(0.0, 0.0, 1.0)))
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }

    @Test
    fun rejectsNonObjectValues() {
        val result = TonemapControls.validateCurve(spec(), "linear")
        assertFalse(result.ok)
        assertEquals("INVALID_VALUE", result.errorCode)
    }
}
