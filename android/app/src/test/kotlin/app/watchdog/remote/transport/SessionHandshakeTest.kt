package app.watchdog.remote.transport

import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionHandshakeTest {
    private val token = "00112233445566778899aabbccddeeff"

    @Test
    fun `accepts a valid hello`() {
        val decision = SessionHandshake.evaluate(hello(), token)
        assertEquals(HandshakeDecision.Accepted(1), decision)
    }

    @Test
    fun `rejects non-hello first frame`() {
        val ping = Packet(MessageType.PING, 0, """{"ts_ms":1,"nonce":"x"}""".toByteArray())
        val decision = SessionHandshake.evaluate(ping, token)
        assertTrue(decision is HandshakeDecision.Rejected)
        assertEquals("PROTOCOL_ERROR", (decision as HandshakeDecision.Rejected).errorCode)
    }

    @Test
    fun `rejects token mismatch`() {
        val decision = SessionHandshake.evaluate(hello(token = "ff".repeat(16)), token)
        assertTrue(decision is HandshakeDecision.Rejected)
        assertEquals("UNAUTHORIZED", (decision as HandshakeDecision.Rejected).errorCode)
    }

    @Test
    fun `rejects missing token`() {
        val decision = SessionHandshake.evaluate(hello(token = null), token)
        assertTrue(decision is HandshakeDecision.Rejected)
        assertEquals("UNAUTHORIZED", (decision as HandshakeDecision.Rejected).errorCode)
    }

    @Test
    fun `rejects incompatible protocol version with actionable hint`() {
        val decision = SessionHandshake.evaluate(hello(min = 2, max = 3), token)
        assertTrue(decision is HandshakeDecision.Rejected)
        val rejected = decision as HandshakeDecision.Rejected
        assertEquals("PROTOCOL_VERSION_MISMATCH", rejected.errorCode)
        assertEquals("upgrade watchdog on the host", rejected.hint)
    }

    @Test
    fun `rejects malformed hello`() {
        val packet = Packet(MessageType.HOST_HELLO, 0, """{"protocol_min":"x"}""".toByteArray())
        val decision = SessionHandshake.evaluate(packet, token)
        assertTrue(decision is HandshakeDecision.Rejected)
        assertEquals("INVALID_REQUEST", (decision as HandshakeDecision.Rejected).errorCode)
    }

    private fun hello(min: Int = 1, max: Int = 1, token: String? = this.token): Packet {
        val json =
            JSONObject()
                .put("protocol_min", min)
                .put("protocol_max", max)
                .put("host", JSONObject().put("name", "watchdog").put("version", "0.1.0"))
        if (token != null) {
            json.put("session_token", token)
        }
        return Packet(MessageType.HOST_HELLO, 0, json.toString().toByteArray())
    }
}
