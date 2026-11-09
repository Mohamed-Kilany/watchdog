package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * M4-T02/T03 state model: validation against the capability descriptor,
 * delta generation, reset semantics and deterministic last-writer-wins.
 */
class ControlStateTest {
    private fun fpsRanges(): JSONArray =
        JSONArray()
            .put(JSONArray().put(15).put(30))
            .put(JSONArray().put(30).put(30))

    private fun rect(x: Double, y: Double, w: Double, h: Double): JSONObject =
        JSONObject()
            .put("x", x)
            .put("y", y)
            .put("w", w)
            .put("h", h)

    private fun descriptor(): JSONObject =
        JSONObject()
            .put(
                "iso",
                JSONObject()
                    .put("supported", true)
                    .put("min", 20)
                    .put("max", 4000)
                    .put("step", 1),
            ).put(
                "exposure_time_ns",
                JSONObject()
                    .put("supported", true)
                    .put("min", 61_000L)
                    .put("max", 125_000_000L)
                    .put("step", 1),
            ).put(
                "ae_mode",
                JSONObject().put("supported", true).put("modes", JSONArray().put(0).put(1).put(3)),
            ).put("ae_lock", JSONObject().put("supported", true))
            .put(
                "zoom_ratio",
                JSONObject()
                    .put("supported", true)
                    .put("min", 1.0)
                    .put("max", 8.0)
                    .put("step", 0),
            ).put("torch", JSONObject().put("supported", false))
            .put(
                "ae_fps_range",
                JSONObject()
                    .put("supported", true)
                    .put("ranges", fpsRanges()),
            ).put("ae_regions", JSONObject().put("supported", true).put("max_regions", 2))
            .put("ae_precapture", JSONObject().put("supported", true))
            .put(
                "af_mode",
                JSONObject().put("supported", true).put("modes", JSONArray().put(0).put(1).put(3)),
            ).put("af_lock", JSONObject().put("supported", true))
            .put("af_regions", JSONObject().put("supported", true).put("max_regions", 1))
            .put("af_trigger", JSONObject().put("supported", true))
            .put(
                "focus_distance",
                JSONObject()
                    .put("supported", true)
                    .put("min", 0.0)
                    .put("max", 10.0)
                    .put("step", 0),
            ).put(
                "awb_mode",
                JSONObject().put("supported", true).put("modes", JSONArray().put(1).put(5)),
            )

    private fun fullState(store: ControlStateStore, seq: Long = 1): JSONObject =
        store.fullState(seq, seq, "0", droppedFrames = 0, storageFreeMb = null).getJSONObject("state")

    private fun deltaState(store: ControlStateStore, seq: Long): JSONObject? =
        store.deltaState(seq, seq, "0", droppedFrames = 0, storageFreeMb = null)?.getJSONObject("state")

    @Test
    fun rejectsUnknownAndUnsupportedControls() {
        val store = ControlStateStore(descriptor())

        val unknown = store.setRequested("nope", 1, ControlSource.HOST)
        assertFalse(unknown.ok)
        assertEquals("UNKNOWN_CONTROL", unknown.errorCode)

        val unsupported = store.setRequested("torch", true, ControlSource.HOST)
        assertFalse(unsupported.ok)
        assertEquals("UNSUPPORTED", unsupported.errorCode)
    }

    @Test
    fun validatesRangesModesAndTypes() {
        val store = ControlStateStore(descriptor())

        assertEquals("OUT_OF_RANGE", store.setRequested("iso", 5000, ControlSource.HOST).errorCode)
        assertEquals("OUT_OF_RANGE", store.setRequested("iso", 19, ControlSource.HOST).errorCode)
        assertEquals("STEP_MISMATCH", store.setRequested("iso", 400.5, ControlSource.HOST).errorCode)
        assertEquals("OUT_OF_RANGE", store.setRequested("ae_mode", 2, ControlSource.HOST).errorCode)
        assertEquals("INVALID_VALUE", store.setRequested("ae_mode", 1.5, ControlSource.HOST).errorCode)
        assertEquals("INVALID_VALUE", store.setRequested("iso", "bright", ControlSource.HOST).errorCode)
        assertEquals("INVALID_VALUE", store.setRequested("ae_lock", "yes", ControlSource.HOST).errorCode)
        assertEquals("NULL_VALUE", store.setRequested("iso", null, ControlSource.HOST).errorCode)
    }

    @Test
    fun normalizesAcceptedValuesAndTracksTheWriter() {
        val store = ControlStateStore(descriptor())

        val iso = store.setRequested("iso", 400, ControlSource.HOST)
        assertTrue(iso.ok)
        assertEquals(400L, iso.applied)
        assertEquals(2.5, store.setRequested("zoom_ratio", 2.5, ControlSource.HOST).applied as Double, 0.0)
        assertTrue(store.setRequested("ae_lock", true, ControlSource.HOST).ok)

        val controls = fullState(store)
        assertEquals(400L, controls.getJSONObject("iso").getLong("requested"))
        assertEquals("host", controls.getJSONObject("iso").getString("source"))
        assertTrue(controls.getJSONObject("ae_lock").getBoolean("locked"))
        assertEquals(2.5, controls.getJSONObject("zoom_ratio").getDouble("requested"), 0.0)
    }

    @Test
    fun requestedValuesOnlyContainAssignments() {
        val store = ControlStateStore(descriptor())
        store.setRequested("iso", 400, ControlSource.HOST)
        store.setRequested("ae_mode", 1, ControlSource.HOST)

        val values = store.requestedValues()
        assertEquals(2, values.size)
        assertEquals(400L, values["iso"])
        assertEquals(1, values["ae_mode"])
    }

    @Test
    fun deltaCarriesOnlyChangedEntries() {
        val store = ControlStateStore(descriptor())
        store.setActual("iso", 100)
        fullState(store)

        assertNull(deltaState(store, 2))

        store.setActual("iso", 200)
        val delta = deltaState(store, 3)
        assertNotNull(delta)
        val state = delta!!
        assertEquals(1, state.length())
        assertEquals(200L, state.getJSONObject("iso").getLong("actual"))
        assertFalse(state.has("ae_mode"))
    }

    @Test
    fun resetBaselineMakesTheNextDeltaAFullSnapshot() {
        val store = ControlStateStore(descriptor())
        fullState(store)
        assertNull(deltaState(store, 2))

        store.resetBaseline()
        val state = deltaState(store, 3)!!
        assertTrue(state.has("iso"))
        assertTrue(state.has("battery_percent"))
    }

    @Test
    fun twoWritersResolveByArrivalOrder() {
        val store = ControlStateStore(descriptor())

        store.setRequested("iso", 100, ControlSource.HOST)
        store.setRequested("iso", 200, ControlSource.DEVICE)
        var iso = fullState(store).getJSONObject("iso")
        assertEquals(200L, iso.getLong("requested"))
        assertEquals("device", iso.getString("source"))

        store.setRequested("iso", 300, ControlSource.DEVICE)
        store.setRequested("iso", 400, ControlSource.HOST)
        iso = fullState(store, 2).getJSONObject("iso")
        assertEquals(400L, iso.getLong("requested"))
        assertEquals("host", iso.getString("source"))
    }

    @Test
    fun readoutsAreDeviceSourcedAndNeverLocked() {
        val store = ControlStateStore(descriptor())
        store.setReadout("fps_actual", 29.97)

        val fps = fullState(store).getJSONObject("fps_actual")
        assertEquals(29.97, fps.getDouble("actual"), 0.001)
        assertEquals("device", fps.getString("source"))
        assertTrue(fps.getBoolean("supported"))
        assertFalse(fps.has("locked"))
    }

    @Test
    fun validatesFpsRangeAgainstTheDescriptor() {
        val store = ControlStateStore(descriptor())

        val ok = store.setRequested("ae_fps_range", JSONArray().put(30).put(30), ControlSource.HOST)
        assertTrue(ok.ok)
        val applied = ok.applied as JSONArray
        assertEquals(30, applied.getInt(0))
        assertEquals(30, applied.getInt(1))

        val unlisted =
            store.setRequested("ae_fps_range", JSONArray().put(60).put(60), ControlSource.HOST)
        assertEquals("OUT_OF_RANGE", unlisted.errorCode)
        val malformed =
            store.setRequested("ae_fps_range", JSONArray().put(30), ControlSource.HOST)
        assertEquals("INVALID_VALUE", malformed.errorCode)
    }

    @Test
    fun validatesAeRegionsAsNormalizedRectangles() {
        val store = ControlStateStore(descriptor())
        val region = rect(0.25, 0.25, 0.5, 0.5)

        val ok = store.setRequested("ae_regions", JSONArray().put(region), ControlSource.HOST)
        assertTrue(ok.ok)
        val applied = ok.applied as JSONArray
        assertEquals(1000, applied.getJSONObject(0).getInt("weight"))

        val outside = rect(0.8, 0.0, 0.5, 0.5)
        val overflow = store.setRequested("ae_regions", JSONArray().put(outside), ControlSource.HOST)
        assertEquals("OUT_OF_RANGE", overflow.errorCode)
        assertEquals(
            "OUT_OF_RANGE",
            store.setRequested("ae_regions", JSONArray(), ControlSource.HOST).errorCode,
        )
        val tooMany = JSONArray().put(region).put(region).put(region)
        assertEquals(
            "OUT_OF_RANGE",
            store.setRequested("ae_regions", tooMany, ControlSource.HOST).errorCode,
        )
        assertEquals(
            "INVALID_VALUE",
            store.setRequested("ae_regions", JSONObject(), ControlSource.HOST).errorCode,
        )
    }

    @Test
    fun triggersAreValidatedWithoutBecomingRequestedState() {
        val store = ControlStateStore(descriptor())

        val start = store.validateControl("ae_precapture", "start")
        assertTrue(start.ok)
        assertEquals("start", start.applied)
        assertFalse(fullState(store).getJSONObject("ae_precapture").has("requested"))

        assertEquals("INVALID_VALUE", store.validateControl("ae_precapture", "fire").errorCode)
        assertEquals("INVALID_VALUE", store.validateControl("ae_precapture", 1).errorCode)
    }

    @Test
    fun lockFlagFollowsTheRequestedBoolean() {
        val store = ControlStateStore(descriptor())
        store.setRequested("ae_lock", true, ControlSource.HOST)
        var lock = fullState(store).getJSONObject("ae_lock")
        assertTrue(lock.getBoolean("locked"))

        store.setRequested("ae_lock", false, ControlSource.HOST)
        lock = fullState(store, 2).getJSONObject("ae_lock")
        assertFalse(lock.getBoolean("locked"))
    }

    @Test
    fun validatesFocusDistanceAgainstTheLensRange() {
        val store = ControlStateStore(descriptor())

        val ok = store.setRequested("focus_distance", 0.5, ControlSource.HOST)
        assertTrue(ok.ok)
        assertEquals(0.5, ok.applied as Double, 0.0)
        assertEquals(
            "OUT_OF_RANGE",
            store.setRequested("focus_distance", 10.5, ControlSource.HOST).errorCode,
        )
        assertEquals(
            "OUT_OF_RANGE",
            store.setRequested("focus_distance", -0.1, ControlSource.HOST).errorCode,
        )
        assertEquals(
            "INVALID_VALUE",
            store.setRequested("focus_distance", "near", ControlSource.HOST).errorCode,
        )
    }

    @Test
    fun validatesAfRegionsLikeAeRegions() {
        val store = ControlStateStore(descriptor())
        val region = rect(0.25, 0.25, 0.5, 0.5)

        val ok = store.setRequested("af_regions", JSONArray().put(region), ControlSource.HOST)
        assertTrue(ok.ok)
        assertEquals(1000, (ok.applied as JSONArray).getJSONObject(0).getInt("weight"))

        val tooMany = JSONArray().put(region).put(region)
        assertEquals(
            "OUT_OF_RANGE",
            store.setRequested("af_regions", tooMany, ControlSource.HOST).errorCode,
        )
    }

    @Test
    fun afTriggerAndLockAreValidated() {
        val store = ControlStateStore(descriptor())

        val start = store.validateControl("af_trigger", "start")
        assertTrue(start.ok)
        assertEquals("start", start.applied)
        assertEquals("INVALID_VALUE", store.validateControl("af_trigger", "hold").errorCode)
        assertFalse(fullState(store).getJSONObject("af_trigger").has("requested"))

        assertTrue(store.setRequested("af_lock", true, ControlSource.HOST).ok)
        assertTrue(fullState(store).getJSONObject("af_lock").getBoolean("requested"))
    }
}
