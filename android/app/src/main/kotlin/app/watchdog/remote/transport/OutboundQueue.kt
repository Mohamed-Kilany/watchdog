package app.watchdog.remote.transport

import app.watchdog.remote.log.DeviceLog
import app.watchdog.remote.protocol.MessageType
import app.watchdog.remote.protocol.Packet
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

/**
 * Bounded outbound queue (`docs/protocol.md` §8). `FRAME` and `LOG_CHUNK` are
 * dropped when the queue is full so a slow host only causes sequence gaps or
 * missing log lines; control messages wait briefly and then report failure,
 * which fails the session instead of silently losing state.
 */
class OutboundQueue(capacity: Int, private val controlWaitMs: Long = CONTROL_WAIT_MS) {
    private val queue = ArrayBlockingQueue<Packet>(capacity)
    private val dropped = AtomicLong()

    @Volatile
    private var closed = false

    /** Enqueues [packet]; returns false when dropped or the queue is closed. */
    fun offer(packet: Packet): Boolean {
        if (closed) {
            return false
        }
        if (packet.type == MessageType.FRAME || packet.type == MessageType.LOG_CHUNK) {
            val accepted = queue.offer(packet)
            if (!accepted) {
                dropped.incrementAndGet()
            }
            return accepted
        }
        return try {
            queue.offer(packet, controlWaitMs, TimeUnit.MILLISECONDS)
        } catch (e: InterruptedException) {
            DeviceLog.d(TAG, "control offer interrupted: ${e.message}")
            Thread.currentThread().interrupt()
            false
        }
    }

    fun poll(timeoutMs: Long): Packet? =
        try {
            queue.poll(timeoutMs, TimeUnit.MILLISECONDS)
        } catch (e: InterruptedException) {
            DeviceLog.d(TAG, "poll interrupted: ${e.message}")
            Thread.currentThread().interrupt()
            null
        }

    fun droppedFrames(): Long = dropped.get()

    fun depth(): Int = queue.size

    fun close() {
        closed = true
        queue.clear()
    }

    companion object {
        private const val CONTROL_WAIT_MS = 1_000L
        private const val TAG = "WatchdogOutbound"
    }
}
