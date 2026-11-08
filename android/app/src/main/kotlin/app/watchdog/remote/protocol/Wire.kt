package app.watchdog.remote.protocol

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.zip.CRC32

/** Protocol version implemented by this build (`docs/protocol.md` §3). */
const val PROTOCOL_VERSION = 1

/** Size of the fixed frame header in bytes. */
const val HEADER_LEN = 16

/** Maximum accepted `payload_len` (64 MiB). */
const val MAX_PAYLOAD_LEN = 64 * 1024 * 1024

private val MAGIC = "WDG1".toByteArray(Charsets.US_ASCII)
private const val BYTE_MASK = 0xFF

/** Message type byte values (spec §4). */
enum class MessageType(val wire: Int) {
    HOST_HELLO(0x01),
    HELLO_ACK(0x02),
    FRAME(0x10),
    STATE(0x20),
    COMMAND(0x21),
    COMMAND_RESULT(0x22),
    EVENT(0x30),
    MEDIA_MANIFEST(0x40),
    LOG_CHUNK(0x50),
    ERROR(0x60),
    PING(0x70),
    PONG(0x71),
    BYE(0x7f),
    ;

    companion object {
        fun fromWire(value: Int): MessageType? = entries.firstOrNull { it.wire == value }
    }
}

/** Stable failure taxonomy; mirrors the Rust `DecodeError` variants. */
enum class ProtocolError {
    BadMagic,
    UnsupportedVersion,
    ReservedNonZero,
    PayloadTooLarge,
    UnknownType,
    CrcMismatch,
    MessageDecode,
}

/** Structural protocol violation: the connection must be dropped. */
class ProtocolException(val error: ProtocolError, message: String, cause: Throwable? = null) : Exception(message, cause)

/** One decoded frame: type, per-message flags and CRC-verified payload. */
class Packet(val type: MessageType, val flags: Int, val payload: ByteArray)

/** Stateless single-frame codec. */
object PacketCodec {
    /** Serializes one packet to a complete frame (header + payload). */
    fun encode(packet: Packet): ByteArray {
        if (packet.payload.size > MAX_PAYLOAD_LEN) {
            throw ProtocolException(
                ProtocolError.PayloadTooLarge,
                "payload too large: ${packet.payload.size}",
            )
        }
        val out = ByteBuffer.allocate(HEADER_LEN + packet.payload.size).order(ByteOrder.LITTLE_ENDIAN)
        out.put(MAGIC)
        out.put(PROTOCOL_VERSION.toByte())
        out.put(packet.type.wire.toByte())
        out.put(packet.flags.toByte())
        out.put(0)
        out.putInt(packet.payload.size)
        out.putInt(crc32(packet.payload))
        out.put(packet.payload)
        return out.array()
    }

    /** Decodes exactly one complete frame; extra or missing bytes are errors. */
    fun decode(frame: ByteArray): Packet {
        if (frame.size < HEADER_LEN) {
            throw ProtocolException(ProtocolError.MessageDecode, "truncated header: ${frame.size} bytes")
        }
        val header = parseHeader(frame, 0)
        val total = HEADER_LEN + header.payloadLen
        if (frame.size != total) {
            throw ProtocolException(
                ProtocolError.MessageDecode,
                "expected $total bytes, got ${frame.size}",
            )
        }
        val payload = frame.copyOfRange(HEADER_LEN, total)
        verifyCrc(payload, header.crc32)
        return Packet(header.type, header.flags, payload)
    }
}

/**
 * Incremental stream decoder: feed arbitrary chunks, get complete frames.
 * A structural violation throws [ProtocolException]; resynchronisation is not
 * attempted by design, the caller drops the connection.
 */
class StreamDecoder {
    private var buffer = ByteArray(INITIAL_BUFFER)
    private var size = 0

    fun bufferedLen(): Int = size

    fun feed(input: ByteArray): List<Packet> {
        append(input)
        val packets = mutableListOf<Packet>()
        var offset = 0
        while (size - offset >= HEADER_LEN) {
            val header = parseHeader(buffer, offset)
            val total = HEADER_LEN + header.payloadLen
            if (size - offset < total) {
                break // wait for the rest; never partially emit
            }
            val payload = buffer.copyOfRange(offset + HEADER_LEN, offset + total)
            verifyCrc(payload, header.crc32)
            packets += Packet(header.type, header.flags, payload)
            offset += total
        }
        if (offset > 0) {
            System.arraycopy(buffer, offset, buffer, 0, size - offset)
            size -= offset
        }
        return packets
    }

    private fun append(input: ByteArray) {
        if (size + input.size > buffer.size) {
            var capacity = buffer.size
            while (capacity < size + input.size) {
                capacity *= 2
            }
            buffer = buffer.copyOf(capacity)
        }
        System.arraycopy(input, 0, buffer, size, input.size)
        size += input.size
    }

    private companion object {
        const val INITIAL_BUFFER = 4096
    }
}

private class Header(val type: MessageType, val flags: Int, val payloadLen: Int, val crc32: Int)

private fun parseHeader(bytes: ByteArray, offset: Int): Header {
    val header = ByteBuffer.wrap(bytes, offset, HEADER_LEN).order(ByteOrder.LITTLE_ENDIAN)
    val magic = ByteArray(MAGIC.size)
    header.get(magic)
    if (!magic.contentEquals(MAGIC)) {
        throw ProtocolException(ProtocolError.BadMagic, "bad magic (expected WDG1)")
    }
    val version = header.get().toInt() and BYTE_MASK
    if (version != PROTOCOL_VERSION) {
        throw ProtocolException(ProtocolError.UnsupportedVersion, "unsupported version $version")
    }
    val typeByte = header.get().toInt() and BYTE_MASK
    val flags = header.get().toInt() and BYTE_MASK
    val reserved = header.get().toInt() and BYTE_MASK
    if (reserved != 0) {
        throw ProtocolException(ProtocolError.ReservedNonZero, "reserved byte must be 0, got $reserved")
    }
    val payloadLen = header.int
    if (payloadLen < 0 || payloadLen > MAX_PAYLOAD_LEN) {
        throw ProtocolException(ProtocolError.PayloadTooLarge, "payload too large: $payloadLen")
    }
    val crc = header.int
    val type =
        MessageType.fromWire(typeByte)
            ?: throw ProtocolException(ProtocolError.UnknownType, "unknown message type 0x%02x".format(typeByte))
    return Header(type, flags, payloadLen, crc)
}

private fun verifyCrc(payload: ByteArray, expected: Int) {
    val actual = crc32(payload)
    if (actual != expected) {
        throw ProtocolException(
            ProtocolError.CrcMismatch,
            "crc mismatch: expected ${expected.toUInt()}, got ${actual.toUInt()}",
        )
    }
}

private fun crc32(payload: ByteArray): Int {
    val crc = CRC32()
    crc.update(payload)
    return crc.value.toInt()
}
