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
    private fun descriptor(): JSONObject =
        JSONObject()
            .put(
                "iso",
                JSONObject().put("supported", true).put("min", 20).put("max", 4000).put("step", 1),
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
                JSONObject().put("supported", true).put("min", 1.0).put("max", 8.0).put("step", 0),
            ).put("torch", JSONObject().put("supported", false))
            .put(
                "awb_mode",
                JSONObject().put("supported", true).put("modes", JSONArray().put(1).put(5)),
            )

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

        val state = store.fullState(1, 1_000, "0", droppedFrames = 0, storageFreeMb = null)
        val controls = state.getJSONObject("state")
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
        store.fullState(1, 1, "0", droppedFrames = 0, storageFreeMb = null)

        assertNull(store.deltaState(2, 2, "0", droppedFrames = 0, storageFreeMb = null))

        store.setActual("iso", 200)
        val delta = store.deltaState(3, 3, "0", droppedFrames = 0, storageFreeMb = null)
        assertNotNull(delta)
        val state = delta!!.getJSONObject("state")
        assertEquals(1, state.length())
        assertEquals(200L, state.getJSONObject("iso").getLong("actual"))
        assertFalse(state.has("ae_mode"))
    }

    @Test
    fun resetBaselineMakesTheNextDeltaAFullSnapshot() {
        val store = ControlStateStore(descriptor())
        store.fullState(1, 1, "0", droppedFrames = 0, storageFreeMb = null)
        assertNull(store.deltaState(2, 2, "0", droppedFrames = 0, storageFreeMb = null))

        store.resetBaseline()
        val state =
            store.deltaState(3, 3, "0", droppedFrames = 0, storageFreeMb = null)!!
                .getJSONObject("state")
        assertTrue(state.has("iso"))
        assertTrue(state.has("battery_percent"))
    }

    @Test
    fun twoWritersResolveByArrivalOrder() {
        val store = ControlStateStore(descriptor())

        store.setRequested("iso", 100, ControlSource.HOST)
        store.setRequested("iso", 200, ControlSource.DEVICE)
        var iso =
            store.fullState(1, 1, "0", droppedFrames = 0, storageFreeMb = null)
                .getJSONObject("state")
                .getJSONObject("iso")
        assertEquals(200L, iso.getLong("requested"))
        assertEquals("device", iso.getString("source"))

        store.setRequested("iso", 300, ControlSource.DEVICE)
        store.setRequested("iso", 400, ControlSource.HOST)
        iso =
            store.fullState(2, 2, "0", droppedFrames = 0, storageFreeMb = null)
                .getJSONObject("state")
                .getJSONObject("iso")
        assertEquals(400L, iso.getLong("requested"))
        assertEquals("host", iso.getString("source"))
    }

    @Test
    fun readoutsAreDeviceSourcedAndNeverLocked() {
        val store = ControlStateStore(descriptor())
        store.setReadout("fps_actual", 29.97)

        val fps =
            store.fullState(1, 1, "0", droppedFrames = 0, storageFreeMb = null)
                .getJSONObject("state")
                .getJSONObject("fps_actual")
        assertEquals(29.97, fps.getDouble("actual"), 0.001)
        assertEquals("device", fps.getString("source"))
        assertTrue(fps.getBoolean("supported"))
        assertFalse(fps.has("locked"))
    }

    @Test
    fun lockFlagFollowsTheRequestedBoolean() {
        val store = ControlStateStore(descriptor())
        store.setRequested("ae_lock", true, ControlSource.HOST)
        var lock =
            store.fullState(1, 1, "0", droppedFrames = 0, storageFreeMb = null)
                .getJSONObject("state")
                .getJSONObject("ae_lock")
        assertTrue(lock.getBoolean("locked"))

        store.setRequested("ae_lock", false, ControlSource.HOST)
        lock =
            store.fullState(2, 2, "0", droppedFrames = 0, storageFreeMb = null)
                .getJSONObject("state")
                .getJSONObject("ae_lock")
        assertFalse(lock.getBoolean("locked"))
    }
}
