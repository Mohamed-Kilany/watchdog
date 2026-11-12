package app.watchdog.remote.camera

import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CaptureFailure
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.MeteringRectangle
import android.hardware.camera2.params.RggbChannelVector
import android.os.Build
import android.os.Handler
import android.util.Range
import android.util.Size
import android.view.Surface
import app.watchdog.remote.BuildConfig
import app.watchdog.remote.log.DeviceLog
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.max

/**
 * M4-T10 "probe everything": submits one single capture per documented value
 * of every advertised [CaptureRequest] key, then reads the same key back from
 * the [TotalCaptureResult].
 *
 * The probe never touches the repeating request. Each candidate is built from
 * the current requested controls, plus the candidate's setup keys, and is sent
 * with `session.capture`; the live stream resumes on the next repeating frame.
 * One key at a time is probed, failures are bounded, and keys that would
 * reconfigure the session or fire a trigger are skipped by [ProbeCatalog].
 *
 * [run] blocks the calling thread (the service's probe worker) and posts all
 * camera work to [handler], so capture callbacks can still be delivered.
 */
class KeyProbeEngine(
    private val device: CameraDevice,
    private val session: CameraCaptureSession,
    private val chars: CameraCharacteristics,
    private val handler: Handler,
    private val targets: List<Surface>,
    private val baseline: Map<String, Any>,
) {
    private val resultKeys = chars.availableCaptureResultKeys
    private val resultKeyByName = resultKeys.associateBy { it.name }
    private val fieldNames = ProbeCatalog.fields
    private val deadline = System.currentTimeMillis() + PROBE_DEADLINE_MS
    private var consecutiveFailures = 0

    fun run(cameraId: String, onlyKey: String?): JSONObject {
        val entries = JSONArray()
        val keys =
            chars.availableCaptureRequestKeys.filter {
                onlyKey == null || onlyKey == fieldNames[it] || onlyKey == it.name
            }
        val abortReason = probeKeys(keys, entries)
        val (probedKeys, probedValues) = summarize(entries)
        return JSONObject()
            .put("schema", REPORT_SCHEMA)
            .put("camera_id", cameraId)
            .put("facing", facingName(chars.get(CameraCharacteristics.LENS_FACING)))
            .put("hardware_level", hardwareLevelName(chars.get(CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL)))
            .put("device", deviceInfo())
            .put("generated_at_ms", System.currentTimeMillis())
            .put("available_request_keys", chars.availableCaptureRequestKeys.size)
            .put("available_result_keys", resultKeys.size)
            .put("probed_keys", probedKeys)
            .put("probed_values", probedValues)
            .put("aborted", abortReason != null)
            .put("abort_reason", abortReason ?: JSONObject.NULL)
            .put("entries", entries)
            .put("not_advertised", notAdvertised())
    }

    /** Probe [keys] in order; returns the abort reason when probing stopped early. */
    private fun probeKeys(keys: List<CaptureRequest.Key<*>>, entries: JSONArray): String? {
        for (key in keys) {
            if (System.currentTimeMillis() > deadline) return "probe deadline reached"
            entries.put(probeKey(key, fieldNames[key], key.name))
            if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
                return "$consecutiveFailures consecutive capture failures"
            }
        }
        return null
    }

    private fun summarize(entries: JSONArray): Pair<Int, Int> {
        var keys = 0
        var values = 0
        for (index in 0 until entries.length()) {
            val entry = entries.getJSONObject(index)
            if (entry.optString("status") == "probed") {
                keys++
                values += entry.optJSONArray("values")?.length() ?: 0
            }
        }
        return keys to values
    }

    private fun notAdvertised(): JSONArray {
        val missing = JSONArray()
        ProbeCatalog.allSpecs().forEach { spec ->
            val key = ProbeCatalog.requestKey(spec.field) ?: return@forEach
            if (!chars.availableCaptureRequestKeys.contains(key)) {
                missing.put(JSONObject().put("field", spec.field).put("key", key.name))
            }
        }
        return missing
    }

    private fun probeKey(key: CaptureRequest.Key<*>, field: String?, name: String): JSONObject {
        val entry =
            JSONObject()
                .put("key", name)
                .put("field", field ?: JSONObject.NULL)
                .put("request_available", true)
                .put("result_available", resultKeyByName.containsKey(key.name))
        val spec = field?.let(ProbeCatalog::specFor)
        if (field == null) {
            return entry.put("status", "unprobed").put("reason", "vendor key without a documented probe table")
        }
        if (spec == null) {
            return entry.put("status", "unprobed").put("reason", "no documented probe table in watchdog")
        }
        if (spec.skipReason != null) {
            return entry.put("status", "skipped").put("reason", spec.skipReason)
        }
        val candidates =
            runCatching { spec.values(chars) }
                .onFailure { DeviceLog.w(TAG, "probe catalog failed for $field: ${it.message}") }
                .getOrDefault(emptyList())
        if (candidates.isEmpty()) {
            return entry.put("status", "unprobed").put("reason", "no candidate values are available on this device")
        }
        val values = JSONArray()
        candidates.forEach { candidate ->
            values.put(probeValue(key, field, candidate))
        }
        return entry.put("status", "probed").put("values", values)
    }

    private fun probeValue(key: CaptureRequest.Key<*>, field: String, candidate: ProbeCandidate): JSONObject {
        val name = ProbeCatalog.valueName(field, candidate.value)
        val label = if (name == null) candidate.label else "${candidate.label} ($name)"
        val outcome = capture(key, candidate)
        if (outcome.error != null) {
            consecutiveFailures = if (outcome.fatal) consecutiveFailures + 1 else 0
            return JSONObject()
                .put("label", label)
                .put("requested", jsonValue(candidate.value))
                .put("outcome", "error")
                .put("reflected", false)
                .put("error", outcome.error)
                .put("elapsed_ms", outcome.elapsedMs)
        }
        consecutiveFailures = 0
        val reflected = resultKeyByName[key.name]?.let { outcome.result?.get(it) }
        val matches = sameValue(candidate.value, reflected)
        return JSONObject()
            .put("label", label)
            .put("requested", jsonValue(candidate.value))
            .put("outcome", if (matches) "accepted" else "ignored")
            .put("reflected", matches)
            .put("result", if (outcome.result == null) JSONObject.NULL else jsonValue(reflected))
            .put("elapsed_ms", outcome.elapsedMs)
    }

    /** [fatal] is true when the capture itself failed rather than just the value. */
    private data class CaptureOutcome(
        val result: TotalCaptureResult?,
        val error: String?,
        val elapsedMs: Long,
        val fatal: Boolean = false,
    )

    private fun capture(key: CaptureRequest.Key<*>, candidate: ProbeCandidate): CaptureOutcome {
        val started = System.currentTimeMillis()
        val latch = CountDownLatch(1)
        var result: TotalCaptureResult? = null
        var failure: String? = null
        var fatalFailure = false
        val posted =
            handler.post {
                runCatching {
                    val builder = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                    targets.forEach { builder.addTarget(it) }
                    builder.set(CaptureRequest.CONTROL_MODE, CaptureRequest.CONTROL_MODE_AUTO)
                    baseline.forEach { (id, value) -> CameraControls.applyTo(builder, chars, id, value) }
                    CameraControls.applyTonemap(builder, baseline)
                    candidate.setup.forEach { (setupField, value) ->
                        ProbeCatalog.requestKey(setupField)?.let { setKey(builder, it, value) }
                    }
                    setKey(builder, key, candidate.value)
                    session.capture(
                        builder.build(),
                        object : CameraCaptureSession.CaptureCallback() {
                            override fun onCaptureCompleted(
                                session: CameraCaptureSession,
                                request: CaptureRequest,
                                total: TotalCaptureResult,
                            ) {
                                result = total
                                latch.countDown()
                            }

                            override fun onCaptureFailed(
                                session: CameraCaptureSession,
                                request: CaptureRequest,
                                captureFailure: CaptureFailure,
                            ) {
                                failure = "capture failed (reason ${captureFailure.reason})"
                                fatalFailure = true
                                latch.countDown()
                            }
                        },
                        handler,
                    )
                }.onFailure {
                    failure = "${it.javaClass.simpleName}: ${it.message}"
                    latch.countDown()
                }
            }
        if (!posted) {
            return CaptureOutcome(null, "camera handler is not available", 0, fatal = true)
        }
        val completed = latch.await(CAPTURE_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        val elapsed = System.currentTimeMillis() - started
        return when {
            !completed ->
                CaptureOutcome(null, "no capture result within ${CAPTURE_TIMEOUT_MS}ms", elapsed, fatal = true)
            failure != null -> CaptureOutcome(null, failure, elapsed, fatal = fatalFailure)
            else -> CaptureOutcome(result, null, elapsed)
        }
    }

    @Suppress("UNCHECKED_CAST")
    private fun setKey(builder: CaptureRequest.Builder, key: CaptureRequest.Key<*>, value: Any) {
        builder.set(key as CaptureRequest.Key<Any>, value)
    }

    private fun deviceInfo(): JSONObject =
        JSONObject()
            .put("model", Build.MODEL)
            .put("device", Build.DEVICE)
            .put("android", Build.VERSION.RELEASE)
            .put("sdk", Build.VERSION.SDK_INT)
            .put("app_version", BuildConfig.VERSION_NAME)

    private fun facingName(facing: Int?): String =
        when (facing) {
            CameraCharacteristics.LENS_FACING_FRONT -> "front"
            CameraCharacteristics.LENS_FACING_BACK -> "back"
            CameraCharacteristics.LENS_FACING_EXTERNAL -> "external"
            else -> "unknown"
        }

    private fun hardwareLevelName(level: Int?): String =
        when (level) {
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LEGACY -> "LEGACY"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_LIMITED -> "LIMITED"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_FULL -> "FULL"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_3 -> "LEVEL_3"
            CameraCharacteristics.INFO_SUPPORTED_HARDWARE_LEVEL_EXTERNAL -> "EXTERNAL"
            else -> "unknown"
        }

    private fun jsonValue(value: Any?): Any =
        when (value) {
            null -> JSONObject.NULL
            is Boolean, is Number, is String -> value
            is Range<*> -> JSONArray(listOf(jsonValue(value.lower), jsonValue(value.upper)))
            is Array<*> -> JSONArray(value.map(::jsonValue))
            is IntArray -> JSONArray(value.toList())
            is LongArray -> JSONArray(value.toList())
            is FloatArray -> JSONArray(value.toList())
            is DoubleArray -> JSONArray(value.toList())
            else -> complexValue(value)
        }

    private fun complexValue(value: Any): Any =
        when (value) {
            is Size -> JSONObject().put("width", value.width).put("height", value.height)
            is android.graphics.Rect ->
                JSONObject()
                    .put("left", value.left)
                    .put("top", value.top)
                    .put("right", value.right)
                    .put("bottom", value.bottom)
            is MeteringRectangle ->
                JSONObject()
                    .put("rect", jsonValue(value.rect))
                    .put("weight", value.meteringWeight)
            is RggbChannelVector ->
                JSONObject()
                    .put("red", value.red)
                    .put("green_even", value.greenEven)
                    .put("green_odd", value.greenOdd)
                    .put("blue", value.blue)
            else -> value.toString()
        }

    private fun sameValue(requested: Any?, reflected: Any?): Boolean {
        if (requested == null || reflected == null) return requested == reflected
        if (requested is Number && reflected is Number) return numbersClose(requested, reflected)
        return sameStructuredValue(requested, reflected)
    }

    private fun sameStructuredValue(requested: Any, reflected: Any): Boolean =
        when {
            requested is Range<*> && reflected is Range<*> ->
                sameValue(requested.lower, reflected.lower) && sameValue(requested.upper, reflected.upper)
            requested is IntArray && reflected is IntArray -> requested.contentEquals(reflected)
            requested is LongArray && reflected is LongArray -> requested.contentEquals(reflected)
            requested is FloatArray && reflected is FloatArray -> arraysClose(requested, reflected)
            requested is Array<*> && reflected is Array<*> -> sameArray(requested, reflected)
            else -> requested == reflected || requested.toString() == reflected.toString()
        }

    private fun sameArray(requested: Array<*>, reflected: Array<*>): Boolean =
        requested.size == reflected.size &&
            requested.indices.all { sameValue(requested[it], reflected[it]) }

    private fun arraysClose(requested: FloatArray, reflected: FloatArray): Boolean =
        requested.size == reflected.size &&
            requested.indices.all { numbersClose(requested[it], reflected[it]) }

    private fun numbersClose(a: Number, b: Number): Boolean {
        val left = a.toDouble()
        val right = b.toDouble()
        if (left == right) return true
        val tolerance = max(MIN_NUMERIC_TOLERANCE, abs(left) * NUMERIC_TOLERANCE_FRACTION)
        return abs(left - right) <= tolerance
    }

    companion object {
        private const val TAG = "WatchdogProbe"
        private const val REPORT_SCHEMA = 1
        private const val CAPTURE_TIMEOUT_MS = 3_000L
        private const val PROBE_DEADLINE_MS = 180_000L
        private const val MAX_CONSECUTIVE_FAILURES = 5
        private const val MIN_NUMERIC_TOLERANCE = 0.001
        private const val NUMERIC_TOLERANCE_FRACTION = 0.05
    }
}
