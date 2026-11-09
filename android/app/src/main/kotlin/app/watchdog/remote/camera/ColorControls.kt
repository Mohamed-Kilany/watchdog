package app.watchdog.remote.camera

import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.ColorSpaceTransform
import android.hardware.camera2.params.RggbChannelVector
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.round

/**
 * M4-T06 white-balance primitives: descriptor entries, validation, Camera2
 * mapping and readback for manual RGGB gains and the colour-correction matrix.
 *
 * Both primitives only work with `COLOR_CORRECTION_MODE = TRANSFORM_MATRIX`, so
 * [applyGains] and [applyTransform] set that mode in the same request. AWB
 * regions reuse the normalized metering-region validation in [ControlStateStore].
 */
internal object ColorControls {
    const val GAINS_MIN = 0.0
    const val GAINS_MAX = 8.0
    const val TRANSFORM_MIN = -4.0
    const val TRANSFORM_MAX = 4.0
    private const val DEFAULT_GAIN = 1.0
    private const val MATRIX_SIZE = 9
    private const val RATIONAL_SCALE = 10_000
    private const val NO_AWB_REGIONS_REASON = "no AWB metering regions"
    private const val NO_MANUAL_COLOR_REASON = "manual color correction is not available"

    private val GAIN_COMPONENTS = listOf("r", "g_even", "g_odd", "b")

    /** Add the T06 control entries (or `unsupported` reasons) to a descriptor. */
    fun describe(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        awbRegions(out, unsupported, chars)
        val requestKeys = chars.availableCaptureRequestKeys
        val modeAvailable = hasRequestKey(requestKeys, CaptureRequest.COLOR_CORRECTION_MODE)
        if (modeAvailable && hasRequestKey(requestKeys, CaptureRequest.COLOR_CORRECTION_GAINS)) {
            val components = JSONArray()
            GAIN_COMPONENTS.forEach { components.put(it) }
            out.put(
                "awb_gains",
                JSONObject()
                    .put("supported", true)
                    .put("components", components)
                    .put("min", GAINS_MIN)
                    .put("max", GAINS_MAX)
                    .put("step", ControlValues.DEFAULT_STEP),
            )
        } else {
            unsupported.put(unsupportedEntry("awb_gains", NO_MANUAL_COLOR_REASON))
        }
        if (modeAvailable && hasRequestKey(requestKeys, CaptureRequest.COLOR_CORRECTION_TRANSFORM)) {
            out.put(
                "color_transform",
                JSONObject()
                    .put("supported", true)
                    .put("matrix_size", MATRIX_SIZE)
                    .put("min", TRANSFORM_MIN)
                    .put("max", TRANSFORM_MAX)
                    .put("step", ControlValues.DEFAULT_STEP),
            )
        } else {
            unsupported.put(unsupportedEntry("color_transform", NO_MANUAL_COLOR_REASON))
        }
    }

    private fun awbRegions(out: JSONObject, unsupported: JSONArray, chars: CameraCharacteristics) {
        val maximum = chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AWB) ?: 0
        if (maximum <= 0) {
            unsupported.put(unsupportedEntry("awb_regions", NO_AWB_REGIONS_REASON))
            return
        }
        out.put("awb_regions", JSONObject().put("supported", true).put("max_regions", maximum))
    }

    private fun hasRequestKey(keys: List<CaptureRequest.Key<*>>, key: CaptureRequest.Key<*>): Boolean =
        keys.any { it.name == key.name }

    /** Validate an RGGB gain vector against the descriptor's per-component range. */
    fun validateGains(spec: JSONObject, value: Any?): ControlValidation {
        val gains =
            value as? JSONObject
                ?: return invalid("awb_gains", "expects an object with r, g_even, g_odd and b")
        val min = spec.optDouble("min", GAINS_MIN)
        val max = spec.optDouble("max", GAINS_MAX)
        val out = JSONObject()
        for (component in GAIN_COMPONENTS) {
            val number =
                ControlValues.asDouble(gains.opt(component))
                    ?: return invalid("awb_gains", "component '$component' expects a number")
            if (!number.isFinite() || number < min || number > max) {
                return outOfRange("awb_gains.$component=$number outside [$min, $max]")
            }
            out.put(component, ControlValues.roundToMillis(number))
        }
        return ControlValidation(true, applied = out)
    }

    /** Validate a row-major 3x3 matrix of finite numbers. */
    fun validateTransform(spec: JSONObject, value: Any?): ControlValidation {
        val size = spec.optInt("matrix_size", MATRIX_SIZE)
        val array =
            value as? JSONArray
                ?: return invalid("color_transform", "expects an array of $size numbers")
        if (array.length() != size) {
            return invalid("color_transform", "expects exactly $size numbers")
        }
        val min = spec.optDouble("min", TRANSFORM_MIN)
        val max = spec.optDouble("max", TRANSFORM_MAX)
        val out = JSONArray()
        for (index in 0 until size) {
            val number =
                ControlValues.asDouble(array.opt(index))
                    ?: return invalid("color_transform[$index]", "expects a number")
            if (!number.isFinite() || number < min || number > max) {
                return outOfRange("color_transform[$index]=$number outside [$min, $max]")
            }
            out.put(ControlValues.roundToMillis(number))
        }
        return ControlValidation(true, applied = out)
    }

    /** Apply an RGGB gain vector and switch colour correction to the matrix mode. */
    fun applyGains(builder: CaptureRequest.Builder, value: Any) {
        val gains = value as? JSONObject ?: return
        builder.set(
            CaptureRequest.COLOR_CORRECTION_GAINS,
            RggbChannelVector(
                gains.optDouble("r", DEFAULT_GAIN).toFloat(),
                gains.optDouble("g_even", DEFAULT_GAIN).toFloat(),
                gains.optDouble("g_odd", DEFAULT_GAIN).toFloat(),
                gains.optDouble("b", DEFAULT_GAIN).toFloat(),
            ),
        )
        builder.set(
            CaptureRequest.COLOR_CORRECTION_MODE,
            CaptureRequest.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX,
        )
    }

    /** Apply a row-major 3x3 matrix as rationals and switch to matrix mode. */
    fun applyTransform(builder: CaptureRequest.Builder, value: Any) {
        val array = value as? JSONArray ?: return
        if (array.length() != MATRIX_SIZE) {
            return
        }
        val elements = IntArray(MATRIX_SIZE * 2)
        for (index in 0 until MATRIX_SIZE) {
            val (numerator, denominator) = rational(array.optDouble(index, 0.0))
            elements[index * 2] = numerator
            elements[index * 2 + 1] = denominator
        }
        builder.set(CaptureRequest.COLOR_CORRECTION_TRANSFORM, ColorSpaceTransform(elements))
        builder.set(
            CaptureRequest.COLOR_CORRECTION_MODE,
            CaptureRequest.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX,
        )
    }

    /** Read back the gains and colour matrix reported by the device. */
    fun extract(result: TotalCaptureResult, out: MutableMap<String, Any>) {
        result.get(CaptureResult.COLOR_CORRECTION_GAINS)?.let { gains ->
            out["awb_gains"] =
                JSONObject()
                    .put("r", gains.red.toDouble())
                    .put("g_even", gains.greenEven.toDouble())
                    .put("g_odd", gains.greenOdd.toDouble())
                    .put("b", gains.blue.toDouble())
        }
        result.get(CaptureResult.COLOR_CORRECTION_TRANSFORM)?.let { transform ->
            out["color_transform"] = transformJson(transform)
        }
    }

    private fun transformJson(transform: ColorSpaceTransform): JSONArray {
        val elements = IntArray(MATRIX_SIZE * 2)
        transform.copyElements(elements, 0)
        val out = JSONArray()
        for (index in 0 until MATRIX_SIZE) {
            val denominator = elements[index * 2 + 1]
            val value =
                if (denominator == 0) {
                    0.0
                } else {
                    elements[index * 2].toDouble() / denominator
                }
            out.put(ControlValues.roundToMillis(value))
        }
        return out
    }

    /** Convert a bounded matrix element to a reduced rational pair. */
    private fun rational(value: Double): Pair<Int, Int> {
        var numerator = round(value * RATIONAL_SCALE).toInt()
        if (numerator == 0) {
            return 0 to 1
        }
        var denominator = RATIONAL_SCALE
        val divisor = gcd(abs(numerator), denominator)
        numerator /= divisor
        denominator /= divisor
        return numerator to denominator
    }

    private fun gcd(first: Int, second: Int): Int = if (second == 0) first else gcd(second, first % second)

    private fun invalid(control: String, message: String): ControlValidation =
        ControlValidation(
            false,
            errorCode = "INVALID_VALUE",
            message = "control '$control' $message",
        )

    private fun outOfRange(message: String): ControlValidation =
        ControlValidation(false, errorCode = "OUT_OF_RANGE", message = message)

    private fun unsupportedEntry(control: String, reason: String): JSONObject =
        JSONObject().put("control", control).put("reason", reason)
}
