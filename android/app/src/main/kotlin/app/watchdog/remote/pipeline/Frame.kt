package app.watchdog.remote.pipeline

/** One encoded live-view frame (protocol `FRAME`, `docs/protocol.md` §5.3). */
data class Frame(
    val sequence: Long,
    val sensorTimestampNs: Long,
    val width: Int,
    val height: Int,
    val rotation: Int,
    val jpeg: ByteArray,
)

/** Bounded frame configuration; size/quality/FPS are settable from the host. */
data class PipelineConfig(
    val width: Int = 1280,
    val height: Int = 720,
    val jpegQuality: Int = 80,
    val rotation: Int = 0,
    val targetFps: Int = 15,
    val queueCapacity: Int = 4,
)

/** Destination of encoded frames; the transport implements this in M3-T06. */
fun interface FrameSink {
    fun send(frame: Frame)
}

/** Snapshot of queue counters for the on-device UI and `STATE`. */
data class QueueStats(val offered: Long, val dropped: Long, val depth: Int)
