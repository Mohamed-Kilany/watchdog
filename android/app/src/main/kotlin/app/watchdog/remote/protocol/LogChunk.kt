package app.watchdog.remote.protocol

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Log severity in `LOG_CHUNK` (`docs/protocol.md` §5.9). */
enum class LogLevel(val wire: Int) {
    TRACE(0),
    DEBUG(1),
    INFO(2),
    WARN(3),
    ERROR(4),
    ;

    companion object {
        fun fromWire(value: Int): LogLevel? = entries.firstOrNull { it.wire == value }
    }
}

/** One device log entry retained by the app ring buffer. */
data class LogRecord(val timestampMs: Long, val level: LogLevel, val text: String)

/** Binary `LOG_CHUNK` payload codec (`docs/protocol.md` §5.9). */
object LogChunkCodec {
    /** `ts_ms u64 LE | level u8`, followed by UTF-8 text. */
    const val HEADER_SIZE = 9

    /** Longest text kept in one record; longer messages are truncated. */
    const val MAX_TEXT_BYTES = 64 * 1024

    /** Target size of a backlog batch; keeps the send queue from overflowing. */
    const val MAX_BATCH_BYTES = 64 * 1024

    fun encode(record: LogRecord): ByteArray {
        val text = cappedText(record.text)
        return ByteBuffer
            .allocate(HEADER_SIZE + text.size)
            .order(ByteOrder.LITTLE_ENDIAN)
            .putLong(record.timestampMs)
            .put(record.level.wire.toByte())
            .put(text)
            .array()
    }

    /**
     * Packs a backlog into few `LOG_CHUNK` payloads: each line is
     * `<ts_ms> <LEVEL> <text>` so timestamps and levels survive batching.
     */
    fun encodeBatch(records: List<LogRecord>, maxBytes: Int = MAX_BATCH_BYTES): List<ByteArray> {
        val batches = mutableListOf<ByteArray>()
        val lines = mutableListOf<ByteArray>()
        var bytes = 0
        var firstTimestamp = 0L
        var maxLevel = LogLevel.TRACE

        fun flush() {
            if (lines.isEmpty()) {
                return
            }
            val text = lines.joinToString(separator = "") { String(it, Charsets.UTF_8) }
            batches += encode(LogRecord(firstTimestamp, maxLevel, text))
            lines.clear()
            bytes = 0
            maxLevel = LogLevel.TRACE
        }

        for (record in records) {
            val line = "${record.timestampMs} ${record.level.name} ${record.text}\n".toByteArray(Charsets.UTF_8)
            if (lines.isNotEmpty() && bytes + line.size > maxBytes) {
                flush()
            }
            if (lines.isEmpty()) {
                firstTimestamp = record.timestampMs
            }
            lines += line
            bytes += line.size
            if (record.level > maxLevel) {
                maxLevel = record.level
            }
        }
        flush()
        return batches
    }

    fun decode(payload: ByteArray): LogRecord {
        if (payload.size < HEADER_SIZE) {
            throw ProtocolException(
                ProtocolError.MessageDecode,
                "truncated LOG_CHUNK: ${payload.size} bytes",
            )
        }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val timestampMs = buffer.long
        val level =
            LogLevel.fromWire(payload[8].toInt() and BYTE_MASK)
                ?: throw ProtocolException(
                    ProtocolError.MessageDecode,
                    "invalid LOG_CHUNK level ${payload[8]}",
                )
        val text = String(payload, HEADER_SIZE, payload.size - HEADER_SIZE, Charsets.UTF_8)
        return LogRecord(timestampMs, level, text)
    }

    /** Payload size of [record] after text truncation. */
    fun encodedSize(record: LogRecord): Int = HEADER_SIZE + cappedText(record.text).size

    private fun cappedText(text: String): ByteArray {
        val bytes = text.toByteArray(Charsets.UTF_8)
        return if (bytes.size > MAX_TEXT_BYTES) bytes.copyOf(MAX_TEXT_BYTES) else bytes
    }

    private const val BYTE_MASK = 0xFF
}
