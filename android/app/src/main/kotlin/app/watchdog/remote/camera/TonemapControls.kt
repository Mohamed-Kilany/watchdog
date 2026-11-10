package app.watchdog.remote.camera

import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.TonemapCurve
import org.json.JSONArray
import org.json.JSONObject

/**
 * M4-T08 tone mapping: contrast curve, gamma value and preset curve. Each
 * parameter is advertised only when the camera lists the matching
 * `TONEMAP_MODE`; the parameter control also implies that mode in the same
 * batch so the request is always valid.
 */
internal object TonemapControls {
    private const val MODE_CONTRAST_CURVE = CaptureRequest.TONEMAP_MODE_CONTRAST_CURVE
    private const val MODE_GAMMA_VALUE = CaptureRequest.TONEMAP_MODE_GAMMA_VALUE
    private const val MODE_PRESET_CURVE = CaptureRequest.TONEMAP_MODE_PRESET_CURVE
    private const val PRESET_SRGB = CaptureRequest.TONEMAP_PRESET_CURVE_SRGB
    private const val PRESET_REC709 = CaptureRequest.TONEMAP_PRESET_CURVE_REC709
    private const val MIN_GAMMA = 0.1
    private const val MAX_GAMMA = 5.0
    private const val GAMMA_STEP = 0.01
    private const val DEFAULT_GAMMA = 1.0
    private const val POINT_SIZE = 2
    private const val MIN_POINTS = 2
    private const val DEFAULT_MAX_POINTS = 2
    private const val UNIT_MAX = 1.0
    private const val NO_CURVE_REASON = "contrast curve is not available"
    private const val NO_GAMMA_REASON = "gamma value is not available"
    private const val NO_PRESET_REASON = "preset curve is not available"
    private val CHANNELS = arrayOf("red", "green", "blue")

    /** Add the tonemap parameter controls supported by this camera. */
    fun describe(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val modes = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES)
        describeCurve(out, unsupported, chars, modes)
        describeGamma(out, unsupported, modes)
        describePreset(out, unsupported, modes)
    }

    private fun describeCurve(
        out: JSONObject,
        unsupported: JSONArray,
        chars: CameraCharacteristics,
        modes: IntArray?,
    ) {
        if (modes == null || MODE_CONTRAST_CURVE !in modes) {
            unsupported.put(entry("tonemap_curve", NO_CURVE_REASON))
            return
        }
        val points = chars.get(CameraCharacteristics.TONEMAP_MAX_CURVE_POINTS) ?: DEFAULT_MAX_POINTS
        out.put(
            "tonemap_curve",
            JSONObject().put("supported", true).put("max_points", points.coerceAtLeast(MIN_POINTS)),
        )
    }

    private fun describeGamma(out: JSONObject, unsupported: JSONArray, modes: IntArray?) {
        if (modes == null || MODE_GAMMA_VALUE !in modes) {
            unsupported.put(entry("tonemap_gamma", NO_GAMMA_REASON))
            return
        }
        out.put(
            "tonemap_gamma",
            JSONObject()
                .put("supported", true)
                .put("min", MIN_GAMMA)
                .put("max", MAX_GAMMA)
                .put("step", GAMMA_STEP),
        )
    }

    private fun describePreset(out: JSONObject, unsupported: JSONArray, modes: IntArray?) {
        if (modes == null || MODE_PRESET_CURVE !in modes) {
            unsupported.put(entry("tonemap_preset_curve", NO_PRESET_REASON))
            return
        }
        out.put(
            "tonemap_preset_curve",
            JSONObject()
                .put("supported", true)
                .put("modes", JSONArray().put(PRESET_SRGB).put(PRESET_REC709)),
        )
    }

    /**
     * Validate a contrast curve: `{"red":[in,out,…],"green":[…],"blue":[…]}`.
     * Each channel is a flat list of points, at least two and at most
     * `max_points`, with inputs strictly increasing and all values in `[0,1]`.
     */
    fun validateCurve(spec: JSONObject, value: Any?): ControlValidation {
        val curve =
            value as? JSONObject
                ?: return invalid("tonemap_curve expects an object with red, green and blue arrays")
        val maxPoints = spec.optInt("max_points", DEFAULT_MAX_POINTS).coerceAtLeast(MIN_POINTS)
        val canonical = JSONObject()
        for (channel in CHANNELS) {
            val points =
                curve.optJSONArray(channel)
                    ?: return invalid("tonemap_curve needs a '$channel' array")
            val parsed = parseChannel(channel, points, maxPoints)
            parsed.error?.let { return invalid(it) }
            canonical.put(channel, parsed.values)
        }
        return ControlValidation(true, applied = canonical)
    }

    private fun parseChannel(name: String, points: JSONArray, maxPoints: Int): ParsedChannel {
        val count = points.length()
        if (count < MIN_POINTS * POINT_SIZE || count % POINT_SIZE != 0) {
            return ParsedChannel(error = "'$name' needs at least 2 [in,out] points")
        }
        if (count / POINT_SIZE > maxPoints) {
            return ParsedChannel(error = "'$name' allows at most $maxPoints points")
        }
        val values = JSONArray()
        var previous = -1.0
        var index = 0
        while (index < count) {
            val input = points.optDouble(index, Double.NaN)
            val output = points.optDouble(index + 1, Double.NaN)
            if (!inUnit(input) || !inUnit(output)) {
                return ParsedChannel(error = "'$name[$index]' must be an [in,out] pair in [0,1]")
            }
            if (input <= previous) {
                return ParsedChannel(error = "'$name' inputs must be strictly increasing")
            }
            previous = input
            values.put(ControlValues.roundToMillis(input)).put(ControlValues.roundToMillis(output))
            index += POINT_SIZE
        }
        return ParsedChannel(values = values)
    }

    private fun inUnit(value: Double): Boolean = value.isFinite() && value >= 0.0 && value <= UNIT_MAX

    /**
     * Apply the parameter that belongs to [mode] from the full requested map.
     * A missing parameter gets a neutral default so `tonemap` alone never sends
     * an incomplete request.
     */
    fun applyParameter(builder: CaptureRequest.Builder, mode: Int, requested: Map<String, Any>) {
        when (mode) {
            MODE_CONTRAST_CURVE ->
                builder.set(CaptureRequest.TONEMAP_CURVE, curve(requested["tonemap_curve"]))
            MODE_GAMMA_VALUE ->
                builder.set(CaptureRequest.TONEMAP_GAMMA, gamma(requested["tonemap_gamma"]))
            MODE_PRESET_CURVE ->
                builder.set(
                    CaptureRequest.TONEMAP_PRESET_CURVE,
                    preset(requested["tonemap_preset_curve"]),
                )
        }
    }

    private fun curve(value: Any?): TonemapCurve {
        val json = value as? JSONObject ?: return identityCurve()
        val red = floats(json.optJSONArray("red")) ?: return identityCurve()
        val green = floats(json.optJSONArray("green")) ?: return identityCurve()
        val blue = floats(json.optJSONArray("blue")) ?: return identityCurve()
        return TonemapCurve(red, green, blue)
    }

    private fun identityCurve(): TonemapCurve {
        val linear = floatArrayOf(0f, 0f, 1f, 1f)
        return TonemapCurve(linear, linear, linear)
    }

    private fun floats(points: JSONArray?): FloatArray? {
        if (points == null || points.length() < MIN_POINTS * POINT_SIZE) {
            return null
        }
        val out = FloatArray(points.length())
        for (index in out.indices) {
            out[index] = points.optDouble(index, 0.0).toFloat()
        }
        return out
    }

    private fun gamma(value: Any?): Float = (value as? Number)?.toFloat() ?: DEFAULT_GAMMA.toFloat()

    private fun preset(value: Any?): Int = (value as? Number)?.toInt() ?: PRESET_SRGB

    /** Read the tonemap parameter that the device actually used. */
    fun extract(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.TONEMAP_CURVE)?.let { out["tonemap_curve"] = curveJson(it) }
        result.get(CaptureResult.TONEMAP_GAMMA)?.let { out["tonemap_gamma"] = it.toDouble() }
        result.get(CaptureResult.TONEMAP_PRESET_CURVE)?.let { out["tonemap_preset_curve"] = it }
    }

    private fun curveJson(curve: TonemapCurve): JSONObject =
        JSONObject()
            .put("red", channelJson(curve, TonemapCurve.CHANNEL_RED))
            .put("green", channelJson(curve, TonemapCurve.CHANNEL_GREEN))
            .put("blue", channelJson(curve, TonemapCurve.CHANNEL_BLUE))

    private fun channelJson(curve: TonemapCurve, channel: Int): JSONArray {
        val values = FloatArray(curve.getPointCount(channel) * POINT_SIZE)
        curve.copyColorCurve(channel, values, 0)
        val out = JSONArray()
        values.forEach { out.put(ControlValues.roundToMillis(it.toDouble())) }
        return out
    }

    private fun invalid(message: String): ControlValidation =
        ControlValidation(false, errorCode = "INVALID_VALUE", message = message)

    private fun entry(control: String, reason: String): JSONObject =
        JSONObject().put("control", control).put("reason", reason)

    private data class ParsedChannel(val values: JSONArray? = null, val error: String? = null)
}
