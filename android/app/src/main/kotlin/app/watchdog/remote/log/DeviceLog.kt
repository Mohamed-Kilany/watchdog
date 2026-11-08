package app.watchdog.remote.log

import app.watchdog.remote.protocol.LogChunkCodec
import app.watchdog.remote.protocol.LogLevel
import app.watchdog.remote.protocol.LogRecord
import java.io.PrintWriter
import java.io.StringWriter

/**
 * Process-wide device logger: every line is mirrored to logcat (when enabled
 * by a real Android component) and retained in a [LogRing]. While a host
 * session is attached, each new record is also streamed as a `LOG_CHUNK`.
 */
object DeviceLog {
    private val ring = LogRing()
    private val lock = Any()
    private var sink: ((ByteArray) -> Unit)? = null

    /** Logcat mirroring is disabled in JVM unit tests (android.jar stubs throw). */
    @Volatile
    var logcatEnabled = false

    /**
     * Attaches a live sink and returns the current backlog encoded as
     * `LOG_CHUNK` payloads. The sink receives records appended afterwards.
     */
    fun attach(sink: (ByteArray) -> Unit): List<ByteArray> =
        synchronized(lock) {
            this.sink = sink
            LogChunkCodec.encodeBatch(ring.snapshot())
        }

    fun detach() {
        synchronized(lock) { sink = null }
    }

    fun trace(tag: String, message: String) = log(LogLevel.TRACE, tag, message, null)

    fun d(tag: String, message: String) = log(LogLevel.DEBUG, tag, message, null)

    fun i(tag: String, message: String) = log(LogLevel.INFO, tag, message, null)

    fun w(tag: String, message: String, error: Throwable? = null) = log(LogLevel.WARN, tag, message, error)

    fun e(tag: String, message: String, error: Throwable? = null) = log(LogLevel.ERROR, tag, message, error)

    private fun log(level: LogLevel, tag: String, message: String, error: Throwable?) {
        val text = format(tag, message, error)
        if (logcatEnabled) {
            android.util.Log.println(level.wire + ANDROID_LOG_VERBOSE, tag, text)
        }
        val record = LogRecord(System.currentTimeMillis(), level, text)
        ring.append(record)
        val payload = LogChunkCodec.encode(record)
        val current = synchronized(lock) { sink }
        current?.invoke(payload)
    }

    private fun format(tag: String, message: String, error: Throwable?): String {
        if (error == null) {
            return "$tag: $message"
        }
        val writer = StringWriter()
        error.printStackTrace(PrintWriter(writer))
        return "$tag: $message\n$writer"
    }

    /** `android.util.Log.VERBOSE`; protocol levels 0..4 map onto 2..6. */
    private const val ANDROID_LOG_VERBOSE = 2
}
