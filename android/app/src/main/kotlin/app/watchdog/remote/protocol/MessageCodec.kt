package app.watchdog.remote.protocol

import org.json.JSONException
import org.json.JSONObject

/**
 * Typed payload validation for every v1 message type. Framing alone is not
 * enough for the golden-vector contract: a well-formed frame with an invalid
 * typed payload must be rejected (`MessageDecode`).
 */
object MessageCodec {
    private const val LOG_CHUNK_HEADER_SIZE = 9
    private const val LOG_LEVEL_INDEX = 8
    private const val MAX_LOG_LEVEL = 4
    private const val BYTE_MASK = 0xFF

    fun validate(packet: Packet) {
        when (packet.type) {
            MessageType.FRAME -> FramePayload.decode(packet.payload)
            MessageType.LOG_CHUNK -> validateLogChunk(packet.payload)
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

    private fun validateLogChunk(payload: ByteArray) {
        if (payload.size < LOG_CHUNK_HEADER_SIZE) {
            throw ProtocolException(
                ProtocolError.MessageDecode,
                "truncated LOG_CHUNK: ${payload.size} bytes",
            )
        }
        val level = payload[LOG_LEVEL_INDEX].toInt() and BYTE_MASK
        if (level > MAX_LOG_LEVEL) {
            throw ProtocolException(ProtocolError.MessageDecode, "invalid LOG_CHUNK level $level")
        }
    }
}
