package app.watchdog.remote.pipeline

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FrameQueueTest {
    private fun frame(sequence: Long): Frame =
        Frame(
            sequence = sequence,
            sensorTimestampNs = sequence * 1_000_000,
            width = 2,
            height = 2,
            rotation = 0,
            jpeg = byteArrayOf(0xFF.toByte(), 0xD8.toByte()),
        )

    @Test
    fun `drops oldest frame when full`() {
        val queue = FrameQueue(capacity = 2)
        queue.offer(frame(0))
        queue.offer(frame(1))
        queue.offer(frame(2))

        assertEquals(2, queue.stats().depth)
        assertEquals(1L, queue.stats().dropped)
        assertEquals(3L, queue.stats().offered)

        assertEquals(1L, queue.poll(0)?.sequence)
        assertEquals(2L, queue.poll(0)?.sequence)
        assertNull(queue.poll(0))
    }

    @Test
    fun `poll returns null on timeout`() {
        val queue = FrameQueue(capacity = 1)
        assertNull(queue.poll(10))
    }

    @Test
    fun `offer wakes a waiting poll`() {
        val queue = FrameQueue(capacity = 1)
        val received = arrayOfNulls<Frame>(1)
        val consumer =
            Thread {
                received[0] = queue.poll(2_000)
            }
        consumer.start()
        Thread.sleep(50)
        queue.offer(frame(7))
        consumer.join(2_000)
        assertEquals(7L, received[0]?.sequence)
    }

    @Test
    fun `rejects non-positive capacity`() {
        val error = runCatching { FrameQueue(capacity = 0) }.exceptionOrNull()
        assertEquals(IllegalArgumentException::class.java, error?.javaClass)
    }
}
