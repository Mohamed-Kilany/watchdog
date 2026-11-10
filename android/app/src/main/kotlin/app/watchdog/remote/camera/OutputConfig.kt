package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject

/** One supported output size (pixels). */
data class OutputSize(val width: Int, val height: Int) {
    fun toJson(): JSONObject = JSONObject().put("width", width).put("height", height)

    override fun toString(): String = "${width}x$height"
}

/**
 * Runtime output configuration (M4-T09): camera/physical camera selection, the
 * live stream, still capture and video recording settings.
 *
 * The values are validated against the capability descriptor before they are
 * applied. Changing the stream size or camera rebuilds the frame pipeline and
 * capture session; quality/rotation changes are picked up by the next frame.
 */
data class OutputConfig(
    val cameraId: String,
    val physicalId: String?,
    val streamSize: OutputSize,
    val jpegQuality: Int,
    val rotation: Int,
    val stillFormat: String,
    val stillSize: OutputSize,
    val videoSize: OutputSize,
    val videoBitrate: Int,
    val videoCodec: String,
    val videoContainer: String,
    val audio: Boolean,
)

/** An output-configuration validation error, attributed to one control id. */
data class OutputError(val control: String, val code: String, val message: String)

/** Result of validating one structural output batch. */
data class OutputValidation(
    val ok: Boolean,
    val config: OutputConfig,
    val applied: Map<String, Any>,
    val errors: Map<String, OutputError>,
)

/** Defaults, descriptor parsing and pure combination rules for [OutputConfig]. */
object OutputConfigs {
    const val CAMERA_ID = "camera_id"
    const val PHYSICAL = "physical_camera_id"
    const val STREAM_SIZE = "stream_size"
    const val JPEG_QUALITY = "jpeg_quality"
    const val STREAM_ROTATION = "stream_rotation"
    const val STILL_FORMAT = "still_format"
    const val STILL_SIZE = "still_size"
    const val VIDEO_SIZE = "video_size"
    const val VIDEO_BITRATE = "video_bitrate"
    const val VIDEO_CODEC = "video_codec"
    const val VIDEO_CONTAINER = "video_container"
    const val AUDIO = "audio"

    val CONTROL_IDS =
        setOf(
            CAMERA_ID,
            PHYSICAL,
            STREAM_SIZE,
            JPEG_QUALITY,
            STREAM_ROTATION,
            STILL_FORMAT,
            STILL_SIZE,
            VIDEO_SIZE,
            VIDEO_BITRATE,
            VIDEO_CODEC,
            VIDEO_CONTAINER,
            AUDIO,
        )

    const val LOGICAL = "none"
    const val JPEG = "JPEG"
    const val RAW = "RAW_SENSOR"
    const val H264 = "h264"
    const val H265 = "h265"
    const val MP4 = "mp4"

    const val MIN_JPEG_QUALITY = 1
    const val MAX_JPEG_QUALITY = 100
    const val DEFAULT_JPEG_QUALITY = 80
    const val DEFAULT_VIDEO_BITRATE = 8_000_000
    const val MIN_VIDEO_BITRATE = 100_000
    const val MAX_VIDEO_BITRATE = 100_000_000
    const val DEFAULT_STREAM_WIDTH = 1280
    const val DEFAULT_STREAM_HEIGHT = 720
    const val DEFAULT_VIDEO_WIDTH = 1920
    const val DEFAULT_VIDEO_HEIGHT = 1080
    const val PREVIEW_MAX_WIDTH = 1920
    const val PREVIEW_MAX_HEIGHT = 1080
    const val PREFERRED_MAX_FPS = 30
    const val NANOS_PER_SECOND = 1_000_000_000.0
    const val FPS_TOLERANCE_NS = 1_000_000.0

    private const val ROTATION_90 = 90
    private const val ROTATION_180 = 180
    private const val ROTATION_270 = 270

    val ROTATIONS = listOf(0, ROTATION_90, ROTATION_180, ROTATION_270)

    /** The configuration used when a camera is opened or selected. */
    fun defaults(cameraId: String, descriptor: JSONObject): OutputConfig {
        val stream = pickStreamSize(descriptor)
        val formats = values(descriptor.optJSONObject(STILL_FORMAT))
        val format = if (JPEG in formats) JPEG else formats.firstOrNull() ?: JPEG
        val still = stillSizes(descriptor, format).maxByOrNull { it.area() } ?: stream
        val codecs = values(descriptor.optJSONObject(VIDEO_CODEC))
        val containers = values(descriptor.optJSONObject(VIDEO_CONTAINER))
        return OutputConfig(
            cameraId = cameraId,
            physicalId = null,
            streamSize = stream,
            jpegQuality = DEFAULT_JPEG_QUALITY,
            rotation = 0,
            stillFormat = format,
            stillSize = still,
            videoSize = pickVideoSize(descriptor) ?: stream,
            videoBitrate = DEFAULT_VIDEO_BITRATE,
            videoCodec = if (H264 in codecs) H264 else codecs.firstOrNull() ?: H264,
            videoContainer = if (MP4 in containers) MP4 else containers.firstOrNull() ?: MP4,
            audio = false,
        )
    }

    fun parseSize(value: Any?): OutputSize? {
        val objectValue = value as? JSONObject ?: return null
        val width = ControlValues.asInt(objectValue.opt("width")) ?: return null
        val height = ControlValues.asInt(objectValue.opt("height")) ?: return null
        return if (width > 0 && height > 0) OutputSize(width, height) else null
    }

    fun asString(value: Any?): String? =
        when (value) {
            is String -> value
            is Number -> value.toString()
            else -> null
        }

    fun values(spec: JSONObject?): List<String> {
        val array = spec?.optJSONArray("values") ?: return emptyList()
        val out = ArrayList<String>(array.length())
        for (index in 0 until array.length()) {
            array.optString(index).takeIf { it.isNotEmpty() }?.let(out::add)
        }
        return out
    }

    fun stillSizes(descriptor: JSONObject, format: String): List<OutputSize> {
        val spec = descriptor.optJSONObject(STILL_SIZE) ?: return emptyList()
        val byFormat = spec.optJSONObject("sizes_by_format") ?: return emptyList()
        return sizes(byFormat.optJSONArray(format))
    }

    fun sizes(array: JSONArray?): List<OutputSize> {
        if (array == null) return emptyList()
        val out = ArrayList<OutputSize>(array.length())
        for (index in 0 until array.length()) {
            val entry = array.optJSONObject(index) ?: continue
            val width = ControlValues.asInt(entry.opt("width")) ?: continue
            val height = ControlValues.asInt(entry.opt("height")) ?: continue
            if (width > 0 && height > 0) out += OutputSize(width, height)
        }
        return out
    }

    fun streamSizes(descriptor: JSONObject): List<OutputSize> =
        sizes(descriptor.optJSONObject(STREAM_SIZE)?.optJSONArray("sizes"))

    fun videoSizes(descriptor: JSONObject): List<OutputSize> =
        sizes(descriptor.optJSONObject(VIDEO_SIZE)?.optJSONArray("sizes"))

    fun fpsPair(value: Any?): Pair<Int, Int>? {
        val first =
            when (value) {
                is JSONArray -> value.opt(0)
                is JSONObject -> value.opt("min")
                is List<*> -> value.getOrNull(0)
                else -> null
            }
        val second =
            when (value) {
                is JSONArray -> value.opt(1)
                is JSONObject -> value.opt("max")
                is List<*> -> value.getOrNull(1)
                else -> null
            }
        val min = ControlValues.asInt(first) ?: return null
        val max = ControlValues.asInt(second) ?: return null
        return if (min > 0 && max >= min) min to max else null
    }

    /**
     * Whether [fpsRange] can sustain the configured stream size. The descriptor
     * carries the minimum frame duration for each size, so a high FPS request on
     * an oversized stream is rejected before it reaches the HAL.
     */
    fun isFpsSupported(descriptor: JSONObject, size: OutputSize, fpsRange: Any?): Boolean {
        val spec = descriptor.optJSONObject(STREAM_SIZE) ?: return true
        if (!spec.optBoolean("supported")) return true
        val pair = fpsPair(fpsRange) ?: return false
        val entry = sizeEntry(spec.optJSONArray("sizes"), size) ?: return false
        val minDuration = entry.optLong("min_frame_duration_ns", 0L)
        if (minDuration <= 0L) return true
        return NANOS_PER_SECOND / pair.second + FPS_TOLERANCE_NS >= minDuration
    }

    /** Best supported range for the stream size, preferring a fixed 30 fps. */
    fun defaultFpsRange(descriptor: JSONObject, size: OutputSize): JSONArray? {
        val ranges = descriptor.optJSONObject("ae_fps_range")?.optJSONArray("ranges") ?: return null
        var best: Pair<Int, Int>? = null
        var fallback: Pair<Int, Int>? = null
        for (index in 0 until ranges.length()) {
            val pair = fpsPair(ranges.optJSONArray(index)) ?: continue
            if (!isFpsSupported(descriptor, size, ranges.optJSONArray(index))) continue
            if (fallback == null || pair.second > fallback.second) fallback = pair
            if (pair.second <= PREFERRED_MAX_FPS && betterRange(pair, best)) {
                best = pair
            }
        }
        val chosen = best ?: fallback ?: return null
        return JSONArray().put(chosen.first).put(chosen.second)
    }

    /** Prefer the highest upper bound, then the highest lower bound (a fixed 30 fps). */
    private fun betterRange(candidate: Pair<Int, Int>, best: Pair<Int, Int>?): Boolean {
        if (best == null) return true
        if (candidate.second != best.second) return candidate.second > best.second
        return candidate.first > best.first
    }

    /**
     * The only hard combination rule the app can prove from the guaranteed
     * Camera2 tables: RAW still capture with a live stream is guaranteed only
     * while the live stream is at preview size (up to 1080p). JPEG stills and
     * recording combinations are validated by the capture-session callback.
     */
    fun combinationError(config: OutputConfig, descriptor: JSONObject): OutputError? {
        if (config.stillFormat != RAW) return null
        if (!hasRawCapability(descriptor)) {
            return OutputError(STILL_FORMAT, "UNSUPPORTED", "RAW_SENSOR is not supported by this camera")
        }
        val stream = config.streamSize
        if (stream.width <= PREVIEW_MAX_WIDTH && stream.height <= PREVIEW_MAX_HEIGHT) return null
        return OutputError(
            STREAM_SIZE,
            "UNSUPPORTED_COMBINATION",
            "RAW still capture with a live stream is only guaranteed up to " +
                "${PREVIEW_MAX_WIDTH}x$PREVIEW_MAX_HEIGHT; use JPEG or a smaller stream_size",
        )
    }

    private fun hasRawCapability(descriptor: JSONObject): Boolean {
        val capabilities = descriptor.optJSONArray("capabilities") ?: return false
        for (index in 0 until capabilities.length()) {
            if (capabilities.optString(index) == "RAW") return true
        }
        return false
    }

    private fun sizeEntry(array: JSONArray?, size: OutputSize): JSONObject? {
        if (array == null) return null
        for (index in 0 until array.length()) {
            val entry = array.optJSONObject(index) ?: continue
            val width = ControlValues.asInt(entry.opt("width")) ?: continue
            val height = ControlValues.asInt(entry.opt("height")) ?: continue
            if (width == size.width && height == size.height) return entry
        }
        return null
    }

    private fun pickStreamSize(descriptor: JSONObject): OutputSize {
        val sizes = streamSizes(descriptor)
        val preferred = OutputSize(DEFAULT_STREAM_WIDTH, DEFAULT_STREAM_HEIGHT)
        return pickAtMost(sizes, preferred) ?: sizes.minByOrNull { it.area() } ?: preferred
    }

    private fun pickVideoSize(descriptor: JSONObject): OutputSize? {
        val sizes = videoSizes(descriptor)
        if (sizes.isEmpty()) return null
        val preferred = OutputSize(DEFAULT_VIDEO_WIDTH, DEFAULT_VIDEO_HEIGHT)
        return pickAtMost(sizes, preferred) ?: sizes.minByOrNull { it.area() }
    }

    private fun pickAtMost(sizes: List<OutputSize>, preferred: OutputSize): OutputSize? {
        if (preferred in sizes) return preferred
        return sizes
            .sortedByDescending { it.area() }
            .firstOrNull { it.width <= preferred.width && it.height <= preferred.height }
    }

    private fun OutputSize.area(): Int = width * height
}
