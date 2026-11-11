package app.watchdog.remote.camera

import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.os.Build
import android.util.Range
import android.util.Rational
import android.util.Size
import app.watchdog.remote.log.DeviceLog
import org.json.JSONArray
import org.json.JSONObject
import java.lang.reflect.Field
import java.lang.reflect.Modifier
import java.lang.reflect.ParameterizedType

/**
 * M4-T01 capability descriptor: one JSON object per camera id, built only from
 * `CameraCharacteristics` without opening a camera.
 *
 * It is the machine-readable source of truth for what the host may request:
 * hardware level and capabilities, every control's available modes or numeric
 * range/step, output sizes and FPS ranges, and the complete inventory of
 * available request/result/characteristics keys (name + Java type, taken from
 * the key metadata). Unsupported controls are listed in `unsupported` with a
 * reason instead of being silently dropped.
 */
object CapabilityDescriptor {
    private const val TAG = "WatchdogCapability"
    private const val MIN_ZOOM_RATIO = 1.0f
    private const val CONTINUOUS_STEP = 0.0
    private const val NO_REASON = "not reported by CameraCharacteristics"
    private const val NO_FLASH_REASON = "no flash unit"
    private const val NO_ZOOM_REASON = "no digital zoom"
    private const val NO_AE_REGIONS_REASON = "no AE metering regions"
    private const val NO_AE_REASON = "AE is not available"
    private const val NO_AF_REGIONS_REASON = "no AF metering regions"
    private const val NO_AF_REASON = "AF is not available"
    private const val NO_FOCUS_DISTANCE_REASON = "fixed-focus lens"
    private const val UNKNOWN_TYPE = "unknown"

    private val KEY_CLASSES =
        listOf(
            CameraCharacteristics.Key::class.java,
            CaptureRequest.Key::class.java,
            CaptureResult.Key::class.java,
        )
    private val KEY_HOLDERS =
        listOf(
            CameraCharacteristics::class.java,
            CaptureRequest::class.java,
            CaptureResult::class.java,
        )
    private val KEY_TYPES: Map<String, String> by lazy { buildKeyTypes() }

    private val OUTPUT_FORMATS =
        intArrayOf(
            ImageFormat.YUV_420_888,
            ImageFormat.JPEG,
            ImageFormat.PRIVATE,
            ImageFormat.RAW_SENSOR,
        )

    private val CAPABILITY_NAMES =
        mapOf(
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_BACKWARD_COMPATIBLE to "BACKWARD_COMPATIBLE",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_MANUAL_SENSOR to "MANUAL_SENSOR",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_MANUAL_POST_PROCESSING to "MANUAL_POST_PROCESSING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_RAW to "RAW",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_PRIVATE_REPROCESSING to "PRIVATE_REPROCESSING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_READ_SENSOR_SETTINGS to "READ_SENSOR_SETTINGS",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_BURST_CAPTURE to "BURST_CAPTURE",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_YUV_REPROCESSING to "YUV_REPROCESSING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_DEPTH_OUTPUT to "DEPTH_OUTPUT",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_CONSTRAINED_HIGH_SPEED_VIDEO to
                "CONSTRAINED_HIGH_SPEED_VIDEO",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_MOTION_TRACKING to "MOTION_TRACKING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_LOGICAL_MULTI_CAMERA to "LOGICAL_MULTI_CAMERA",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_MONOCHROME to "MONOCHROME",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_SECURE_IMAGE_DATA to "SECURE_IMAGE_DATA",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_SYSTEM_CAMERA to "SYSTEM_CAMERA",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_OFFLINE_PROCESSING to "OFFLINE_PROCESSING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_ULTRA_HIGH_RESOLUTION_SENSOR to
                "ULTRA_HIGH_RESOLUTION_SENSOR",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_REMOSAIC_REPROCESSING to "REMOSAIC_REPROCESSING",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_DYNAMIC_RANGE_TEN_BIT to "DYNAMIC_RANGE_TEN_BIT",
            CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_STREAM_USE_CASE to "STREAM_USE_CASE",
        )

    /** Describe every camera currently reported by the camera service. */
    fun all(context: Context): JSONObject {
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val out = JSONObject()
        val ids =
            try {
                manager.cameraIdList
            } catch (e: CameraAccessException) {
                DeviceLog.e(TAG, "cannot enumerate cameras: ${e.message}")
                return out
            }
        for (id in ids) {
            try {
                out.put(id, describe(manager.getCameraCharacteristics(id), ids.toList()))
            } catch (e: CameraAccessException) {
                DeviceLog.w(TAG, "camera $id characteristics unavailable: ${e.message}")
                out.put(id, JSONObject().put("error", e.message ?: e.javaClass.simpleName))
            } catch (e: IllegalArgumentException) {
                DeviceLog.w(TAG, "camera $id disappeared: ${e.message}")
                out.put(id, JSONObject().put("error", e.message ?: e.javaClass.simpleName))
            }
        }
        return out
    }

    /** Build the descriptor for one camera without opening it. */
    fun describe(chars: CameraCharacteristics, cameraIds: List<String> = emptyList()): JSONObject {
        val out = JSONObject()
        val unsupported = JSONArray()
        out.put(
            "hardware_level",
            when (chars.get(CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL)) {
                CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LEGACY -> "LEGACY"
                CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LIMITED -> "LIMITED"
                CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_FULL -> "FULL"
                CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_3 -> "LEVEL_3"
                CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_EXTERNAL -> "EXTERNAL"
                else -> "UNKNOWN"
            },
        )
        out.put(
            "lens_facing",
            when (chars.get(CameraCharacteristics.LENS_FACING)) {
                CameraCharacteristics.LENS_FACING_BACK -> "back"
                CameraCharacteristics.LENS_FACING_FRONT -> "front"
                CameraCharacteristics.LENS_FACING_EXTERNAL -> "external"
                else -> "unknown"
            },
        )
        out.put("sensor_orientation", chars.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0)
        out.put(
            "capabilities",
            capabilityNames(chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES)),
        )
        out.put("physical_ids", stringArray(physicalIds(chars)))
        addControls(out, unsupported, chars, cameraIds)
        out.put("streams", streams(chars))
        out.put(
            "available_request_keys",
            keyArray(chars.availableCaptureRequestKeys),
        )
        out.put(
            "available_result_keys",
            keyArray(chars.availableCaptureResultKeys),
        )
        out.put(
            "available_characteristics_keys",
            keyArray(chars.keys),
        )
        out.put("unsupported", unsupported)
        return out
    }

    private fun addControls(
        out: JSONObject,
        unsupported: JSONArray,
        chars: CameraCharacteristics,
        cameraIds: List<String>,
    ) {
        addModes(out, unsupported, chars)
        addValues(out, unsupported, chars)
        TonemapControls.describe(out, unsupported, chars)
        OutputControls.describe(out, unsupported, chars, cameraIds)
    }

    private fun addModes(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        modes(out, unsupported, "control_mode", chars.get(CameraCharacteristics.CONTROL_AVAILABLE_MODES))
        modes(out, unsupported, "ae_mode", chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_MODES))
        modes(out, unsupported, "af_mode", chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES))
        modes(out, unsupported, "awb_mode", chars.get(CameraCharacteristics.CONTROL_AWB_AVAILABLE_MODES))
        modes(
            out,
            unsupported,
            "anti_banding",
            chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_ANTIBANDING_MODES),
        )
        modes(out, unsupported, "scene_mode", chars.get(CameraCharacteristics.CONTROL_AVAILABLE_SCENE_MODES))
        modes(out, unsupported, "effect", chars.get(CameraCharacteristics.CONTROL_AVAILABLE_EFFECTS))
        modes(
            out,
            unsupported,
            "noise_reduction",
            chars.get(CameraCharacteristics.NOISE_REDUCTION_AVAILABLE_NOISE_REDUCTION_MODES),
        )
        modes(out, unsupported, "edge_enhancement", chars.get(CameraCharacteristics.EDGE_AVAILABLE_EDGE_MODES))
        modes(out, unsupported, "hot_pixel", chars.get(CameraCharacteristics.HOT_PIXEL_AVAILABLE_HOT_PIXEL_MODES))
        modes(out, unsupported, "tonemap", chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES))
        modes(
            out,
            unsupported,
            "test_pattern",
            chars.get(CameraCharacteristics.SENSOR_AVAILABLE_TEST_PATTERN_MODES),
        )
        modes(
            out,
            unsupported,
            "aberration_correction",
            chars.get(CameraCharacteristics.COLOR_CORRECTION_AVAILABLE_ABERRATION_MODES),
        )
        modes(
            out,
            unsupported,
            "video_stabilization",
            chars.get(CameraCharacteristics.CONTROL_AVAILABLE_VIDEO_STABILIZATION_MODES),
        )
        modes(
            out,
            unsupported,
            "optical_stabilization",
            chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_OPTICAL_STABILIZATION),
        )
        modes(out, unsupported, "lens_shading", chars.get(CameraCharacteristics.SHADING_AVAILABLE_MODES))
        modes(
            out,
            unsupported,
            "face_detect",
            chars.get(CameraCharacteristics.STATISTICS_INFO_AVAILABLE_FACE_DETECT_MODES),
        )
    }

    private fun addValues(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val flashAvailable = chars.get(CameraCharacteristics.FLASH_INFO_AVAILABLE) ?: false
        lock(out, unsupported, "ae_lock", chars.get(CameraCharacteristics.CONTROL_AE_LOCK_AVAILABLE))
        lock(out, unsupported, "awb_lock", chars.get(CameraCharacteristics.CONTROL_AWB_LOCK_AVAILABLE))
        flashControls(out, unsupported, flashAvailable)
        intRange(out, unsupported, "iso", chars.get(CameraCharacteristics.SENSOR_INFO_SENSITIVITY_RANGE))
        longRange(
            out,
            unsupported,
            "exposure_time_ns",
            chars.get(CameraCharacteristics.SENSOR_INFO_EXPOSURE_TIME_RANGE),
        )
        frameDuration(
            out,
            unsupported,
            chars.get(CameraCharacteristics.SENSOR_INFO_MAX_FRAME_DURATION),
        )
        aeCompensation(
            out,
            unsupported,
            chars.get(CameraCharacteristics.CONTROL_AE_COMPENSATION_RANGE),
            chars.get(CameraCharacteristics.CONTROL_AE_COMPENSATION_STEP),
        )
        aeRegions(out, unsupported, chars)
        aeFpsRange(out, unsupported, chars)
        val aeModes = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_MODES)
        if (aeModes == null || aeModes.none { it != CaptureRequest.CONTROL_AE_MODE_OFF }) {
            unsupported.put(unsupportedEntry("ae_precapture", NO_AE_REASON))
        } else {
            out.put("ae_precapture", JSONObject().put("supported", true))
        }
        focusControls(out, unsupported, chars)
        ColorControls.describe(out, unsupported, chars)
        zoom(out, unsupported, chars)
        LensControls.describe(out, unsupported, chars)
    }

    /** AF modes, regions, trigger/lock and manual focus distance (M4-T05). */
    private fun focusControls(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val modes = chars.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
        val available = modes != null && modes.any { it != CaptureRequest.CONTROL_AF_MODE_OFF }
        if (!available) {
            unsupported.put(unsupportedEntry("af_regions", NO_AF_REASON))
            unsupported.put(unsupportedEntry("af_trigger", NO_AF_REASON))
            unsupported.put(unsupportedEntry("af_lock", NO_AF_REASON))
        } else {
            out.put("af_trigger", JSONObject().put("supported", true))
            out.put("af_lock", JSONObject().put("supported", true))
            val regions = chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AF) ?: 0
            if (regions > 0) {
                out.put("af_regions", JSONObject().put("supported", true).put("max_regions", regions))
            } else {
                unsupported.put(unsupportedEntry("af_regions", NO_AF_REGIONS_REASON))
            }
        }
        val minimum = chars.get(CameraCharacteristics.LENS_INFO_MINIMUM_FOCUS_DISTANCE)
        if (minimum != null && minimum > 0f) {
            out.put(
                "focus_distance",
                JSONObject()
                    .put("supported", true)
                    .put("min", 0.0)
                    .put("max", minimum.toDouble())
                    .put("step", CONTINUOUS_STEP),
            )
        } else {
            unsupported.put(unsupportedEntry("focus_distance", NO_FOCUS_DISTANCE_REASON))
        }
    }

    private fun aeRegions(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val maximum = chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AE) ?: 0
        if (maximum <= 0) {
            unsupported.put(unsupportedEntry("ae_regions", NO_AE_REGIONS_REASON))
            return
        }
        out.put("ae_regions", JSONObject().put("supported", true).put("max_regions", maximum))
    }

    private fun aeFpsRange(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val ranges = chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
        if (ranges == null || ranges.isEmpty()) {
            unsupported.put(unsupportedEntry("ae_fps_range", NO_REASON))
            return
        }
        val array = JSONArray()
        ranges.forEach { range -> array.put(JSONArray().put(range.lower).put(range.upper)) }
        out.put("ae_fps_range", JSONObject().put("supported", true).put("ranges", array))
    }

    private fun modes(out: JSONObject, unsupported: JSONArray, name: String, values: IntArray?) {
        if (values == null) {
            unsupported.put(unsupportedEntry(name, NO_REASON))
            return
        }
        val array = JSONArray()
        values.forEach { array.put(it) }
        out.put(name, JSONObject().put("supported", values.isNotEmpty()).put("modes", array))
    }

    private fun lock(out: JSONObject, unsupported: JSONArray, name: String, available: Boolean?) {
        if (available == null) {
            unsupported.put(unsupportedEntry(name, NO_REASON))
            return
        }
        out.put(name, JSONObject().put("supported", available))
    }

    private fun flashControls(out: JSONObject, unsupported: JSONArray, available: Boolean) {
        if (!available) {
            unsupported.put(unsupportedEntry("flash_mode", NO_FLASH_REASON))
            unsupported.put(unsupportedEntry("torch", NO_FLASH_REASON))
            out.put("flash_mode", JSONObject().put("supported", false))
            out.put("torch", JSONObject().put("supported", false))
            return
        }
        val modes =
            JSONArray()
                .put(CaptureRequest.FLASH_MODE_OFF)
                .put(CaptureRequest.FLASH_MODE_SINGLE)
                .put(CaptureRequest.FLASH_MODE_TORCH)
        out.put("flash_mode", JSONObject().put("supported", true).put("modes", modes))
        out.put("torch", JSONObject().put("supported", true))
    }

    private fun intRange(out: JSONObject, unsupported: JSONArray, name: String, range: Range<Int>?) {
        if (range == null) {
            unsupported.put(unsupportedEntry(name, NO_REASON))
            return
        }
        out.put(
            name,
            JSONObject()
                .put("supported", true)
                .put("min", range.lower)
                .put("max", range.upper)
                .put("step", 1),
        )
    }

    private fun longRange(out: JSONObject, unsupported: JSONArray, name: String, range: Range<Long>?) {
        if (range == null) {
            unsupported.put(unsupportedEntry(name, NO_REASON))
            return
        }
        out.put(
            name,
            JSONObject()
                .put("supported", true)
                .put("min", range.lower)
                .put("max", range.upper)
                .put("step", 1),
        )
    }

    private fun frameDuration(out: JSONObject, unsupported: JSONArray, maximum: Long?) {
        if (maximum == null) {
            unsupported.put(unsupportedEntry("frame_duration_ns", NO_REASON))
            return
        }
        out.put(
            "frame_duration_ns",
            JSONObject()
                .put("supported", true)
                .put("min", 0)
                .put("max", maximum)
                .put("step", 1),
        )
    }

    private fun aeCompensation(out: JSONObject, unsupported: JSONArray, range: Range<Int>?, step: Rational?) {
        if (range == null) {
            unsupported.put(unsupportedEntry("ae_compensation", NO_REASON))
            return
        }
        out.put(
            "ae_compensation",
            JSONObject()
                .put("supported", true)
                .put("min", range.lower)
                .put("max", range.upper)
                // The wire value is a step count, so the increment is always 1;
                // `ev_step` reports the exposure change each step represents.
                .put("step", 1)
                .put("ev_step", step?.toDouble() ?: CONTINUOUS_STEP),
        )
    }

    private fun zoom(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val range = chars.get(CameraCharacteristics.CONTROL_ZOOM_RATIO_RANGE)
            if (range != null) {
                out.put(
                    "zoom_ratio",
                    JSONObject()
                        .put("supported", true)
                        .put("min", range.lower.toDouble())
                        .put("max", range.upper.toDouble())
                        .put("step", CONTINUOUS_STEP)
                        .put("legacy_crop_region", false),
                )
                return
            }
        }
        val maximum = chars.get(CameraCharacteristics.SCALER_AVAILABLE_MAX_DIGITAL_ZOOM) ?: MIN_ZOOM_RATIO
        if (maximum > MIN_ZOOM_RATIO) {
            out.put(
                "zoom_ratio",
                JSONObject()
                    .put("supported", true)
                    .put("min", MIN_ZOOM_RATIO.toDouble())
                    .put("max", maximum.toDouble())
                    .put("step", CONTINUOUS_STEP)
                    .put("legacy_crop_region", true),
            )
        } else {
            unsupported.put(unsupportedEntry("zoom_ratio", NO_ZOOM_REASON))
        }
    }

    private fun streams(chars: CameraCharacteristics): JSONObject {
        val out = JSONObject()
        val map = chars.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        if (map == null) {
            return out.put("available", false)
        }
        out.put("available", true)
        val formats = JSONObject()
        for (format in OUTPUT_FORMATS) {
            val sizes = map.getOutputSizes(format) ?: continue
            formats.put(formatName(format), sizesArray(sizes))
        }
        out.put("formats", formats)
        val fps = JSONArray()
        chars.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)?.forEach { range ->
            fps.put(JSONArray().put(range.lower).put(range.upper))
        }
        out.put("fps_ranges", fps)
        val highSpeed = JSONObject()
        val highSpeedSizes = map.highSpeedVideoSizes
        highSpeed.put("sizes", sizesArray(highSpeedSizes))
        val highSpeedRanges = JSONArray()
        for (size in highSpeedSizes) {
            val ranges = JSONArray()
            map.getHighSpeedVideoFpsRangesFor(size).forEach { range ->
                ranges.put(JSONArray().put(range.lower).put(range.upper))
            }
            highSpeedRanges.put(
                JSONObject()
                    .put("width", size.width)
                    .put("height", size.height)
                    .put("fps_ranges", ranges),
            )
        }
        highSpeed.put("fps_ranges", highSpeedRanges)
        out.put("high_speed_video", highSpeed)
        return out
    }

    private fun sizesArray(sizes: Array<Size>): JSONArray {
        val out = JSONArray()
        sizes.sortedByDescending { it.width * it.height }.forEach { size ->
            out.put(JSONObject().put("width", size.width).put("height", size.height))
        }
        return out
    }

    private fun formatName(format: Int): String =
        when (format) {
            ImageFormat.YUV_420_888 -> "YUV_420_888"
            ImageFormat.JPEG -> "JPEG"
            ImageFormat.PRIVATE -> "PRIVATE"
            ImageFormat.RAW_SENSOR -> "RAW_SENSOR"
            else -> "FORMAT_$format"
        }

    private fun keyArray(keys: Collection<*>): JSONArray {
        val out = JSONArray()
        keys.sortedBy { keyName(it) }.forEach { key ->
            val name = keyName(key)
            out.put(JSONObject().put("name", name).put("type", KEY_TYPES[name] ?: UNKNOWN_TYPE))
        }
        return out
    }

    /**
     * Android's public `Key` API does not expose its generic type, so derive it
     * from the public static key fields of the SDK holder classes. Vendor keys
     * without a public declaration are reported as `unknown`.
     */
    private fun buildKeyTypes(): Map<String, String> {
        val out = HashMap<String, String>()
        for (holder in KEY_HOLDERS) {
            for (field in holder.declaredFields) {
                val entry = keyTypeEntry(field) ?: continue
                out.putIfAbsent(entry.first, entry.second)
            }
        }
        return out
    }

    private fun keyTypeEntry(field: Field): Pair<String, String>? {
        if (!Modifier.isPublic(field.modifiers) || !Modifier.isStatic(field.modifiers)) return null
        if (KEY_CLASSES.none { it.isAssignableFrom(field.type) }) return null
        val generic = field.genericType as? ParameterizedType ?: return null
        val key = runCatching { field.get(null) }.getOrNull() ?: return null
        val argument = generic.actualTypeArguments.firstOrNull() ?: return null
        val typeName =
            when (argument) {
                is Class<*> -> argument.name
                is ParameterizedType -> (argument.rawType as Class<*>).name
                else -> argument.typeName
            }
        return keyName(key) to typeName
    }

    private fun keyName(key: Any?): String =
        when (key) {
            is CameraCharacteristics.Key<*> -> key.name
            is CaptureRequest.Key<*> -> key.name
            is CaptureResult.Key<*> -> key.name
            else -> key?.toString() ?: UNKNOWN_TYPE
        }

    private fun capabilityNames(values: IntArray?): JSONArray {
        val out = JSONArray()
        values?.forEach { out.put(CAPABILITY_NAMES[it] ?: "CAPABILITY_$it") }
        return out
    }

    private fun physicalIds(chars: CameraCharacteristics): List<String> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            chars.physicalCameraIds.sorted()
        } else {
            emptyList()
        }

    private fun stringArray(values: List<String>): JSONArray {
        val out = JSONArray()
        values.forEach { out.put(it) }
        return out
    }

    private fun unsupportedEntry(control: String, reason: String): JSONObject =
        JSONObject().put("control", control).put("reason", reason)
}
