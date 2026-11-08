package app.watchdog.remote.protocol

import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val U16_MASK = 0xFFFF
private const val BYTE_MASK = 0xFF

/**
 * Binary `FRAME` payload (`docs/protocol.md` §5.3): a fixed 28-byte header
 * followed by exactly one complete JPEG.
 */
data class FramePayload(
    val sequence: Long,
    val sensorTimestampNs: Long,
    val width: Int,
    val height: Int,
    val rotation: Int,
    val flags: Int,
    val jpeg: ByteArray,
) {
    fun encode(): ByteArray {
        val out = ByteBuffer.allocate(HEADER_SIZE + jpeg.size).order(ByteOrder.LITTLE_ENDIAN)
        out.putLong(sequence)
        out.putLong(sensorTimestampNs)
        out.putInt(width)
        out.putInt(height)
        out.putShort(rotation.toShort())
        out.put(flags.toByte())
        out.put(0)
        out.put(jpeg)
        return out.array()
    }

    companion object {
        const val HEADER_SIZE = 28
        const val FLAG_COLOR_TRANSFORM = 0x01
        const val FLAG_SNAPSHOT = 0x02

        private val VALID_ROTATIONS = setOf(0, 90, 180, 270)
        fun decode(payload: ByteArray): FramePayload {
            if (payload.size < HEADER_SIZE) {
                throw ProtocolException(
                    ProtocolError.MessageDecode,
                    "truncated FRAME header: ${payload.size} bytes",
                )
            }
            val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
            val sequence = buffer.long
            val sensorTimestampNs = buffer.long
            val width = buffer.int
            val height = buffer.int
            val rotation = buffer.short.toInt() and U16_MASK
            val flags = buffer.get().toInt() and BYTE_MASK
            val reserved = buffer.get().toInt() and BYTE_MASK
            if (reserved != 0) {
                throw ProtocolException(ProtocolError.MessageDecode, "FRAME reserved byte must be 0")
            }
            if (rotation !in VALID_ROTATIONS) {
                throw ProtocolException(ProtocolError.MessageDecode, "invalid rotation $rotation")
            }
            return FramePayload(
                sequence = sequence,
                sensorTimestampNs = sensorTimestampNs,
                width = width,
                height = height,
                rotation = rotation,
                flags = flags,
                jpeg = payload.copyOfRange(HEADER_SIZE, payload.size),
            )
        }
    }
}
