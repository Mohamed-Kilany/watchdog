package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.round

/**
 * Parsing and canonicalization helpers shared by the control state store.
 *
 * JSON numbers arrive as Int, Long, Double or String depending on the client,
 * so every helper accepts `Any?` and returns `null` for values it cannot read.
 * Keeping them outside [ControlStateStore] keeps the store focused on state
 * and validation.
 */
internal object ControlValues {
    const val MILLIS = 1000.0
    const val STEP_TOLERANCE = 1e-6
    const val DEFAULT_STEP = 0.0

    fun asInt(value: Any?): Int? =
        when (value) {
            is Int -> value
            is Long -> if (value in Int.MIN_VALUE..Int.MAX_VALUE) value.toInt() else null
            is Double -> if (value == round(value)) value.toInt() else null
            is Float -> if (value.toDouble() == round(value.toDouble())) value.toInt() else null
            is Number -> value.toInt()
            is String -> value.toIntOrNull()
            else -> null
        }

    fun asDouble(value: Any?): Double? =
        when (value) {
            is Number -> value.toDouble()
            is String -> value.toDoubleOrNull()
            else -> null
        }

    fun asBoolean(value: Any?): Boolean? =
        when (value) {
            is Boolean -> value
            is String -> value.toBooleanStrictOrNull()
            else -> null
        }

    fun normalize(value: Any?): Any? =
        when (value) {
            is Int -> value.toLong()
            is Float -> roundToMillis(value.toDouble())
            is Double -> roundToMillis(value)
            is JSONObject, is JSONArray -> value
            else -> value
        }

    /** Validate a discrete numeric choice (for example an ND filter density). */
    fun validateChoice(id: String, values: JSONArray, value: Any?): ControlValidation {
        val number =
            asDouble(value)
                ?: return ControlValidation(
                    false,
                    errorCode = "INVALID_VALUE",
                    message = "control '$id' expects a number",
                )
        if (!matchesChoice(values, number)) {
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "$id=$value is not one of the available values",
            )
        }
        return ControlValidation(true, applied = roundToMillis(number))
    }

    /** Whether [number] equals one of the discrete [values] within tolerance. */
    private fun matchesChoice(values: JSONArray, number: Double): Boolean {
        for (index in 0 until values.length()) {
            val candidate = values.optDouble(index, Double.NaN)
            if (abs(candidate - number) <= STEP_TOLERANCE) {
                return true
            }
        }
        return false
    }

    fun normalizeRangeValue(id: String, number: Double): Any =
        when (id) {
            "zoom_ratio" -> roundToMillis(number)
            "exposure_time_ns", "frame_duration_ns" -> round(number).toLong()
            "ae_compensation" -> round(number).toInt()
            else -> if (number == round(number)) round(number).toLong() else roundToMillis(number)
        }

    fun roundToMillis(value: Double): Double = round(value * MILLIS) / MILLIS
}
