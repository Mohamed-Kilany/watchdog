package app.watchdog.remote.pipeline

import java.util.ArrayDeque
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock

/**
 * Bounded queue with drop-oldest policy: a slow consumer never blocks the
 * camera thread and never grows memory without bound. Dropped frames show up
 * as `sequence` gaps on the host, which the protocol explicitly allows.
 */
class FrameQueue(private val capacity: Int) {
    private val lock = ReentrantLock()
    private val notEmpty = lock.newCondition()
    private val frames = ArrayDeque<Frame>(capacity)

    private var offered = 0L
    private var dropped = 0L

    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    /** Enqueues [frame], dropping the oldest frame when the queue is full. */
    fun offer(frame: Frame): Boolean {
        lock.lock()
        try {
            if (frames.size == capacity) {
                frames.removeFirst()
                dropped++
            }
            frames.addLast(frame)
            offered++
            notEmpty.signal()
            return true
        } finally {
            lock.unlock()
        }
    }

    /** Waits up to [timeoutMs] for a frame; returns null on timeout. */
    fun poll(timeoutMs: Long): Frame? {
        lock.lock()
        try {
            var remainingNs = TimeUnit.MILLISECONDS.toNanos(timeoutMs)
            while (frames.isEmpty()) {
                if (remainingNs <= 0) {
                    return null
                }
                remainingNs = notEmpty.awaitNanos(remainingNs)
            }
            return frames.removeFirst()
        } finally {
            lock.unlock()
        }
    }

    fun stats(): QueueStats {
        lock.lock()
        try {
            return QueueStats(offered = offered, dropped = dropped, depth = frames.size)
        } finally {
            lock.unlock()
        }
    }
}
