package app.watchdog.remote.log

import app.watchdog.remote.protocol.LogLevel
import app.watchdog.remote.protocol.LogRecord
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LogRingTest {
    private fun record(seq: Int, text: String = "message $seq") =
        LogRecord(seq.toLong(), LogLevel.INFO, text)

    @Test
    fun keepsRecordsInAppendOrder() {
        val ring = LogRing(1024)

        repeat(5) { ring.append(record(it)) }

        assertEquals(listOf(0L, 1L, 2L, 3L, 4L), ring.snapshot().map { it.timestampMs })
        assertEquals(5, ring.count())
    }

    @Test
    fun evictsOldestRecordsWhenCapacityIsExceeded() {
        val text = "x".repeat(50)
        val recordBytes = 9 + text.length
        val ring = LogRing(recordBytes * 3 + 10)

        repeat(5) { ring.append(record(it, text)) }

        assertEquals(3, ring.count())
        assertEquals(listOf(2L, 3L, 4L), ring.snapshot().map { it.timestampMs })
        assertTrue(ring.sizeBytes() <= recordBytes * 3 + 10)
    }

    @Test
    fun retainsLastBytesAcrossManySmallRecords() {
        val ring = LogRing(256)

        repeat(1_000) { ring.append(record(it)) }

        assertTrue(ring.sizeBytes() <= 256)
        assertTrue(ring.snapshot().first().timestampMs > 900L)
        assertEquals(999L, ring.snapshot().last().timestampMs)
    }

    @Test
    fun rejectsSingleRecordLargerThanCapacity() {
        val ring = LogRing(32)

        assertFalse(ring.append(record(1, "x".repeat(64))))
        assertEquals(0, ring.count())
    }

    @Test
    fun clearDropsEverything() {
        val ring = LogRing(1024)
        repeat(3) { ring.append(record(it)) }

        ring.clear()

        assertEquals(0, ring.count())
        assertEquals(0, ring.sizeBytes())
    }
}
