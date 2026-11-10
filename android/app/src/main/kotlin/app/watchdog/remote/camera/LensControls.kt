package app.watchdog.remote.camera

import android.hardware.camera2.CameraCharacteristics
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

/**
 * M4-T07 lens descriptor: fixed lens metadata (focal lengths, apertures, ND
 * filter choices), the optical zoom span of the lens set and distortion
 * correction modes. Kept separate from [CapabilityDescriptor] so the lens
 * schema stays with the other lens controls.
 */
internal object LensControls {
    private const val MIN_ZOOM_RATIO = 1.0
    private const val NO_DISTORTION_REASON = "no distortion correction"
    private const val NO_FILTER_DENSITY_REASON = "no adjustable neutral-density filter"

    /** Add the `lens` object and the lens-related controls to a descriptor. */
    fun describe(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        out.put("lens", lens(chars))
        filterDensity(out, unsupported, chars)
        distortionCorrection(out, unsupported, chars)
    }

    private fun lens(chars: CameraCharacteristics): JSONObject {
        val out = JSONObject()
        val focalLengths = chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)
        out.put("focal_lengths", floatArray(focalLengths))
        out.put("apertures", floatArray(chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_APERTURES)))
        out.put(
            "filter_densities",
            floatArray(chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FILTER_DENSITIES)),
        )
        out.put(
            "focus_distance_calibration",
            focusCalibration(chars.get(CameraCharacteristics.LENS_INFO_FOCUS_DISTANCE_CALIBRATION)),
        )
        out.put("optical_zoom", opticalZoom(focalLengths))
        out.put(
            "hyperfocal_distance",
            chars.get(CameraCharacteristics.LENS_INFO_HYPERFOCAL_DISTANCE) ?: JSONObject.NULL,
        )
        out.put(
            "minimum_focus_distance",
            chars.get(CameraCharacteristics.LENS_INFO_MINIMUM_FOCUS_DISTANCE) ?: JSONObject.NULL,
        )
        return out
    }

    /**
     * Optical zoom span of the lens set relative to its widest lens. A single
     * lens reports `[1.0, 1.0]`; logical multi-camera devices list the physical
     * lenses separately in `physical_ids` (selected in M4-T09).
     */
    private fun opticalZoom(focalLengths: FloatArray?): JSONObject {
        val usable = focalLengths?.filter { it > 0f }
        val minimum = usable?.minOrNull()
        val maximum = usable?.maxOrNull()
        val span =
            if (minimum != null && maximum != null) {
                (maximum / minimum).toDouble()
            } else {
                MIN_ZOOM_RATIO
            }
        return JSONObject().put("min", MIN_ZOOM_RATIO).put("max", span)
    }

    private fun focusCalibration(value: Int?): String =
        when (value) {
            CameraCharacteristics.LENS_INFO_FOCUS_DISTANCE_CALIBRATION_UNCALIBRATED -> "uncalibrated"
            CameraCharacteristics.LENS_INFO_FOCUS_DISTANCE_CALIBRATION_APPROXIMATE -> "approximate"
            CameraCharacteristics.LENS_INFO_FOCUS_DISTANCE_CALIBRATION_CALIBRATED -> "calibrated"
            else -> "unknown"
        }

    /**
     * Neutral-density filter, only when the lens lists a real choice. A list
     * containing just `0.0` means the camera has no adjustable ND filter; the
     * actual density is still reported as the `lens_filter_density` readout.
     */
    private fun filterDensity(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val densities = chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FILTER_DENSITIES)
        val choices = densities?.filter { it > 0f }?.distinct()
        if (densities == null || choices.isNullOrEmpty()) {
            unsupported.put(unsupportedEntry("filter_density", NO_FILTER_DENSITY_REASON))
            return
        }
        val values = JSONArray()
        densities.distinct().sorted().forEach { values.put(it.toDouble()) }
        out.put("filter_density", JSONObject().put("supported", true).put("values", values))
    }

    /** Distortion correction is a public Camera2 control from Android 13 (API 33). */
    private fun distortionCorrection(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val modes =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                chars.get(CameraCharacteristics.DISTORTION_CORRECTION_AVAILABLE_MODES)
            } else {
                null
            }
        if (modes == null) {
            unsupported.put(unsupportedEntry("distortion_correction", NO_DISTORTION_REASON))
            return
        }
        val array = JSONArray()
        modes.forEach { array.put(it) }
        out.put(
            "distortion_correction",
            JSONObject().put("supported", modes.isNotEmpty()).put("modes", array),
        )
    }

    private fun floatArray(values: FloatArray?): JSONArray {
        val out = JSONArray()
        values?.forEach { out.put(it.toDouble()) }
        return out
    }

    private fun unsupportedEntry(control: String, reason: String): JSONObject =
        JSONObject().put("control", control).put("reason", reason)
}
