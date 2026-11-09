package app.watchdog.remote.camera

import android.graphics.Rect
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.MeteringRectangle
import android.os.Build
import android.util.Range
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

/**
 * Mapping between the control ids published by [CapabilityDescriptor] and the
 * Camera2 request/result keys (M4-T02/T03, extended by M4-T04…M4-T09).
 *
 * Applying a batch builds one request, so the whole batch takes effect in the
 * next repeating request. Values are already validated against the descriptor
 * by [ControlStateStore]; unknown ids are ignored here.
 */
object CameraControls {
    private const val DEFAULT_METERING_WEIGHT = 1000

    /** Apply one validated control to a request builder. */
    fun applyTo(builder: CaptureRequest.Builder, chars: CameraCharacteristics, id: String, value: Any) {
        when (id) {
            "zoom_ratio" -> applyZoom(builder, chars, number(value))
            "ae_fps_range" -> applyFpsRange(builder, value)
            "ae_regions" -> applyRegions(builder, chars, value, CaptureRequest.CONTROL_AE_REGIONS)
            "af_regions" -> applyRegions(builder, chars, value, CaptureRequest.CONTROL_AF_REGIONS)
            "focus_distance" -> applyFocusDistance(builder, number(value))
            else -> applyValue(builder, id, value)
        }
    }

    /** Direct request values and modes that need no characteristics context. */
    private fun applyValue(builder: CaptureRequest.Builder, id: String, value: Any) {
        when (id) {
            "torch" -> applyTorch(builder, bool(value))
            "ae_lock" -> builder.set(CaptureRequest.CONTROL_AE_LOCK, bool(value))
            "awb_lock" -> builder.set(CaptureRequest.CONTROL_AWB_LOCK, bool(value))
            "af_lock" -> Unit
            "iso" -> builder.set(CaptureRequest.SENSOR_SENSITIVITY, int(value))
            "exposure_time_ns" -> builder.set(CaptureRequest.SENSOR_EXPOSURE_TIME, long(value))
            "frame_duration_ns" -> builder.set(CaptureRequest.SENSOR_FRAME_DURATION, long(value))
            "ae_compensation" -> builder.set(CaptureRequest.CONTROL_AE_EXPOSURE_COMPENSATION, int(value))
            "flash_mode" -> builder.set(CaptureRequest.FLASH_MODE, int(value))
            else -> applyMode(builder, id, value)
        }
    }

    /** Capture and scene modes, one Camera2 request key each. */
    private fun applyMode(builder: CaptureRequest.Builder, id: String, value: Any) {
        val key =
            when (id) {
                "control_mode" -> CaptureRequest.CONTROL_MODE
                "ae_mode" -> CaptureRequest.CONTROL_AE_MODE
                "af_mode" -> CaptureRequest.CONTROL_AF_MODE
                "awb_mode" -> CaptureRequest.CONTROL_AWB_MODE
                "anti_banding" -> CaptureRequest.CONTROL_AE_ANTIBANDING_MODE
                "scene_mode" -> CaptureRequest.CONTROL_SCENE_MODE
                "effect" -> CaptureRequest.CONTROL_EFFECT_MODE
                else -> null
            }
        if (key != null) {
            builder.set(key, int(value))
            return
        }
        applyProcessing(builder, id, value)
    }

    /** Per-frame processing modes (noise reduction, tone mapping, …). */
    private fun applyProcessing(builder: CaptureRequest.Builder, id: String, value: Any) {
        val key =
            when (id) {
                "noise_reduction" -> CaptureRequest.NOISE_REDUCTION_MODE
                "edge_enhancement" -> CaptureRequest.EDGE_MODE
                "hot_pixel" -> CaptureRequest.HOT_PIXEL_MODE
                "tonemap" -> CaptureRequest.TONEMAP_MODE
                "aberration_correction" -> CaptureRequest.COLOR_CORRECTION_ABERRATION_MODE
                "video_stabilization" -> CaptureRequest.CONTROL_VIDEO_STABILIZATION_MODE
                "optical_stabilization" -> CaptureRequest.LENS_OPTICAL_STABILIZATION_MODE
                "lens_shading" -> CaptureRequest.SHADING_MODE
                "face_detect" -> CaptureRequest.STATISTICS_FACE_DETECT_MODE
                else -> return
            }
        builder.set(key, int(value))
    }

    private fun applyTorch(builder: CaptureRequest.Builder, on: Boolean) {
        val mode =
            if (on) {
                CaptureRequest.FLASH_MODE_TORCH
            } else {
                CaptureRequest.FLASH_MODE_OFF
            }
        builder.set(CaptureRequest.FLASH_MODE, mode)
    }

    private fun applyFpsRange(builder: CaptureRequest.Builder, value: Any) {
        val pair = value as? JSONArray ?: return
        val min = pair.optInt(0, 0)
        val max = pair.optInt(1, 0)
        if (min > 0 && max >= min) {
            builder.set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, Range(min, max))
        }
    }

    private fun applyRegions(
        builder: CaptureRequest.Builder,
        chars: CameraCharacteristics,
        value: Any,
        key: CaptureRequest.Key<Array<MeteringRectangle>>,
    ) {
        val regions = value as? JSONArray ?: return
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return
        val rectangles = ArrayList<MeteringRectangle>(regions.length())
        for (index in 0 until regions.length()) {
            val region = regions.optJSONObject(index) ?: continue
            val x = region.optDouble("x", 0.0)
            val y = region.optDouble("y", 0.0)
            val w = region.optDouble("w", 0.0)
            val h = region.optDouble("h", 0.0)
            if (w > 0.0 && h > 0.0) {
                val left =
                    (active.left + x * active.width()).roundToInt().coerceIn(active.left, active.right - 1)
                val top =
                    (active.top + y * active.height()).roundToInt().coerceIn(active.top, active.bottom - 1)
                val width = (w * active.width()).roundToInt().coerceAtLeast(1).coerceAtMost(active.right - left)
                val height = (h * active.height()).roundToInt().coerceAtLeast(1).coerceAtMost(active.bottom - top)
                rectangles +=
                    MeteringRectangle(left, top, width, height, region.optInt("weight", DEFAULT_METERING_WEIGHT))
            }
        }
        if (rectangles.isNotEmpty()) {
            builder.set(key, rectangles.toTypedArray())
        }
    }

    /**
     * Manual focus distance in dioptres (0 = infinity). The service implies
     * `af_mode = OFF` in the same batch; setting the mode here as well would
     * make a later explicit AF mode impossible while the distance is requested.
     */
    private fun applyFocusDistance(builder: CaptureRequest.Builder, dioptres: Double) {
        builder.set(CaptureRequest.LENS_FOCUS_DISTANCE, dioptres.toFloat())
    }

    /**
     * Values read back from `TotalCaptureResult` (the truth shown in STATE).
     * Keys the device does not report are simply absent.
     */
    fun extract(result: TotalCaptureResult, chars: CameraCharacteristics): Map<String, Any> {
        val out = LinkedHashMap<String, Any>()
        extractExposure(result, out)
        extractModes(result, out)
        extractProcessing(result, out)
        extractCorrection(result, out)
        extractLocks(result, out)
        extractReadouts(result, out)
        zoomRatio(result, chars)?.let { out["zoom_actual"] = it }
        return out
    }

    private fun extractExposure(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.SENSOR_SENSITIVITY)?.let { out["iso"] = it }
        result.get(CaptureResult.SENSOR_EXPOSURE_TIME)?.let { out["exposure_time_ns"] = it }
        result.get(CaptureResult.SENSOR_FRAME_DURATION)?.let { out["frame_duration_ns"] = it }
    }

    private fun extractModes(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.CONTROL_AE_MODE)?.let { out["ae_mode"] = it }
        result.get(CaptureResult.CONTROL_AF_MODE)?.let { out["af_mode"] = it }
        result.get(CaptureResult.CONTROL_AWB_MODE)?.let { out["awb_mode"] = it }
        result.get(CaptureResult.CONTROL_MODE)?.let { out["control_mode"] = it }
    }

    private fun extractProcessing(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.CONTROL_AE_ANTIBANDING_MODE)?.let { out["anti_banding"] = it }
        result.get(CaptureResult.CONTROL_SCENE_MODE)?.let { out["scene_mode"] = it }
        result.get(CaptureResult.CONTROL_EFFECT_MODE)?.let { out["effect"] = it }
        result.get(CaptureResult.NOISE_REDUCTION_MODE)?.let { out["noise_reduction"] = it }
        result.get(CaptureResult.EDGE_MODE)?.let { out["edge_enhancement"] = it }
        result.get(CaptureResult.HOT_PIXEL_MODE)?.let { out["hot_pixel"] = it }
        result.get(CaptureResult.TONEMAP_MODE)?.let { out["tonemap"] = it }
    }

    private fun extractCorrection(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.COLOR_CORRECTION_ABERRATION_MODE)?.let { out["aberration_correction"] = it }
        result.get(CaptureResult.CONTROL_VIDEO_STABILIZATION_MODE)?.let { out["video_stabilization"] = it }
        result.get(CaptureResult.LENS_OPTICAL_STABILIZATION_MODE)?.let { out["optical_stabilization"] = it }
        result.get(CaptureResult.SHADING_MODE)?.let { out["lens_shading"] = it }
        result.get(CaptureResult.STATISTICS_FACE_DETECT_MODE)?.let { out["face_detect"] = it }
    }

    private fun extractLocks(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.CONTROL_AE_LOCK)?.let { out["ae_lock"] = it }
        result.get(CaptureResult.CONTROL_AWB_LOCK)?.let { out["awb_lock"] = it }
        result.get(CaptureResult.CONTROL_AF_STATE)?.let { state ->
            out["af_lock"] =
                state == CaptureResult.CONTROL_AF_STATE_FOCUSED_LOCKED ||
                state == CaptureResult.CONTROL_AF_STATE_NOT_FOCUSED_LOCKED
        }
        result.get(CaptureResult.CONTROL_AE_EXPOSURE_COMPENSATION)?.let { out["ae_compensation"] = it }
    }

    private fun extractReadouts(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.CONTROL_AE_STATE)?.let { out["ae_state"] = aeState(it) }
        result.get(CaptureResult.CONTROL_AF_STATE)?.let { out["af_state"] = afState(it) }
        result.get(CaptureResult.CONTROL_AWB_STATE)?.let { out["awb_state"] = awbState(it) }
        result.get(CaptureResult.LENS_FOCUS_DISTANCE)?.let {
            out["focus_distance"] = it.toDouble()
            out["focus_distance_diopters"] = it.toDouble()
        }
        result.get(CaptureResult.COLOR_CORRECTION_GAINS)?.let { gains ->
            out["awb_gains"] =
                JSONObject()
                    .put("r", gains.red.toDouble())
                    .put("g_even", gains.greenEven.toDouble())
                    .put("g_odd", gains.greenOdd.toDouble())
                    .put("b", gains.blue.toDouble())
        }
    }

    private fun applyZoom(builder: CaptureRequest.Builder, chars: CameraCharacteristics, ratio: Double) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            builder.set(CaptureRequest.CONTROL_ZOOM_RATIO, ratio.toFloat())
            return
        }
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return
        val width = (active.width() / ratio).roundToInt().coerceIn(1, active.width())
        val height = (active.height() / ratio).roundToInt().coerceIn(1, active.height())
        val left = active.left + (active.width() - width) / 2
        val top = active.top + (active.height() - height) / 2
        builder.set(CaptureRequest.SCALER_CROP_REGION, Rect(left, top, left + width, top + height))
    }

    private fun zoomRatio(result: TotalCaptureResult, chars: CameraCharacteristics): Double? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            result.get(CaptureResult.CONTROL_ZOOM_RATIO)?.let { return it.toDouble() }
        }
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return null
        val crop = result.get(CaptureResult.SCALER_CROP_REGION) ?: return null
        if (crop.width() <= 0) {
            return null
        }
        return active.width().toDouble() / crop.width()
    }

    private fun aeState(value: Int): String =
        when (value) {
            CaptureResult.CONTROL_AE_STATE_INACTIVE -> "inactive"
            CaptureResult.CONTROL_AE_STATE_SEARCHING -> "searching"
            CaptureResult.CONTROL_AE_STATE_CONVERGED -> "converged"
            CaptureResult.CONTROL_AE_STATE_LOCKED -> "locked"
            CaptureResult.CONTROL_AE_STATE_FLASH_REQUIRED -> "flash_required"
            CaptureResult.CONTROL_AE_STATE_PRECAPTURE -> "precapture"
            else -> "unknown"
        }

    private fun afState(value: Int): String =
        when (value) {
            CaptureResult.CONTROL_AF_STATE_INACTIVE -> "inactive"
            CaptureResult.CONTROL_AF_STATE_PASSIVE_SCAN -> "passive_scan"
            CaptureResult.CONTROL_AF_STATE_PASSIVE_FOCUSED -> "passive_focused"
            CaptureResult.CONTROL_AF_STATE_ACTIVE_SCAN -> "active_scan"
            CaptureResult.CONTROL_AF_STATE_FOCUSED_LOCKED -> "focused_locked"
            CaptureResult.CONTROL_AF_STATE_NOT_FOCUSED_LOCKED -> "not_focused_locked"
            CaptureResult.CONTROL_AF_STATE_PASSIVE_UNFOCUSED -> "passive_unfocused"
            else -> "unknown"
        }

    private fun awbState(value: Int): String =
        when (value) {
            CaptureResult.CONTROL_AWB_STATE_INACTIVE -> "inactive"
            CaptureResult.CONTROL_AWB_STATE_SEARCHING -> "searching"
            CaptureResult.CONTROL_AWB_STATE_CONVERGED -> "converged"
            CaptureResult.CONTROL_AWB_STATE_LOCKED -> "locked"
            else -> "unknown"
        }

    private fun int(value: Any): Int =
        when (value) {
            is Number -> value.toInt()
            else -> value.toString().toInt()
        }

    private fun long(value: Any): Long =
        when (value) {
            is Number -> value.toLong()
            else -> value.toString().toLong()
        }

    private fun number(value: Any): Double =
        when (value) {
            is Number -> value.toDouble()
            else -> value.toString().toDouble()
        }

    private fun bool(value: Any): Boolean =
        when (value) {
            is Boolean -> value
            else -> value.toString().toBooleanStrict()
        }
}
