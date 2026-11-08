package app.watchdog.remote.protocol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class JsonMessagesTest {
    @Test
    fun eventCarriesKindTimestampAndData() {
        val payload =
            JsonMessages.encodeEvent(
                kind = "camera_disconnected",
                timestampMs = 1_730_900_001_000,
                data = JSONObject().put("camera_id", "0"),
            )

        val json = JSONObject(String(payload, Charsets.UTF_8))
        assertEquals("camera_disconnected", json.getString("kind"))
        assertEquals(1_730_900_001_000, json.getLong("timestamp_ms"))
        assertEquals("0", json.getJSONObject("data").getString("camera_id"))
    }

    @Test
    fun eventWithoutDataOmitsTheField() {
        val payload = JsonMessages.encodeEvent("camera_reopened", timestampMs = 5)

        val json = JSONObject(String(payload, Charsets.UTF_8))
        assertEquals("camera_reopened", json.getString("kind"))
        assertEquals(false, json.has("data"))
    }
}
