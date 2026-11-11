package app.watchdog.remote

import android.content.Context
import android.view.LayoutInflater
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import kotlin.math.floor

/** Control ids shared by the descriptor, the state store and the panels. */
private object ControlIds {
    const val AE_MODE = "ae_mode"
    const val EXPOSURE_TIME_NS = "exposure_time_ns"
    const val ISO = "iso"
    const val FRAME_DURATION_NS = "frame_duration_ns"
    const val AE_COMPENSATION = "ae_compensation"
    const val AE_LOCK = "ae_lock"
    const val AE_REGIONS = "ae_regions"
    const val AE_FPS_RANGE = "ae_fps_range"
    const val ANTI_BANDING = "anti_banding"
    const val AE_PRECAPTURE = "ae_precapture"
    const val AE_STATE = "ae_state"
    const val CONTROL_MODE = "control_mode"

    const val AF_MODE = "af_mode"
    const val FOCUS_DISTANCE = "focus_distance"
    const val AF_LOCK = "af_lock"
    const val AF_REGIONS = "af_regions"
    const val AF_TRIGGER = "af_trigger"
    const val AF_STATE = "af_state"
    const val FOCUS_DISTANCE_DIOPTERS = "focus_distance_diopters"

    const val AWB_MODE = "awb_mode"
    const val AWB_LOCK = "awb_lock"
    const val AWB_REGIONS = "awb_regions"
    const val AWB_GAINS = "awb_gains"
    const val COLOR_TRANSFORM = "color_transform"
    const val EFFECT = "effect"
    const val ABERRATION_CORRECTION = "aberration_correction"
    const val AWB_STATE = "awb_state"

    const val ZOOM_RATIO = "zoom_ratio"
    const val ZOOM_ACTUAL = "zoom_actual"
    const val LENS_FOCAL_LENGTH = "lens_focal_length"
    const val LENS_APERTURE = "lens_aperture"
    const val LENS_FILTER_DENSITY = "lens_filter_density"
    const val LENS_STATE = "lens_state"
    const val FILTER_DENSITY = "filter_density"
    const val OPTICAL_STABILIZATION = "optical_stabilization"
    const val VIDEO_STABILIZATION = "video_stabilization"
    const val LENS_SHADING = "lens_shading"
    const val DISTORTION_CORRECTION = "distortion_correction"

    const val TORCH = "torch"
    const val FLASH_MODE = "flash_mode"
    const val SCENE_MODE = "scene_mode"
    const val NOISE_REDUCTION = "noise_reduction"
    const val EDGE_ENHANCEMENT = "edge_enhancement"
    const val HOT_PIXEL = "hot_pixel"
    const val TONEMAP = "tonemap"
    const val TONEMAP_CURVE = "tonemap_curve"
    const val TONEMAP_GAMMA = "tonemap_gamma"
    const val TONEMAP_PRESET_CURVE = "tonemap_preset_curve"
    const val FACE_DETECT = "face_detect"
    const val FACE_COUNT = "face_count"
    const val FACES = "faces"
    const val TEST_PATTERN = "test_pattern"

    const val CAMERA_ID = "camera_id"
    const val PHYSICAL_CAMERA_ID = "physical_camera_id"
    const val STREAM_SIZE = "stream_size"
    const val JPEG_QUALITY = "jpeg_quality"
    const val STREAM_ROTATION = "stream_rotation"
    const val STILL_FORMAT = "still_format"
    const val STILL_SIZE = "still_size"
    const val VIDEO_SIZE = "video_size"
    const val VIDEO_BITRATE = "video_bitrate"
    const val VIDEO_CODEC = "video_codec"
    const val VIDEO_CONTAINER = "video_container"
    const val AUDIO = "audio"
    const val FPS_ACTUAL = "fps_actual"

    const val RECORDING = "recording"
    const val BATTERY_PERCENT = "battery_percent"
    const val STORAGE_FREE_MB = "storage_free_mb"
}

private data class GroupSpec(val titleRes: Int, val left: Boolean, val ids: List<String>)

private data class ControlRow(val label: String, val value: String, val readoutOnly: Boolean)

private data class ControlGroup(val titleRes: Int, val left: Boolean, val rows: List<ControlRow>)

/**
 * Turns the canonical control state (`docs/protocol.md` §5.4) into the grouped
 * rows shown on the device. The renderer is deliberately data-driven: every
 * control the camera reports appears even when this build has no dedicated
 * label for it, so the panels never silently drop state.
 */
private object ControlPanelModel {
    private const val SOURCE_DEVICE = "device"
    private const val PLACEHOLDER = "—"
    private const val MATRIX_ROW = 3
    private const val MAX_LIST_ITEMS = 3
    private const val NANOS_PER_MILLISECOND = 1_000_000.0
    private const val BITS_PER_MEGABIT = 1_000_000.0

    private val GROUPS =
        listOf(
            GroupSpec(
                R.string.group_exposure,
                true,
                listOf(
                    ControlIds.CONTROL_MODE,
                    ControlIds.AE_MODE,
                    ControlIds.EXPOSURE_TIME_NS,
                    ControlIds.ISO,
                    ControlIds.FRAME_DURATION_NS,
                    ControlIds.AE_COMPENSATION,
                    ControlIds.AE_LOCK,
                    ControlIds.AE_REGIONS,
                    ControlIds.AE_FPS_RANGE,
                    ControlIds.ANTI_BANDING,
                    ControlIds.AE_PRECAPTURE,
                    ControlIds.AE_STATE,
                ),
            ),
            GroupSpec(
                R.string.group_focus,
                true,
                listOf(
                    ControlIds.AF_MODE,
                    ControlIds.FOCUS_DISTANCE,
                    ControlIds.FOCUS_DISTANCE_DIOPTERS,
                    ControlIds.AF_LOCK,
                    ControlIds.AF_REGIONS,
                    ControlIds.AF_TRIGGER,
                    ControlIds.AF_STATE,
                ),
            ),
            GroupSpec(
                R.string.group_white_balance,
                true,
                listOf(
                    ControlIds.AWB_MODE,
                    ControlIds.AWB_LOCK,
                    ControlIds.AWB_REGIONS,
                    ControlIds.AWB_GAINS,
                    ControlIds.COLOR_TRANSFORM,
                    ControlIds.AWB_STATE,
                    ControlIds.EFFECT,
                    ControlIds.ABERRATION_CORRECTION,
                ),
            ),
            GroupSpec(
                R.string.group_lens,
                false,
                listOf(
                    ControlIds.ZOOM_RATIO,
                    ControlIds.ZOOM_ACTUAL,
                    ControlIds.LENS_FOCAL_LENGTH,
                    ControlIds.LENS_APERTURE,
                    ControlIds.LENS_FILTER_DENSITY,
                    ControlIds.LENS_STATE,
                    ControlIds.FILTER_DENSITY,
                    ControlIds.OPTICAL_STABILIZATION,
                    ControlIds.VIDEO_STABILIZATION,
                    ControlIds.LENS_SHADING,
                    ControlIds.DISTORTION_CORRECTION,
                ),
            ),
            GroupSpec(
                R.string.group_flash_scene,
                false,
                listOf(
                    ControlIds.TORCH,
                    ControlIds.FLASH_MODE,
                    ControlIds.SCENE_MODE,
                    ControlIds.NOISE_REDUCTION,
                    ControlIds.EDGE_ENHANCEMENT,
                    ControlIds.HOT_PIXEL,
                    ControlIds.TONEMAP,
                    ControlIds.TONEMAP_CURVE,
                    ControlIds.TONEMAP_GAMMA,
                    ControlIds.TONEMAP_PRESET_CURVE,
                    ControlIds.FACE_DETECT,
                    ControlIds.FACE_COUNT,
                    ControlIds.FACES,
                    ControlIds.TEST_PATTERN,
                ),
            ),
            GroupSpec(
                R.string.group_output,
                false,
                listOf(
                    ControlIds.CAMERA_ID,
                    ControlIds.PHYSICAL_CAMERA_ID,
                    ControlIds.STREAM_SIZE,
                    ControlIds.JPEG_QUALITY,
                    ControlIds.STREAM_ROTATION,
                    ControlIds.STILL_FORMAT,
                    ControlIds.STILL_SIZE,
                    ControlIds.VIDEO_SIZE,
                    ControlIds.VIDEO_BITRATE,
                    ControlIds.VIDEO_CODEC,
                    ControlIds.VIDEO_CONTAINER,
                    ControlIds.AUDIO,
                    ControlIds.FPS_ACTUAL,
                ),
            ),
            GroupSpec(
                R.string.group_recording,
                false,
                listOf(ControlIds.RECORDING, ControlIds.BATTERY_PERCENT, ControlIds.STORAGE_FREE_MB),
            ),
        )

    private val LABELS =
        mapOf(
            ControlIds.AE_MODE to "AE mode",
            ControlIds.EXPOSURE_TIME_NS to "Exposure time",
            ControlIds.ISO to "ISO",
            ControlIds.FRAME_DURATION_NS to "Frame duration",
            ControlIds.AE_COMPENSATION to "Exposure compensation",
            ControlIds.AE_LOCK to "AE lock",
            ControlIds.AE_REGIONS to "AE regions",
            ControlIds.AE_FPS_RANGE to "Target FPS range",
            ControlIds.ANTI_BANDING to "Anti-banding",
            ControlIds.AE_PRECAPTURE to "AE precapture",
            ControlIds.AE_STATE to "AE state",
            ControlIds.CONTROL_MODE to "Control mode",
            ControlIds.AF_MODE to "AF mode",
            ControlIds.FOCUS_DISTANCE to "Focus distance",
            ControlIds.FOCUS_DISTANCE_DIOPTERS to "Focus distance (readout)",
            ControlIds.AF_LOCK to "AF lock",
            ControlIds.AF_REGIONS to "AF regions",
            ControlIds.AF_TRIGGER to "AF trigger",
            ControlIds.AF_STATE to "AF state",
            ControlIds.AWB_MODE to "AWB mode",
            ControlIds.AWB_LOCK to "AWB lock",
            ControlIds.AWB_REGIONS to "AWB regions",
            ControlIds.AWB_GAINS to "AWB gains",
            ControlIds.COLOR_TRANSFORM to "Colour matrix",
            ControlIds.AWB_STATE to "AWB state",
            ControlIds.EFFECT to "Colour effect",
            ControlIds.ABERRATION_CORRECTION to "Aberration correction",
            ControlIds.ZOOM_RATIO to "Zoom ratio",
            ControlIds.ZOOM_ACTUAL to "Zoom (readout)",
            ControlIds.LENS_FOCAL_LENGTH to "Focal length",
            ControlIds.LENS_APERTURE to "Aperture",
            ControlIds.LENS_FILTER_DENSITY to "ND density (readout)",
            ControlIds.LENS_STATE to "Lens state",
            ControlIds.FILTER_DENSITY to "Filter density",
            ControlIds.OPTICAL_STABILIZATION to "Optical stabilization",
            ControlIds.VIDEO_STABILIZATION to "Video stabilization",
            ControlIds.LENS_SHADING to "Lens shading",
            ControlIds.DISTORTION_CORRECTION to "Distortion correction",
            ControlIds.TORCH to "Torch",
            ControlIds.FLASH_MODE to "Flash mode",
            ControlIds.SCENE_MODE to "Scene mode",
            ControlIds.NOISE_REDUCTION to "Noise reduction",
            ControlIds.EDGE_ENHANCEMENT to "Edge enhancement",
            ControlIds.HOT_PIXEL to "Hot pixel",
            ControlIds.TONEMAP to "Tonemap",
            ControlIds.TONEMAP_CURVE to "Contrast curve",
            ControlIds.TONEMAP_GAMMA to "Tonemap gamma",
            ControlIds.TONEMAP_PRESET_CURVE to "Preset curve",
            ControlIds.FACE_DETECT to "Face detection",
            ControlIds.FACE_COUNT to "Face count",
            ControlIds.FACES to "Face boxes",
            ControlIds.TEST_PATTERN to "Test pattern",
            ControlIds.CAMERA_ID to "Camera",
            ControlIds.PHYSICAL_CAMERA_ID to "Physical camera",
            ControlIds.STREAM_SIZE to "Live stream",
            ControlIds.JPEG_QUALITY to "JPEG quality",
            ControlIds.STREAM_ROTATION to "Stream rotation",
            ControlIds.STILL_FORMAT to "Still format",
            ControlIds.STILL_SIZE to "Still size",
            ControlIds.VIDEO_SIZE to "Video size",
            ControlIds.VIDEO_BITRATE to "Video bitrate",
            ControlIds.VIDEO_CODEC to "Video codec",
            ControlIds.VIDEO_CONTAINER to "Video container",
            ControlIds.AUDIO to "Audio",
            ControlIds.FPS_ACTUAL to "Measured FPS",
            ControlIds.RECORDING to "Recording",
            ControlIds.BATTERY_PERCENT to "Battery",
            ControlIds.STORAGE_FREE_MB to "Storage free",
        )

    /** Convert a raw control-state snapshot into display readouts. */
    fun snapshot(state: JSONObject): Map<String, ControlReadout> {
        val controls = LinkedHashMap<String, ControlReadout>()
        val keys = state.keys()
        while (keys.hasNext()) {
            val id = keys.next()
            val entry = state.optJSONObject(id) ?: continue
            controls[id] =
                ControlReadout(
                    requested = entry.opt("requested")?.let { formatValue(id, it) },
                    actual = entry.opt("actual")?.let { formatValue(id, it) },
                    supported = entry.optBoolean("supported"),
                    source = entry.optString("source", SOURCE_DEVICE),
                    locked = entry.optBoolean("locked"),
                )
        }
        return controls
    }

    /** Group the readouts, including a fallback group for unknown controls. */
    fun groups(controls: Map<String, ControlReadout>): List<ControlGroup> {
        val grouped = HashSet<String>()
        val groups = ArrayList<ControlGroup>()
        for (spec in GROUPS) {
            val rows = ArrayList<ControlRow>()
            for (id in spec.ids) {
                grouped += id
                row(id, controls[id])?.let(rows::add)
            }
            if (rows.isNotEmpty()) {
                groups += ControlGroup(spec.titleRes, spec.left, rows)
            }
        }
        val other = controls.keys.filter { it !in grouped }.mapNotNull { row(it, controls[it]) }
        if (other.isNotEmpty()) {
            groups += ControlGroup(R.string.group_other, false, other)
        }
        return groups
    }

    private fun row(id: String, control: ControlReadout?): ControlRow? {
        if (control == null) return null
        val value = display(control) ?: return null
        return ControlRow(label(id), value, readoutOnly = !control.supported && control.actual != null)
    }

    private fun display(control: ControlReadout): String? {
        val actual = control.actual
        val requested = control.requested
        if (!control.supported && actual == null) return null
        val base = actual ?: requested ?: PLACEHOLDER
        val details = ArrayList<String>(2)
        if (requested != null && actual == null) {
            details += "via ${control.source}"
        } else if (requested != null && requested != actual) {
            details += "req $requested (${control.source})"
        }
        if (control.locked) {
            details += "locked"
        }
        return if (details.isEmpty()) base else "$base · ${details.joinToString(" · ")}"
    }

    private fun label(id: String): String = LABELS[id] ?: id.replace('_', ' ')

    private fun formatValue(id: String, value: Any?): String =
        when (value) {
            null, JSONObject.NULL -> PLACEHOLDER
            is JSONObject -> formatObject(id, value)
            is JSONArray -> formatArray(id, value)
            is Boolean -> if (value) "on" else "off"
            is Number -> formatNumber(id, value.toDouble())
            else -> value.toString()
        }

    private fun formatNumber(id: String, value: Double): String =
        when (id) {
            ControlIds.EXPOSURE_TIME_NS, ControlIds.FRAME_DURATION_NS ->
                scaled(value, NANOS_PER_MILLISECOND, " ms")
            ControlIds.VIDEO_BITRATE -> scaled(value, BITS_PER_MEGABIT, " Mbps")
            ControlIds.FOCUS_DISTANCE, ControlIds.FOCUS_DISTANCE_DIOPTERS -> "${decimal(value)} dpt"
            ControlIds.LENS_FOCAL_LENGTH -> "${decimal(value)} mm"
            ControlIds.LENS_APERTURE -> "f/${decimal(value)}"
            ControlIds.ZOOM_RATIO, ControlIds.ZOOM_ACTUAL -> "${decimal(value)}x"
            ControlIds.FPS_ACTUAL -> "${decimal(value)} fps"
            ControlIds.LENS_FILTER_DENSITY -> "${decimal(value)} ND"
            ControlIds.STREAM_ROTATION -> "${decimal(value)}°"
            ControlIds.JPEG_QUALITY -> "${decimal(value)}%"
            ControlIds.AE_COMPENSATION -> signed(value)
            else -> decimal(value)
        }

    private fun formatObject(id: String, value: JSONObject): String {
        objectValue(value)?.let { return it }
        val keys = value.keys()
        val parts = ArrayList<String>()
        while (keys.hasNext()) {
            val key = keys.next()
            parts += "$key ${formatValue(id, value.opt(key))}"
        }
        return parts.joinToString(" · ").ifEmpty { PLACEHOLDER }
    }

    private fun objectValue(value: JSONObject): String? {
        if (value.has("width") && value.has("height")) {
            return "${value.optInt("width")}×${value.optInt("height")}"
        }
        if (value.has("r") && value.has("b")) {
            return gains(value)
        }
        if (value.has("red") && value.has("green") && value.has("blue")) {
            return curve(value)
        }
        if (value.has("min") && value.has("max")) {
            return "${decimal(value.optDouble("min"))}–${decimal(value.optDouble("max"))}"
        }
        if (value.has("components")) {
            return matrix(value)
        }
        return null
    }

    private fun curve(value: JSONObject): String {
        val red = value.optJSONArray("red")?.length() ?: 0
        val green = value.optJSONArray("green")?.length() ?: 0
        val blue = value.optJSONArray("blue")?.length() ?: 0
        return "curve $red/$green/$blue pts"
    }

    private fun matrix(value: JSONObject): String {
        val size = value.optInt("matrix_size", 0)
        return if (size > 0) "${size / MATRIX_ROW}×$MATRIX_ROW matrix" else "matrix"
    }

    private fun gains(value: JSONObject): String {
        val red = decimal(value.optDouble("r"))
        val green =
            if (value.has("g_even")) {
                "${decimal(value.optDouble("g_even"))}/${decimal(value.optDouble("g_odd"))}"
            } else {
                decimal(value.optDouble("g"))
            }
        return "r $red · g $green · b ${decimal(value.optDouble("b"))}"
    }

    private fun formatArray(id: String, value: JSONArray): String {
        if (value.length() == 0) return "0"
        val first = value.opt(0)
        if (first is JSONObject) {
            return when {
                first.has("x") -> "${value.length()} region${plural(value.length())}"
                first.has("score") || first.has("id") -> "${value.length()} face${plural(value.length())}"
                else -> "${value.length()} items"
            }
        }
        if (id == ControlIds.AE_FPS_RANGE && value.length() >= 2) {
            return "${value.optInt(0)}–${value.optInt(1)} fps"
        }
        val shown = (0 until minOf(value.length(), MAX_LIST_ITEMS)).map { formatValue(id, value.opt(it)) }
        val suffix = if (value.length() > MAX_LIST_ITEMS) " …" else ""
        return shown.joinToString(", ") + suffix
    }

    private fun plural(count: Int): String = if (count == 1) "" else "s"

    private fun signed(value: Double): String {
        val text = decimal(value)
        return if (value > 0) "+$text" else text
    }

    private fun scaled(value: Double, divisor: Double, unit: String): String = decimal(value / divisor) + unit

    private fun decimal(value: Double): String =
        if (value == floor(value)) {
            value.toLong().toString()
        } else {
            String.format(Locale.US, "%.2f", value)
        }
}

/** Convert a raw control-state snapshot for the on-device panels. */
internal fun controlSnapshot(state: JSONObject): Map<String, ControlReadout> = ControlPanelModel.snapshot(state)

/**
 * Renders the grouped control readouts into the two side panels. Rebuilding is
 * skipped while the values are unchanged, so the twice-per-second refresh does
 * not churn the view hierarchy.
 */
class UiControlPanel(private val context: Context, private val left: LinearLayout, private val right: LinearLayout) {
    private var rendered: List<ControlGroup>? = null

    fun render(controls: Map<String, ControlReadout>) {
        val groups = ControlPanelModel.groups(controls)
        if (groups == rendered) return
        rendered = groups
        left.removeAllViews()
        right.removeAllViews()
        groups.forEach { group -> addGroup(if (group.left) left else right, group) }
    }

    private fun addGroup(container: LinearLayout, group: ControlGroup) {
        val title = LayoutInflater.from(context).inflate(R.layout.item_control_group, container, false) as TextView
        title.text = context.getString(group.titleRes)
        container.addView(title)
        group.rows.forEach { row -> addRow(container, row) }
    }

    private fun addRow(container: LinearLayout, row: ControlRow) {
        val view = LayoutInflater.from(context).inflate(R.layout.item_control, container, false)
        val label = view.findViewById<TextView>(R.id.controlLabel)
        label.text =
            if (row.readoutOnly) {
                context.getString(R.string.control_readout_label, row.label)
            } else {
                row.label
            }
        view.findViewById<TextView>(R.id.controlValue).text = row.value
        container.addView(view)
    }
}
