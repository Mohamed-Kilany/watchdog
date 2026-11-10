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
        // Unsupported controls are listed separately; remembering them as
        // unsupported keeps requests rejected while readouts (e.g. awb_gains)
        // can still report actual values.
        val unsupported = descriptor.optJSONArray("unsupported")
        if (unsupported != null) {
            for (index in 0 until unsupported.length()) {
                val id = unsupported.optJSONObject(index)?.optString("control") ?: continue
                if (id.isNotEmpty() && id !in entries) {
                    entries[id] = Entry(supported = false)
                }
            }
        }
        READOUTS.forEach { id ->
            if (id !in entries) {
                entries[id] = Entry(supported = true, source = ControlSource.DEVICE)
            }
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
        val validation = validateControl(id, value)
        if (!validation.ok) {
            return validation
        }
        val applied = validation.applied
        val entry = entries.getValue(id)
        entry.requested = applied
        entry.source = source
        if (id in LOCKS) {
            entry.locked = applied as? Boolean
        }
        return ControlValidation(true, applied = applied)
    }

    /**
     * Validate a value without recording it as requested state. Used for
     * one-shot triggers such as `ae_precapture`.
     */
    @Synchronized
    fun validateControl(id: String, value: Any?): ControlValidation {
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
        return validate(id, descriptor.optJSONObject(id), value)
    }

    /** Record a value read from `TotalCaptureResult`; never changes `source`. */
    @Synchronized
    fun setActual(id: String, value: Any?) {
        val entry = entries[id] ?: return
        entry.actual = ControlValues.normalize(value)
    }

    /** Record a device readout (FPS, battery, …) as both actual and source. */
    @Synchronized
    fun setReadout(id: String, value: Any?) {
        val entry = entries[id] ?: return
        entry.actual = ControlValues.normalize(value)
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
            return ControlValidation(true, applied = ControlValues.normalize(value))
        }
        if (spec.has("modes")) {
            return validateMode(id, spec.getJSONArray("modes"), value)
        }
        if (spec.has("ranges")) {
            return validateFpsRange(id, spec.getJSONArray("ranges"), value)
        }
        if (spec.has("max_regions")) {
            return validateRegions(id, spec.optInt("max_regions"), value)
        }
        if (spec.has("values")) {
            val number =
                ControlValues.asDouble(value)
                    ?: return ControlValidation(
                        false,
                        errorCode = "INVALID_VALUE",
                        message = "control '$id' expects a number",
                    )
            if (ControlValues.matchesChoice(spec.getJSONArray("values"), number)) {
                return ControlValidation(true, applied = ControlValues.roundToMillis(number))
            }
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "$id=$value is not one of the available values",
            )
        }
        if (spec.has("components")) {
            return ColorControls.validateGains(spec, value)
        }
        if (spec.has("matrix_size")) {
            return ColorControls.validateTransform(spec, value)
        }
        if (id in TRIGGERS) {
            return validateTrigger(id, value)
        }
        if (spec.has("min") && spec.has("max")) {
            return validateRange(id, spec, value)
        }
        if (id in BOOLEANS) {
            val bool =
                ControlValues.asBoolean(value)
                    ?: return ControlValidation(
                        false,
                        errorCode = "INVALID_VALUE",
                        message = "control '$id' expects a boolean",
                    )
            return ControlValidation(true, applied = bool)
        }
        return ControlValidation(true, applied = ControlValues.normalize(value))
    }

    private fun validateMode(id: String, modes: JSONArray, value: Any?): ControlValidation {
        val mode =
            ControlValues.asInt(value)
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
            ControlValues.asDouble(value)
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
        val step = spec.optDouble("step", ControlValues.DEFAULT_STEP)
        if (step > ControlValues.DEFAULT_STEP) {
            val steps = (number - min) / step
            if (abs(steps - round(steps)) > ControlValues.STEP_TOLERANCE) {
                return ControlValidation(
                    false,
                    errorCode = "STEP_MISMATCH",
                    message = "$id=$value is not a multiple of step $step from $min",
                )
            }
        }
        return ControlValidation(true, applied = ControlValues.normalizeRangeValue(id, number))
    }

    private fun validateFpsRange(id: String, ranges: JSONArray, value: Any?): ControlValidation {
        val pair =
            fpsPair(value)
                ?: return ControlValidation(
                    false,
                    errorCode = "INVALID_VALUE",
                    message = "control '$id' expects [min, max]",
                )
        for (index in 0 until ranges.length()) {
            val candidate = ranges.optJSONArray(index) ?: continue
            if (candidate.optInt(0, -1) == pair.first && candidate.optInt(1, -1) == pair.second) {
                return ControlValidation(
                    true,
                    applied = JSONArray().put(pair.first).put(pair.second),
                )
            }
        }
        return ControlValidation(
            false,
            errorCode = "OUT_OF_RANGE",
            message = "FPS range [${pair.first}, ${pair.second}] is not available for '$id'",
        )
    }

    private fun fpsPair(value: Any?): Pair<Int, Int>? {
        val first =
            when (value) {
                is JSONArray -> value.opt(0)
                is JSONObject -> value.opt("min")
                else -> null
            }
        val second =
            when (value) {
                is JSONArray -> value.opt(1)
                is JSONObject -> value.opt("max")
                else -> null
            }
        val min = ControlValues.asInt(first) ?: return null
        val max = ControlValues.asInt(second) ?: return null
        return if (max >= min) min to max else null
    }

    private fun validateRegions(id: String, maxRegions: Int, value: Any?): ControlValidation {
        val array =
            value as? JSONArray
                ?: return ControlValidation(
                    false,
                    errorCode = "INVALID_VALUE",
                    message = "control '$id' expects an array of regions",
                )
        if (array.length() < 1 || array.length() > maxRegions) {
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "'$id' accepts 1..$maxRegions regions",
            )
        }
        val out = JSONArray()
        for (index in 0 until array.length()) {
            val region =
                array.optJSONObject(index)
                    ?: return ControlValidation(
                        false,
                        errorCode = "INVALID_VALUE",
                        message = "'$id[$index]' must be an object",
                    )
            val validation = validateRegion(id, index, region)
            if (!validation.ok) {
                return validation
            }
            out.put(validation.applied)
        }
        return ControlValidation(true, applied = out)
    }

    private fun validateRegion(id: String, index: Int, region: JSONObject): ControlValidation {
        val x = ControlValues.asDouble(region.opt("x"))
        val y = ControlValues.asDouble(region.opt("y"))
        val w = ControlValues.asDouble(region.opt("w"))
        val h = ControlValues.asDouble(region.opt("h"))
        if (x == null || y == null) {
            return ControlValidation(
                false,
                errorCode = "INVALID_VALUE",
                message = "'$id[$index]' needs x, y, w and h",
            )
        }
        if (w == null || h == null) {
            return ControlValidation(
                false,
                errorCode = "INVALID_VALUE",
                message = "'$id[$index]' needs x, y, w and h",
            )
        }
        val startsInBounds = x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0
        val endsInBounds = x + w <= 1.0 && y + h <= 1.0
        if (!startsInBounds || !endsInBounds) {
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "'$id[$index]' must fit in normalized [0,1] coordinates",
            )
        }
        val weight = region.optInt("weight", DEFAULT_METERING_WEIGHT)
        if (weight < MIN_METERING_WEIGHT || weight > MAX_METERING_WEIGHT) {
            return ControlValidation(
                false,
                errorCode = "OUT_OF_RANGE",
                message = "'$id[$index].weight' must be in [$MIN_METERING_WEIGHT, $MAX_METERING_WEIGHT]",
            )
        }
        return ControlValidation(
            true,
            applied =
                JSONObject()
                    .put("x", ControlValues.roundToMillis(x))
                    .put("y", ControlValues.roundToMillis(y))
                    .put("w", ControlValues.roundToMillis(w))
                    .put("h", ControlValues.roundToMillis(h))
                    .put("weight", weight),
        )
    }

    private fun validateTrigger(id: String, value: Any?): ControlValidation {
        val trigger = value?.toString()?.lowercase()
        if (trigger != TRIGGER_START && trigger != TRIGGER_CANCEL) {
            return ControlValidation(
                false,
                errorCode = "INVALID_VALUE",
                message = "control '$id' expects \"$TRIGGER_START\" or \"$TRIGGER_CANCEL\"",
            )
        }
        return ControlValidation(true, applied = trigger)
    }

    private companion object {
        const val DEFAULT_METERING_WEIGHT = 1000
        const val MIN_METERING_WEIGHT = -1000
        const val MAX_METERING_WEIGHT = 1000
        const val TRIGGER_START = "start"
        const val TRIGGER_CANCEL = "cancel"
        val LOCKS = setOf("ae_lock", "awb_lock")
        val BOOLEANS = setOf("ae_lock", "awb_lock", "af_lock", "torch")
        val TRIGGERS = setOf("ae_precapture", "af_trigger")
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
                "lens_focal_length",
                "lens_aperture",
                "lens_filter_density",
                "lens_state",
            )
    }
}
