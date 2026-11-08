package app.watchdog.remote.protocol

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class LogChunkTest {
    @Test
    fun roundTripPreservesTimestampLevelAndMultilineText() {
        val record = LogRecord(1_730_900_001_234, LogLevel.WARN, "WatchdogCamera: line one\nline two")

        val decoded = LogChunkCodec.decode(LogChunkCodec.encode(record))

        assertEquals(record, decoded)
    }

    @Test
    fun payloadLayoutMatchesProtocol() {
        val payload = LogChunkCodec.encode(LogRecord(0x0102030405060708, LogLevel.ERROR, "boom"))

        assertEquals(9 + 4, payload.size)
        assertArrayEquals(
            byteArrayOf(
                0x08,
                0x07,
                0x06,
                0x05,
                0x04,
                0x03,
                0x02,
                0x01,
                4,
                'b'.code.toByte(),
                'o'.code.toByte(),
                'o'.code.toByte(),
                'm'.code.toByte(),
            ),
            payload,
        )
    }

    @Test
    fun decodeRejectsTruncatedPayload() {
        assertThrows(ProtocolException::class.java) {
            LogChunkCodec.decode(byteArrayOf(1, 2, 3))
        }
    }

    @Test
    fun decodeRejectsUnknownLevel() {
        val payload = LogChunkCodec.encode(LogRecord(1, LogLevel.INFO, "x"))
        payload[8] = 9

        assertThrows(ProtocolException::class.java) {
            LogChunkCodec.decode(payload)
        }
    }

    @Test
    fun oversizedTextIsTruncatedButPayloadStaysValid() {
        val record = LogRecord(7, LogLevel.DEBUG, "x".repeat(LogChunkCodec.MAX_TEXT_BYTES + 100))

        val decoded = LogChunkCodec.decode(LogChunkCodec.encode(record))

        assertEquals(LogChunkCodec.MAX_TEXT_BYTES, decoded.text.length)
        assertEquals(LogChunkCodec.MAX_TEXT_BYTES + LogChunkCodec.HEADER_SIZE, LogChunkCodec.encodedSize(record))
    }

    @Test
    fun batchPreservesEveryRecordAcrossPayloads() {
        val records = (1..500).map { LogRecord(it.toLong(), LogLevel.INFO, "line $it") }

        val batches = LogChunkCodec.encodeBatch(records, maxBytes = 512)

        assertTrue(batches.size > 1)
        val lines = batches.flatMap { LogChunkCodec.decode(it).text.lines() }.filter { it.isNotEmpty() }
        assertEquals(500, lines.size)
        assertEquals("1 INFO line 1", lines.first())
        assertEquals("500 INFO line 500", lines.last())
    }

    @Test
    fun batchUsesFirstTimestampAndHighestLevel() {
        val records =
            listOf(
                LogRecord(10, LogLevel.DEBUG, "a"),
                LogRecord(20, LogLevel.ERROR, "b"),
                LogRecord(30, LogLevel.INFO, "c"),
            )

        val decoded = LogChunkCodec.decode(LogChunkCodec.encodeBatch(records).single())

        assertEquals(10, decoded.timestampMs)
        assertEquals(LogLevel.ERROR, decoded.level)
    }

    @Test
    fun emptyBacklogProducesNoBatches() {
        assertTrue(LogChunkCodec.encodeBatch(emptyList()).isEmpty())
    }
}
