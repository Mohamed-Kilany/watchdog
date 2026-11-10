package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/** M4-T09 structural entries in the canonical control state. */
class OutputStateTest {
    private fun descriptor(): JSONObject =
        JSONObject()
            .put(
                OutputConfigs.CAMERA_ID,
                JSONObject()
                    .put("supported", true)
                    .put("structural", true)
                    .put("values", JSONArray().put("0")),
            ).put(
                OutputConfigs.STREAM_SIZE,
                JSONObject()
                    .put("supported", true)
                    .put("structural", true)
                    .put("sizes", JSONArray()),
            ).put(
                "iso",
                JSONObject()
                    .put("supported", true)
                    .put("min", 50)
                    .put("max", 3200)
                    .put("step", 1),
            ).put("unsupported", JSONArray())

    private fun size(width: Int, height: Int): JSONObject = JSONObject().put("width", width).put("height", height)

    @Test
    fun structuralControlsCannotBeWrittenThroughTheGenericPath() {
        val store = ControlStateStore(descriptor())
        val validation = store.validateControl(OutputConfigs.STREAM_SIZE, size(1280, 720))
        assertFalse(validation.ok)
        assertEquals("STRUCTURAL_CONTROL", validation.errorCode)
    }

    @Test
    fun structuralStateIsRecordedButNotSentAsCamera2Controls() {
        val store = ControlStateStore(descriptor())
        store.setStructural(OutputConfigs.CAMERA_ID, "0", ControlSource.HOST)
        store.setStructural(OutputConfigs.STREAM_SIZE, size(1280, 720))

        val requested = store.requestedValues()
        assertFalse(requested.containsKey(OutputConfigs.CAMERA_ID))
        assertFalse(requested.containsKey(OutputConfigs.STREAM_SIZE))

        val state = store.fullState(1, 0, "0", 0, null).getJSONObject("state")
        val camera = state.getJSONObject(OutputConfigs.CAMERA_ID)
        assertEquals("0", camera.getString("requested"))
        assertEquals("host", camera.getString("source"))
        val stream = state.getJSONObject(OutputConfigs.STREAM_SIZE)
        assertEquals(1280, stream.getJSONObject("actual").getInt("width"))
        assertEquals(720, stream.getJSONObject("actual").getInt("height"))
    }

    @Test
    fun automaticStructuralValuesReportTheAutoSource() {
        val store = ControlStateStore(descriptor())
        store.setStructural(OutputConfigs.CAMERA_ID, "0", ControlSource.AUTO)
        val camera =
            store
                .fullState(2, 0, "0", 0, null)
                .getJSONObject("state")
                .getJSONObject(OutputConfigs.CAMERA_ID)
        assertEquals("auto", camera.getString("source"))
    }
}
