package app.watchdog.remote.protocol

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** M4-T03 command pipeline wire format (`docs/protocol.md` §5.5–5.6). */
class CommandMessagesTest {
    @Test
    fun parsesCommandEnvelope() {
        val command =
            JsonMessages.parseCommand(
                """{"id":"c-1","command":"set_controls","params":{"controls":[{"id":"iso","value":400}]}}"""
                    .toByteArray(),
            )

        assertEquals("c-1", command.id)
        assertEquals("set_controls", command.command)
        val control = command.params.getJSONArray("controls").getJSONObject(0)
        assertEquals(400, control.getInt("value"))
    }

    @Test
    fun missingParamsBecomesAnEmptyObject() {
        val command =
            JsonMessages.parseCommand(
                """{"id":"c-2","command":"get_state"}""".toByteArray(),
            )

        assertEquals("get_state", command.command)
        assertEquals(0, command.params.length())
    }

    @Test
    fun preservesAssignmentOrderAndDuplicates() {
        val params =
            JSONObject(
                """{"controls":[{"id":"iso","value":100},{"id":"iso","value":200},{"id":"ae_lock","value":true}]}""",
            )

        val assignments = JsonMessages.parseControlAssignments(params)
        assertEquals(listOf("iso", "iso", "ae_lock"), assignments.map { it.id })
        assertEquals(100, assignments[0].value)
        assertEquals(200, assignments[1].value)
        assertEquals(true, assignments[2].value)
    }

    @Test(expected = ProtocolException::class)
    fun rejectsMissingControlsArray() {
        JsonMessages.parseControlAssignments(JSONObject())
    }

    @Test(expected = ProtocolException::class)
    fun rejectsAssignmentWithoutId() {
        JsonMessages.parseControlAssignments(
            JSONObject("""{"controls":[{"value":1}]}"""),
        )
    }

    @Test
    fun encodesPerControlResults() {
        val payload =
            JsonMessages.encodeCommandResult(
                "c-1",
                listOf(
                    JsonMessages.controlResult("iso", true, applied = 400),
                    JsonMessages.controlResult(
                        "awb_mode",
                        false,
                        errorCode = "OUT_OF_RANGE",
                        message = "mode 9 is not available",
                    ),
                ),
                timestampMs = 1_234,
            )

        val json = JSONObject(String(payload, Charsets.UTF_8))
        assertEquals("c-1", json.getString("id"))
        assertEquals(1_234L, json.getLong("timestamp_ms"))
        val results = json.getJSONArray("results")
        assertEquals(2, results.length())

        val first = results.getJSONObject(0)
        assertEquals("iso", first.getString("control"))
        assertTrue(first.getBoolean("ok"))
        assertEquals(400, first.getInt("applied"))
        assertFalse(first.has("error_code"))

        val second = results.getJSONObject(1)
        assertEquals("awb_mode", second.getString("control"))
        assertFalse(second.getBoolean("ok"))
        assertEquals("OUT_OF_RANGE", second.getString("error_code"))
        assertEquals("mode 9 is not available", second.getString("message"))
        assertFalse(second.has("applied"))
    }

    @Test
    fun encodesWholeCommandErrors() {
        val payload =
            JsonMessages.encodeCommandResult(
                "c-2",
                emptyList(),
                error = "camera is not open",
            )

        val json = JSONObject(String(payload, Charsets.UTF_8))
        assertEquals("c-2", json.getString("id"))
        assertEquals("camera is not open", json.getString("error"))
        assertEquals(0, json.getJSONArray("results").length())
    }
}
