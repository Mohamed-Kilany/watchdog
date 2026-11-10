package app.watchdog.remote.transport

import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicBoolean

class OutboundQueueTest {
    @Test
    fun `frames are dropped when the queue is full`() {
        val queue = OutboundQueue(capacity = 2, controlWaitMs = 50)
        assertTrue(queue.offer(frame(1)))
        assertTrue(queue.offer(frame(2)))
        assertFalse(queue.offer(frame(3)))

        assertEquals(1L, queue.droppedFrames())
        assertEquals(2, queue.depth())
    }

    @Test
    fun `control packets wait for capacity instead of being dropped`() {
        val queue = OutboundQueue(capacity = 1, controlWaitMs = 2_000)
        assertTrue(queue.offer(frame(1)))

        val accepted = AtomicBoolean(false)
        val producer = Thread { accepted.set(queue.offer(control())) }
        producer.start()
        Thread.sleep(50)
        assertNotNull(queue.poll(1_000))
        producer.join(2_000)
        assertTrue(accepted.get())
        assertEquals(0L, queue.droppedFrames())
    }

    @Test
    fun `control packets evict a queued frame instead of failing`() {
        val queue = OutboundQueue(capacity = 1, controlWaitMs = 50)
        assertTrue(queue.offer(frame(1)))

        assertTrue(queue.offer(control()))

        assertEquals(1L, queue.droppedFrames())
        assertEquals(MessageType.STATE, queue.poll(0)?.type)
    }

    @Test
    fun `control packets are never evicted for other controls`() {
        val queue = OutboundQueue(capacity = 1, controlWaitMs = 50)
        assertTrue(queue.offer(control()))

        assertFalse(queue.offer(control()))

        assertEquals(0L, queue.droppedFrames())
        assertNotNull(queue.poll(0))
    }

    @Test
    fun `closed queue rejects packets`() {
        val queue = OutboundQueue(capacity = 1)
        queue.close()
        assertFalse(queue.offer(frame(1)))
        assertNull(queue.poll(10))
    }

    private fun frame(sequence: Int): Packet = Packet(MessageType.FRAME, 0, byteArrayOf(sequence.toByte()))

    private fun control(): Packet = Packet(MessageType.STATE, 0, """{"seq":1}""".toByteArray())
}
