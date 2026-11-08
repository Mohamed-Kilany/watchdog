package app.watchdog.remote.protocol

import org.json.JSONException
import org.json.JSONObject

/**
 * Typed payload validation for every v1 message type. Framing alone is not
 * enough for the golden-vector contract: a well-formed frame with an invalid
 * typed payload must be rejected (`MessageDecode`).
 */
object MessageCodec {
    fun validate(packet: Packet) {
        when (packet.type) {
            MessageType.FRAME -> FramePayload.decode(packet.payload)
            MessageType.LOG_CHUNK -> LogChunkCodec.decode(packet.payload)
            MessageType.HOST_HELLO -> JsonMessages.parseHostHello(packet.payload)
            MessageType.PING -> JsonMessages.parsePing(packet.payload)
            MessageType.BYE -> JsonMessages.parseBye(packet.payload)
            else -> validateJsonObject(packet)
        }
    }

    private fun validateJsonObject(packet: Packet) {
        try {
            JSONObject(String(packet.payload, Charsets.UTF_8))
        } catch (e: JSONException) {
            throw ProtocolException(
                ProtocolError.MessageDecode,
                "invalid ${packet.type} JSON: ${e.message}",
                e,
            )
        }
    }
}
