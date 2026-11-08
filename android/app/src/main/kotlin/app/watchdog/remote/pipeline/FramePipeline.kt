package app.watchdog.remote.pipeline

import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.media.Image
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import java.io.ByteArrayOutputStream
import java.util.concurrent.atomic.AtomicLong

/**
 * Analysis stream: `ImageReader` (YUV_420_888) → NV21 → JPEG → bounded queue.
 *
 * Conversion reuses the NV21 and JPEG buffers. The JPEG is never rotated on
 * the device; the frame header carries the rotation hint (protocol §5.3).
 * A sender thread drains the queue into [sink]; a slow sink only causes
 * drop-oldest, never unbounded memory or a blocked camera thread.
 */
class FramePipeline(private val config: PipelineConfig, private val sink: FrameSink) {
    private val queue = FrameQueue(config.queueCapacity)
    private val sequence = AtomicLong(0)
    private val jpegBuffer = ByteArrayOutputStream(INITIAL_JPEG_BUFFER)
    private var nv21 = ByteArray(nv21Size(config.width, config.height))

    private var thread: HandlerThread? = null
    private var reader: ImageReader? = null
    private var sender: Thread? = null

    @Volatile
    private var running = false

    /** Surface to add to the Camera2 capture session (null before [start]). */
    val surface: Surface?
        get() = reader?.surface

    fun start() {
        if (reader == null) {
            val pipelineThread = HandlerThread("watchdog-pipeline").apply { start() }
            val pipelineHandler = Handler(pipelineThread.looper)
            val imageReader = ImageReader.newInstance(config.width, config.height, ImageFormat.YUV_420_888, 2)
            imageReader.setOnImageAvailableListener(::onImageAvailable, pipelineHandler)
            thread = pipelineThread
            reader = imageReader
            running = true
            sender = Thread(::senderLoop, "watchdog-sender").apply { start() }
        }
    }

    fun stop() {
        running = false
        reader?.setOnImageAvailableListener(null, null)
        reader?.close()
        reader = null
        sender?.interrupt()
        sender = null
        thread?.quitSafely()
        thread = null
    }

    fun stats(): QueueStats = queue.stats()

    private fun onImageAvailable(source: ImageReader) {
        val image = source.acquireLatestImage() ?: return
        try {
            val jpeg = encode(image)
            if (jpeg != null) {
                queue.offer(
                    Frame(
                        sequence = sequence.getAndIncrement(),
                        sensorTimestampNs = image.timestamp,
                        width = image.width,
                        height = image.height,
                        rotation = config.rotation,
                        jpeg = jpeg,
                    ),
                )
            }
        } finally {
            image.close()
        }
    }

    private fun encode(image: Image): ByteArray? {
        val width = image.width
        val height = image.height
        if (width % 2 != 0 || height % 2 != 0) {
            Log.w(TAG, "dropping odd frame size ${width}x$height (NV21 needs even dimensions)")
            return null
        }
        val ySize = width * height
        val size = ySize + ySize / 2
        if (nv21.size != size) {
            nv21 = ByteArray(size)
        }
        copyY(image, width, height)
        copyVu(image, width, height, ySize)

        jpegBuffer.reset()
        val ok =
            YuvImage(nv21, ImageFormat.NV21, width, height, null)
                .compressToJpeg(Rect(0, 0, width, height), config.jpegQuality, jpegBuffer)
        return if (ok && jpegBuffer.size() > 0) {
            jpegBuffer.toByteArray()
        } else {
            Log.w(TAG, "JPEG encode produced no data")
            null
        }
    }

    private fun copyY(image: Image, width: Int, height: Int) {
        val plane = image.planes[0]
        val buffer = plane.buffer
        val rowStride = plane.rowStride
        for (row in 0 until height) {
            buffer.position(row * rowStride)
            buffer.get(nv21, row * width, width)
        }
    }

    private fun copyVu(image: Image, width: Int, height: Int, ySize: Int) {
        val uPlane = image.planes[1]
        val vPlane = image.planes[2]
        val uBuffer = uPlane.buffer
        val vBuffer = vPlane.buffer
        val chromaWidth = width / 2
        val chromaHeight = height / 2
        var dst = ySize
        for (row in 0 until chromaHeight) {
            val uRow = row * uPlane.rowStride
            val vRow = row * vPlane.rowStride
            for (col in 0 until chromaWidth) {
                nv21[dst++] = vBuffer.get(vRow + col * vPlane.pixelStride)
                nv21[dst++] = uBuffer.get(uRow + col * uPlane.pixelStride)
            }
        }
    }

    private fun senderLoop() {
        while (running) {
            val frame =
                try {
                    queue.poll(POLL_TIMEOUT_MS)
                } catch (e: InterruptedException) {
                    Log.d(TAG, "sender interrupted: ${e.message}")
                    break
                }
            if (frame != null) {
                sink.send(frame)
            }
        }
    }

    companion object {
        private const val TAG = "WatchdogPipeline"
        private const val POLL_TIMEOUT_MS = 100L
        private const val INITIAL_JPEG_BUFFER = 256 * 1024

        private fun nv21Size(width: Int, height: Int): Int = width * height + width * height / 2
    }
}
