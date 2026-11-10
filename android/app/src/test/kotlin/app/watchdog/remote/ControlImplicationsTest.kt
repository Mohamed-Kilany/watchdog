package app.watchdog.remote

import app.watchdog.remote.camera.ControlSource
import app.watchdog.remote.camera.ControlStateStore
import app.watchdog.remote.camera.TonemapControls
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
                    .put(
                        "modes",
                        JSONArray()
                            .put(TonemapControls.MODE_CONTRAST_CURVE)
                            .put(TonemapControls.MODE_GAMMA_VALUE)
                            .put(TonemapControls.MODE_PRESET_CURVE),
                    ),
            ).put("tonemap_curve", JSONObject().put("supported", true).put("max_points", 4))

    private fun points(vararg values: Double): JSONArray {
        val out = JSONArray()
        values.forEach { out.put(it) }
        return out
    }

    private fun curve(): JSONObject =
        JSONObject()
            .put("red", points(0.0, 0.0, 1.0, 1.0))
            .put("green", points(0.0, 0.0, 1.0, 1.0))
            .put("blue", points(0.0, 0.0, 1.0, 1.0))

    @Test
    fun contrastCurveImpliesTheMatchingMode() {
        val store = ControlStateStore(descriptor())
        val toApply = mutableMapOf<String, Any>("tonemap_curve" to curve())
        val results = mutableListOf<JSONObject>()

        ControlImplications().apply(store, toApply, results, mutableMapOf(), manualSensor = false)

        val mode = TonemapControls.MODE_CONTRAST_CURVE
        assertEquals(mode, toApply["tonemap"])
        assertEquals(mode, store.requestedValues()["tonemap"])
        assertTrue(results.any { it.optString("control") == "tonemap" && it.getBoolean("ok") })
    }

    @Test
    fun anExplicitModeWinsOverTheImpliedOne() {
        val store = ControlStateStore(descriptor())
        val explicit = TonemapControls.MODE_GAMMA_VALUE
        assertTrue(store.setRequested("tonemap", explicit, ControlSource.HOST).ok)
        val toApply = mutableMapOf<String, Any>("tonemap_curve" to curve(), "tonemap" to explicit)
        val results = mutableListOf<JSONObject>()

        ControlImplications().apply(store, toApply, results, mutableMapOf(), manualSensor = false)

        assertEquals(explicit, toApply["tonemap"])
        assertEquals(explicit, store.requestedValues()["tonemap"])
        assertTrue(results.isEmpty())
    }
}
