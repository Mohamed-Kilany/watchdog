package app.watchdog.remote

import android.hardware.camera2.CaptureRequest
import app.watchdog.remote.camera.ControlStateStore
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** M4-T08: a tonemap parameter implies its matching mode in the same batch. */
class ControlImplicationsTest {
    private fun descriptor(): JSONObject =
        JSONObject()
            .put(
                "tonemap",
                JSONObject()
                    .put("supported", true)
                    .put("modes", JSONArray().put(1).put(4).put(5)),
            ).put("tonemap_curve", JSONObject().put("supported", true).put("max_points", 4))

    private fun curve(): JSONObject =
        JSONObject()
            .put("red", JSONArray().put(0.0).put(0.0).put(1.0).put(1.0))
            .put("green", JSONArray().put(0.0).put(0.0).put(1.0).put(1.0))
            .put("blue", JSONArray().put(0.0).put(0.0).put(1.0).put(1.0))

    @Test
    fun contrastCurveImpliesTheMatchingMode() {
        val store = ControlStateStore(descriptor())
        val toApply = mutableMapOf<String, Any>("tonemap_curve" to curve())
        val results = mutableListOf<JSONObject>()

        ControlImplications().apply(store, toApply, results, mutableMapOf(), manualSensor = false)

        val mode = CaptureRequest.TONEMAP_MODE_CONTRAST_CURVE
        assertEquals(mode, toApply["tonemap"])
        assertEquals(mode, store.requestedValues()["tonemap"])
        assertTrue(results.any { it.optString("control") == "tonemap" && it.getBoolean("ok") })
    }

    @Test
    fun anExplicitModeWinsOverTheImpliedOne() {
        val store = ControlStateStore(descriptor())
        val toApply = mutableMapOf<String, Any>("tonemap_curve" to curve(), "tonemap" to 2)
        val results = mutableListOf<JSONObject>()

        ControlImplications().apply(store, toApply, results, mutableMapOf(), manualSensor = false)

        assertEquals(2, toApply["tonemap"])
        assertEquals(2, store.requestedValues()["tonemap"])
        assertTrue(results.isEmpty())
    }
}
