package app.watchdog.remote.camera

import android.graphics.ImageFormat
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.params.StreamConfigurationMap
import android.media.MediaCodecList
import android.media.MediaRecorder
import android.os.Build
import android.util.Size
import org.json.JSONArray
import org.json.JSONObject

/**
 * M4-T09 descriptor entries for output configuration. Sizes and codecs are
 * derived from the camera's `StreamConfigurationMap` and the platform encoder
 * list, so the host can only request combinations the device reports.
 */
object OutputControls {
    private const val NO_STREAM_MAP_REASON = "camera does not expose a stream configuration map"
    private const val NO_STREAM_SIZES_REASON = "no even YUV_420_888 output size"
    private const val NO_STILL_SIZES_REASON = "no JPEG or RAW output size"
    private const val NO_PHYSICAL_REASON = "not a logical multi-camera"
    private const val NO_VIDEO_REASON = "no video output sizes reported by MediaRecorder"
    private const val NO_CODEC_REASON = "no H.264/H.265 encoder"
    private const val NO_AUDIO_REASON = "no AAC encoder"

    private val NO_STREAM_MAP_CONTROLS =
        listOf(
            OutputConfigs.STREAM_SIZE,
            OutputConfigs.JPEG_QUALITY,
            OutputConfigs.STREAM_ROTATION,
            OutputConfigs.STILL_FORMAT,
            OutputConfigs.STILL_SIZE,
            OutputConfigs.VIDEO_SIZE,
            OutputConfigs.VIDEO_BITRATE,
            OutputConfigs.VIDEO_CODEC,
            OutputConfigs.VIDEO_CONTAINER,
            OutputConfigs.AUDIO,
        )

    fun describe(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics, cameraIds: List<String>) {
        cameraSelection(out, unsupported, chars, cameraIds)
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        if (map == null) {
            NO_STREAM_MAP_CONTROLS.forEach { unsupported.put(unsupportedEntry(it, NO_STREAM_MAP_REASON)) }
            return
        }
        streamControls(out, unsupported, map)
        stillControls(out, unsupported, chars, map)
        videoControls(out, unsupported, map)
    }

    private fun cameraSelection(
        out: JSONObject,
        unsupported: JSONArray,
        chars: CameraCharacteristics,
        cameraIds: List<String>,
    ) {
        val ids = JSONArray()
        cameraIds.forEach { ids.put(it) }
        out.put(OutputConfigs.CAMERA_ID, structural(JSONObject().put("values", ids)))
        val physical = physicalIds(chars)
        if (physical.isEmpty()) {
            out.put(OutputConfigs.PHYSICAL, JSONObject().put("supported", false))
            unsupported.put(unsupportedEntry(OutputConfigs.PHYSICAL, NO_PHYSICAL_REASON))
            return
        }
        val values = JSONArray().put(OutputConfigs.LOGICAL)
        physical.forEach { values.put(it) }
        out.put(OutputConfigs.PHYSICAL, structural(JSONObject().put("values", values)))
    }

    private fun streamControls(out: JSONObject, unsupported: JSONArray, map: StreamConfigurationMap) {
        val sizes =
            map.getOutputSizes(ImageFormat.YUV_420_888)
                ?.filter { it.width % 2 == 0 && it.height % 2 == 0 }
                .orEmpty()
        if (sizes.isEmpty()) {
            unsupported.put(unsupportedEntry(OutputConfigs.STREAM_SIZE, NO_STREAM_SIZES_REASON))
        } else {
            out.put(
                OutputConfigs.STREAM_SIZE,
                structural(JSONObject().put("sizes", streamSizes(map, sizes))),
            )
        }
        out.put(
            OutputConfigs.JPEG_QUALITY,
            structural(
                range(OutputConfigs.MIN_JPEG_QUALITY, OutputConfigs.MAX_JPEG_QUALITY),
            ),
        )
        out.put(
            OutputConfigs.STREAM_ROTATION,
            structural(JSONObject().put("modes", modes(OutputConfigs.ROTATIONS))),
        )
    }

    private fun stillControls(
        out: JSONObject,
        unsupported: JSONArray,
        chars: CameraCharacteristics,
        map: StreamConfigurationMap,
    ) {
        val jpeg = map.getOutputSizes(ImageFormat.JPEG)?.toList().orEmpty()
        val raw =
            if (supportsRaw(chars)) {
                map.getOutputSizes(ImageFormat.RAW_SENSOR)?.toList().orEmpty()
            } else {
                emptyList()
            }
        val formats = JSONArray()
        if (jpeg.isNotEmpty()) formats.put(OutputConfigs.JPEG)
        if (raw.isNotEmpty()) formats.put(OutputConfigs.RAW)
        if (formats.length() == 0) {
            unsupported.put(unsupportedEntry(OutputConfigs.STILL_FORMAT, NO_STILL_SIZES_REASON))
            unsupported.put(unsupportedEntry(OutputConfigs.STILL_SIZE, NO_STILL_SIZES_REASON))
            return
        }
        out.put(OutputConfigs.STILL_FORMAT, structural(JSONObject().put("values", formats)))
        val byFormat = JSONObject()
        if (jpeg.isNotEmpty()) byFormat.put(OutputConfigs.JPEG, sizesArray(jpeg))
        if (raw.isNotEmpty()) byFormat.put(OutputConfigs.RAW, sizesArray(raw))
        out.put(
            OutputConfigs.STILL_SIZE,
            structural(JSONObject().put("sizes_by_format", byFormat)),
        )
    }

    private fun videoControls(out: JSONObject, unsupported: JSONArray, map: StreamConfigurationMap) {
        val sizes =
            map.getOutputSizes(MediaRecorder::class.java)?.toList()
                ?: map.getOutputSizes(SurfaceTexture::class.java)?.toList().orEmpty()
        if (sizes.isEmpty()) {
            unsupported.put(unsupportedEntry(OutputConfigs.VIDEO_SIZE, NO_VIDEO_REASON))
        } else {
            out.put(OutputConfigs.VIDEO_SIZE, structural(JSONObject().put("sizes", sizesArray(sizes))))
        }
        out.put(
            OutputConfigs.VIDEO_BITRATE,
            structural(range(OutputConfigs.MIN_VIDEO_BITRATE, OutputConfigs.MAX_VIDEO_BITRATE)),
        )
        val codecs = videoCodecs()
        if (codecs.isEmpty()) {
            unsupported.put(unsupportedEntry(OutputConfigs.VIDEO_CODEC, NO_CODEC_REASON))
            unsupported.put(unsupportedEntry(OutputConfigs.VIDEO_CONTAINER, NO_CODEC_REASON))
        } else {
            out.put(OutputConfigs.VIDEO_CODEC, structural(JSONObject().put("values", strings(codecs))))
            out.put(
                OutputConfigs.VIDEO_CONTAINER,
                structural(JSONObject().put("values", strings(listOf(OutputConfigs.MP4)))),
            )
        }
        if (hasEncoder("audio/mp4a-latm")) {
            out.put(OutputConfigs.AUDIO, structural(JSONObject()))
        } else {
            out.put(OutputConfigs.AUDIO, JSONObject().put("supported", false))
            unsupported.put(unsupportedEntry(OutputConfigs.AUDIO, NO_AUDIO_REASON))
        }
    }

    private fun streamSizes(map: StreamConfigurationMap, sizes: List<Size>): JSONArray {
        val out = JSONArray()
        sizes.sortedByDescending { it.width * it.height }.forEach { size ->
            val duration = map.getOutputMinFrameDuration(ImageFormat.YUV_420_888, size)
            out.put(
                OutputSize(size.width, size.height)
                    .toJson()
                    .put("min_frame_duration_ns", duration),
            )
        }
        return out
    }

    private fun sizesArray(sizes: List<Size>): JSONArray {
        val out = JSONArray()
        sizes.sortedByDescending { it.width * it.height }.forEach { size ->
            out.put(OutputSize(size.width, size.height).toJson())
        }
        return out
    }

    private fun range(min: Int, max: Int): JSONObject =
        JSONObject().put("min", min).put("max", max).put("step", 1)

    private fun modes(values: List<Int>): JSONArray {
        val out = JSONArray()
        values.forEach { out.put(it) }
        return out
    }

    private fun strings(values: List<String>): JSONArray {
        val out = JSONArray()
        values.forEach { out.put(it) }
        return out
    }

    private fun structural(spec: JSONObject): JSONObject =
        spec.put("supported", true).put("structural", true)

    private fun supportsRaw(chars: CameraCharacteristics): Boolean =
        chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES)?.contains(
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_RAW,
        ) == true

    private fun physicalIds(chars: CameraCharacteristics): List<String> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            chars.physicalCameraIds.sorted()
        } else {
            emptyList()
        }

    private fun videoCodecs(): List<String> {
        val types = encoderTypes()
        val out = ArrayList<String>(2)
        if ("video/avc" in types) out += OutputConfigs.H264
        if ("video/hevc" in types) out += OutputConfigs.H265
        return out
    }

    private fun hasEncoder(mime: String): Boolean = mime in encoderTypes()

    private fun encoderTypes(): Set<String> {
        val types = HashSet<String>()
        val codecs = MediaCodecList(MediaCodecList.REGULAR_CODECS)
        codecs.codecInfos.filter { it.isEncoder }.forEach { info ->
            info.supportedTypes.forEach { types += it.lowercase() }
        }
        return types
    }

    private fun unsupportedEntry(control: String, reason: String): JSONObject =
        JSONObject().put("control", control).put("reason", reason)
}
