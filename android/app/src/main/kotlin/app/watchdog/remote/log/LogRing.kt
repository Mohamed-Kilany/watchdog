package app.watchdog.remote.log

import app.watchdog.remote.protocol.LogChunkCodec
import app.watchdog.remote.protocol.LogRecord

/**
 * Bounded, byte-accounted ring of device log records. The oldest complete
 * record is evicted when a new record would exceed [capacityBytes], so memory
 * stays bounded no matter how noisy a failing camera gets.
 */
class LogRing(private val capacityBytes: Int = DEFAULT_CAPACITY_BYTES) {
    private val lock = Any()
    private val records = ArrayDeque<LogRecord>()
    private var bytes = 0

    init {
        require(capacityBytes > LogChunkCodec.HEADER_SIZE) {
            "capacityBytes must exceed the LOG_CHUNK header"
        }
    }

    /** Appends [record], evicting oldest records to stay within capacity. */
    fun append(record: LogRecord): Boolean {
        val size = LogChunkCodec.encodedSize(record)
        if (size > capacityBytes) {
            return false
        }
        synchronized(lock) {
            while (bytes + size > capacityBytes && records.isNotEmpty()) {
                bytes -= LogChunkCodec.encodedSize(records.removeFirst())
            }
            records.addLast(record)
            bytes += size
        }
        return true
    }

    /** Records in append order (oldest first). */
    fun snapshot(): List<LogRecord> = synchronized(lock) { records.toList() }

    /** Retained payload bytes, including per-record headers. */
    fun sizeBytes(): Int = synchronized(lock) { bytes }

    fun count(): Int = synchronized(lock) { records.size }

    fun clear() {
        synchronized(lock) {
            records.clear()
            bytes = 0
        }
    }

    companion object {
        /** Default retained log budget (last 256 KiB). */
        const val DEFAULT_CAPACITY_BYTES = 256 * 1024
    }
}
