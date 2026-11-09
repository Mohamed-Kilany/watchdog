package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import java.util.TreeMap
import kotlin.math.abs
import kotlin.math.round

/** Who last wrote a control value (`docs/protocol.md` §5.4). */
enum class ControlSource(val wire: String) {
    HOST("host"),
    DEVICE("device"),
    AUTO("auto"),
}

/** Result of validating one requested control. */
data class ControlValidation(
    val ok: Boolean,
    val applied: Any? = null,
    val errorCode: String? = null,
    val message: String? = null,
)

/**
 * Canonical control state (M4-T02): one entry per control the camera reports,
 * with the requested value, the value read back from `TotalCaptureResult`, the
 * lock state, support and the last writer. Also tracks device readouts such as
 * FPS and battery.
 *
 * Delta frames are produced by comparing each entry with the last value sent on
 * the current session, so a reconnect always starts from a full snapshot.
 * All public methods are synchronized: the camera thread, the command thread
 * and the state emitter may call them concurrently. Two writers are resolved by
 * arrival order (last writer wins) and the winner's source is recorded.
 */
class ControlStateStore(private val descriptor: JSONObject) {
    private data class Entry(
        var requested: Any? = null,
        var actual: Any? = null,
        var locked: Boolean? = null,
        val supported: Boolean,
        var source: ControlSource = ControlSource.DEVICE,
    )

    private data class StateHeader(
        val seq: Long,
        val timestampMs: Long,
        val cameraId: String,
        val droppedFrames: Long,
        val storageFreeMb: Long?,
    )

    private val entries = TreeMap<String, Entry>()
    private val lastSent = HashMap<String, String>()
    private var lastCameraId: String? = null

    init {
        val keys = descriptor.keys()
        while (keys.hasNext()) {
            val id = keys.next()
            val spec = descriptor.optJSONObject(id)
            if (spec != null && spec.has("supported")) {
                entries[id] = Entry(supported = spec.optBoolean("supported"))
            }
        }
        READOUTS.forEach { id ->
            entries[id] = Entry(supported = true, source = ControlSource.DEVICE)
        }
    }

    /** Whether the camera descriptor exposes [id] as supported. */
    @Synchronized
    fun isSupported(id: String): Boolean = entries[id]?.supported == true

    /**
     * Validate and record a host/device request. Returns a per-control error
     * instead of throwing; callers turn it into a `COMMAND_RESULT` entry.
     */
    @Synchronized
    fun setRequested(id: String, value: Any?, source: ControlSource): ControlValidation {
        val entry =
            entries[id]
                ?: return ControlValidation(
                    false,
                    errorCode = "UNKNOWN_CONTROL",
                    message = "control '$id' is not reported by this camera",
                )
        if (!entry.supported) {
            return ControlValidation(
                false,
                errorCode = "UNSUPPORTED",
                message = "control '$id' is not supported by this camera",
            )
        }
        val validation = validate(id, descriptor.optJSONObject(id), value)
        if (!validation.ok) {
            return validation
        }
        val applied = validation.applied
        entry.requested = applied
        entry.source = source
        if (id in LOCKS) {
            entry.locked = applied as? Boolean
        }
        return ControlValidation(true, applied = applied)
    }

    /** Record a value read from `TotalCaptureResult`; never changes `source`. */
    @Synchronized
    fun setActual(id: String, value: Any?) {
        val entry = entries[id] ?: return
        entry.actual = normalize(value)
    }

    /** Record a device readout (FPS, battery, …) as both actual and source. */
    @Synchronized
    fun setReadout(id: String, value: Any?) {
        val entry = entries[id] ?: return
        entry.actual = normalize(value)
        entry.source = ControlSource.DEVICE
    }

    /** Requested values to apply to the next repeating request, in id order. */
    @Synchronized
    fun requestedValues(): Map<String, Any> {
        val out = LinkedHashMap<String, Any>()
        entries.forEach { (id, entry) ->
            entry.requested?.let { out[id] = it }
        }
        return out
    }

    /** Forget the delta baseline; the next frame is a full snapshot. */
    @Synchronized
    fun resetBaseline() {
        lastSent.clear()
        lastCameraId = null
    }

    /** Full snapshot (`flags = 0`). Always marks every entry as sent. */
    @Synchronized
    fun fullState(
        seq: Long,
        timestampMs: Long,
        cameraId: String,
        droppedFrames: Long,
        storageFreeMb: Long?,
    ): JSONObject {
        val state = JSONObject()
        entries.forEach { (id, entry) ->
            val json = entryJson(entry)
            state.put(id, json)
            lastSent[id] = json.toString()
        }
        lastCameraId = cameraId
        return envelope(StateHeader(seq, timestampMs, cameraId, droppedFrames, storageFreeMb), state)
    }

    /**
     * Delta snapshot (`flags = 0x01`) or `null` when nothing changed. Entries
     * are diffed on their canonical JSON, so reordering or equivalent number
     * types never produce spurious updates.
     */
    @Synchronized
    fun deltaState(
        seq: Long,
        timestampMs: Long,
        cameraId: String,
        droppedFrames: Long,
        storageFreeMb: Long?,
    ): JSONObject? {
        val changed = JSONObject()
        var count = 0
        entries.forEach { (id, entry) ->
            val json = entryJson(entry)
            val canonical = json.toString()
            if (lastSent[id] != canonical) {
                changed.put(id, json)
                lastSent[id] = canonical
                count++
            }
        }
        val cameraChanged = cameraId != lastCameraId
        lastCameraId = cameraId
        if (count == 0 && !cameraChanged) {
            return null
        }
        return envelope(StateHeader(seq, timestampMs, cameraId, droppedFrames, storageFreeMb), changed)
    }

    private fun envelope(header: StateHeader, state: JSONObject): JSONObject {
        val out =
            JSONObject()
                .put("seq", header.seq)
                .put("timestamp_ms", header.timestampMs)
                .put("camera_id", header.cameraId)
                .put("state", state)
                .put("dropped_frames", header.droppedFrames)
        if (header.storageFreeMb != null) {
            out.put("storage_free_mb", header.storageFreeMb)
        }
        return out
    }

    private fun entryJson(entry: Entry): JSONObject {
        val json = JSONObject()
        entry.requested?.let { json.put("requested", it) }
        entry.actual?.let { json.put("actual", it) }
        entry.locked?.let { json.put("locked", it) }
        json.put("supported", entry.supported)
        json.put("source", entry.source.wire)
        return json
    }

    private fun validate(id: String, spec: JSONObject?, value: Any?): ControlValidation {
        if (value == null || value == JSONObject.NULL) {
            return ControlValidation(false, errorCode = "NULL_VALUE", message = "control '$id' needs a value")
        }
        if (spec == null) {
            return ControlValidation(true, applied = normalize(value))
        }
        if (spec.has("modes")) {
            return validateMode(id, spec.getJSONArray("modes"), value)
        }
        if (spec.has("min") && spec.has("max")) {
            return validateRange(id, spec, value)
        }
        if (id in BOOLEANS) {
            val bool =
                asBoolean(value)
                    ?: return ControlValidation(
                        false,
                        errorCode = "INVALID_VALUE",
                        message = "control '$id' expects a boolean",
                    )
            return ControlValidation(true, applied = bool)
        }
        return ControlValidation(true, applied = normalize(value))
    }

    private fun validateMode(id: String, modes: JSONArray, value: Any?): ControlValidation {
        val mode =
            asInt(value)
                ?: return ControlValidation(
                    false,
                    errorCode = "INVALID_VALUE",
                    message = "control '$id' expects an integer mode",
                )
        for (index in 0 until modes.length()) {
            if (modes.optInt(index, Int.MIN_VALUE) == mode) {
                return ControlValidation(true, applied = mode)
            }
        }
        return ControlValidation(
            false,
            errorCode = "OUT_OF_RANGE",
            message = "mode $mode is not available for '$id'",
        )
    }

    private fun validateRange(id: String, spec: JSONObject, value: Any?): ControlValidation {
        val number =
            asDouble(value)
                ?: return ControlValidation(
                    false,
                    errorCode = "INVALID_VALUE",
                    message = "control '$id' expects a number",
                )
        val min = spec.optDouble("min", Double.NEGATIVE_INFINITY)
        val max = spec.optDouble("max", Double.POSITIVE_INFINITY)
        if (number < min || number > max) {
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "$id=$value outside [$min, $max]",
            )
        }
        val step = spec.optDouble("step", DEFAULT_STEP)
        if (step > DEFAULT_STEP) {
            val steps = (number - min) / step
            if (abs(steps - round(steps)) > STEP_TOLERANCE) {
                return ControlValidation(
                    false,
                    errorCode = "STEP_MISMATCH",
                    message = "$id=$value is not a multiple of step $step from $min",
                )
            }
        }
        return ControlValidation(true, applied = normalizeRangeValue(id, number))
    }

    private fun normalizeRangeValue(id: String, number: Double): Any =
        when (id) {
            "zoom_ratio" -> roundToMillis(number)
            "exposure_time_ns", "frame_duration_ns" -> round(number).toLong()
            "ae_compensation" -> round(number).toInt()
            else -> if (number == round(number)) round(number).toLong() else roundToMillis(number)
        }

    private fun asInt(value: Any?): Int? =
        when (value) {
            is Int -> value
            is Long -> if (value in Int.MIN_VALUE..Int.MAX_VALUE) value.toInt() else null
            is Double -> if (value == round(value)) value.toInt() else null
            is Float -> if (value.toDouble() == round(value.toDouble())) value.toInt() else null
            is Number -> value.toInt()
            is String -> value.toIntOrNull()
            else -> null
        }

    private fun asDouble(value: Any?): Double? =
        when (value) {
            is Number -> value.toDouble()
            is String -> value.toDoubleOrNull()
            else -> null
        }

    private fun asBoolean(value: Any?): Boolean? =
        when (value) {
            is Boolean -> value
            is String -> value.toBooleanStrictOrNull()
            else -> null
        }

    private fun normalize(value: Any?): Any? =
        when (value) {
            is Int -> value.toLong()
            is Float -> roundToMillis(value.toDouble())
            is Double -> roundToMillis(value)
            is JSONObject, is JSONArray -> value
            else -> value
        }

    private fun roundToMillis(value: Double): Double = round(value * MILLIS) / MILLIS

    private companion object {
        const val MILLIS = 1000.0
        const val STEP_TOLERANCE = 1e-6
        const val DEFAULT_STEP = 0.0
        val LOCKS = setOf("ae_lock", "awb_lock")
        val BOOLEANS = setOf("ae_lock", "awb_lock", "torch")
        val READOUTS =
            setOf(
                "fps_actual",
                "recording",
                "battery_percent",
                "ae_state",
                "af_state",
                "awb_state",
                "focus_distance_diopters",
                "awb_gains",
                "zoom_actual",
            )
    }
}
