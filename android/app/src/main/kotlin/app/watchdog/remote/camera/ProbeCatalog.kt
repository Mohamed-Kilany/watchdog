package app.watchdog.remote.camera

import android.graphics.Rect
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.params.ColorSpaceTransform
import android.hardware.camera2.params.MeteringRectangle
import android.hardware.camera2.params.RggbChannelVector
import android.hardware.camera2.params.TonemapCurve
import android.os.Build
import android.util.Range
import kotlin.math.max
import kotlin.math.min

/** One documented value to try for a capture-request key. */
data class ProbeCandidate(
    val label: String,
    val value: Any,
    /** Request keys that must be set in the same request for [value] to take effect. */
    val setup: List<Pair<String, Any>> = emptyList(),
)

/**
 * A probeable request key. [values] returns the documented values that make
 * sense for this device; an empty list means the key is advertised but no
 * candidate can be constructed from its characteristics.
 */
data class ProbeSpec(
    val field: String,
    val skipReason: String? = null,
    val values: (CameraCharacteristics) -> List<ProbeCandidate> = { emptyList() },
)

/**
 * Catalog for M4-T10 "probe everything".
 *
 * Keys are resolved by reflection so the app keeps working on Android
 * versions that predate a key (a missing platform field simply removes that
 * spec). Values come from the camera characteristics where the API publishes
 * an allowed set; static documented values are used only where no such list
 * exists.
 */
object ProbeCatalog {
    private const val THIRTY_FPS_NS = 33_333_333L
    private const val HALF = 2
    private const val CENTER_DIVISOR = 4
    private const val METERING_WEIGHT = 1000
    private const val MAX_CANDIDATES = 8
    private const val EXPOSURE_CAP_NS = 33_333_333L
    private const val DEFAULT_ZOOM_PROBE = 2.0f
    private const val WARM_RED_GAIN = 2f
    private const val WARM_GREEN_GAIN = 1.5f
    private const val WARM_MATRIX_RED = 2
    private const val GAMMA_TWO_POINT_TWO = 2.2f
    private const val JPEG_QUALITY_MID = 50
    private const val JPEG_QUALITY_MAX = 100
    private const val CURVE_MIDPOINT = 0.5f
    private const val ROTATION_90 = 90
    private const val ROTATION_180 = 180
    private const val ROTATION_270 = 270

    /** Identity map from the platform key object to its Java field name. */
    val fields: Map<CaptureRequest.Key<*>, String> by lazy { buildFields() }

    private val specs: Map<String, ProbeSpec> by lazy { buildSpecs().associateBy { it.field } }

    fun specFor(field: String): ProbeSpec? = specs[field]

    fun requestKey(field: String): CaptureRequest.Key<*>? = fields.entries.firstOrNull { it.value == field }?.key

    fun allSpecs(): List<ProbeSpec> = specs.values.toList()

    private fun buildFields(): Map<CaptureRequest.Key<*>, String> {
        val out = LinkedHashMap<CaptureRequest.Key<*>, String>()
        CaptureRequest::class.java.fields.forEach { field ->
            val value = field.get(null)
            if (value is CaptureRequest.Key<*>) {
                out[value] = field.name
            }
        }
        return out
    }

    private fun buildSpecs(): List<ProbeSpec> =
        colorSpecs() +
            exposureSpecs() +
            controlSpecs() +
            lensSpecs() +
            outputSpecs() +
            sensorSpecs() +
            statisticsSpecs() +
            skippedSpecs()

    private fun colorSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("COLOR_CORRECTION_ABERRATION_MODE") { chars ->
                modes(chars, CameraCharacteristics.COLOR_CORRECTION_AVAILABLE_ABERRATION_MODES)
            },
            ProbeSpec("COLOR_CORRECTION_GAINS") {
                listOf(
                    ProbeCandidate(
                        "neutral 1:1:1:1",
                        RggbChannelVector(1f, 1f, 1f, 1f),
                        setup = colorCorrectionSetup(),
                    ),
                    ProbeCandidate(
                        "warm 2:1:1.5:1",
                        RggbChannelVector(WARM_RED_GAIN, 1f, WARM_GREEN_GAIN, 1f),
                        setup = colorCorrectionSetup(),
                    ),
                )
            },
            ProbeSpec("COLOR_CORRECTION_MODE") {
                listOf(
                    CaptureRequest.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX,
                    CaptureRequest.COLOR_CORRECTION_MODE_FAST,
                    CaptureRequest.COLOR_CORRECTION_MODE_HIGH_QUALITY,
                ).map { ProbeCandidate("mode $it", it) }
            },
            ProbeSpec("COLOR_CORRECTION_TRANSFORM") {
                listOf(
                    ProbeCandidate("identity", transform(WARM_MATRIX_RED)),
                    ProbeCandidate("warm", transform(WARM_RED_GAIN.toInt())),
                )
            },
        )

    private fun exposureSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("CONTROL_AE_ANTIBANDING_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AE_AVAILABLE_ANTIBANDING_MODES)
            },
            ProbeSpec("CONTROL_AE_EXPOSURE_COMPENSATION") { chars ->
                intRangeCandidates(chars.get(CameraCharacteristics.CONTROL_AE_COMPENSATION_RANGE))
            },
            ProbeSpec("CONTROL_AE_LOCK") { booleans() },
            ProbeSpec("CONTROL_AE_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AE_AVAILABLE_MODES)
            },
            ProbeSpec("CONTROL_AE_REGIONS") { chars ->
                regions(chars, chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AE))
            },
            ProbeSpec("CONTROL_AE_TARGET_FPS_RANGE") { chars -> fpsRanges(chars) },
            ProbeSpec("CONTROL_AF_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES)
            },
            ProbeSpec("CONTROL_AF_REGIONS") { chars ->
                regions(chars, chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AF))
            },
            ProbeSpec("CONTROL_AWB_LOCK") { booleans() },
            ProbeSpec("CONTROL_AWB_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AWB_AVAILABLE_MODES)
            },
            ProbeSpec("CONTROL_AWB_REGIONS") { chars ->
                regions(chars, chars.get(CameraCharacteristics.CONTROL_MAX_REGIONS_AWB))
            },
        )

    private fun controlSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("CONTROL_CAPTURE_INTENT") {
                (CaptureRequest.CONTROL_CAPTURE_INTENT_CUSTOM..CaptureRequest.CONTROL_CAPTURE_INTENT_MOTION_TRACKING)
                    .map { ProbeCandidate("intent $it", it) }
            },
            ProbeSpec("CONTROL_EFFECT_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AVAILABLE_EFFECTS)
            },
            ProbeSpec("CONTROL_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AVAILABLE_MODES)
            },
            ProbeSpec("CONTROL_POST_RAW_SENSITIVITY_BOOST") { chars ->
                intRangeCandidates(chars.get(CameraCharacteristics.CONTROL_POST_RAW_SENSITIVITY_BOOST_RANGE))
            },
            ProbeSpec("CONTROL_SCENE_MODE") { chars ->
                val setup = listOf("CONTROL_MODE" to CaptureRequest.CONTROL_MODE_USE_SCENE_MODE)
                modes(chars, CameraCharacteristics.CONTROL_AVAILABLE_SCENE_MODES).map { it.copy(setup = setup) }
            },
            ProbeSpec("CONTROL_VIDEO_STABILIZATION_MODE") { chars ->
                modes(chars, CameraCharacteristics.CONTROL_AVAILABLE_VIDEO_STABILIZATION_MODES)
            },
            ProbeSpec("CONTROL_ZOOM_RATIO") { chars ->
                val maximum = chars.get(CameraCharacteristics.SCALER_AVAILABLE_MAX_DIGITAL_ZOOM) ?: 1f
                if (maximum <= 1f) {
                    emptyList()
                } else {
                    listOf(1f, min(maximum, DEFAULT_ZOOM_PROBE)).distinct().map {
                        ProbeCandidate("zoom $it", it)
                    }
                }
            },
            ProbeSpec("CONTROL_AUTOFRAMING") {
                listOf(
                    CaptureRequest.CONTROL_AUTOFRAMING_OFF,
                    CaptureRequest.CONTROL_AUTOFRAMING_ON,
                ).map { ProbeCandidate("mode $it", it) }
            },
        )

    private fun lensSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("LENS_APERTURE") { chars ->
                floats(chars, CameraCharacteristics.LENS_INFO_AVAILABLE_APERTURES)
            },
            ProbeSpec("LENS_FILTER_DENSITY") { chars ->
                floats(chars, CameraCharacteristics.LENS_INFO_AVAILABLE_FILTER_DENSITIES)
            },
            ProbeSpec("LENS_FOCAL_LENGTH") { chars ->
                floats(chars, CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)
            },
            ProbeSpec("LENS_FOCUS_DISTANCE") { chars -> focusDistances(chars) },
            ProbeSpec("LENS_OPTICAL_STABILIZATION_MODE") { chars ->
                modes(chars, CameraCharacteristics.LENS_INFO_AVAILABLE_OPTICAL_STABILIZATION)
            },
        )

    private fun outputSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("DISTORTION_CORRECTION_MODE") {
                listOf(
                    CaptureRequest.DISTORTION_CORRECTION_MODE_OFF,
                    CaptureRequest.DISTORTION_CORRECTION_MODE_FAST,
                    CaptureRequest.DISTORTION_CORRECTION_MODE_HIGH_QUALITY,
                ).map { ProbeCandidate("mode $it", it) }
            },
            ProbeSpec("EDGE_MODE") { chars ->
                modes(chars, CameraCharacteristics.EDGE_AVAILABLE_EDGE_MODES)
            },
            ProbeSpec("FLASH_MODE") {
                // Only OFF: a single-capture flash or torch would emit light.
                listOf(ProbeCandidate("OFF", CaptureRequest.FLASH_MODE_OFF))
            },
            ProbeSpec("HOT_PIXEL_MODE") { chars ->
                modes(chars, CameraCharacteristics.HOT_PIXEL_AVAILABLE_HOT_PIXEL_MODES)
            },
            ProbeSpec("JPEG_ORIENTATION") {
                listOf(0, ROTATION_90, ROTATION_180, ROTATION_270).map {
                    ProbeCandidate("$it deg", it)
                }
            },
            ProbeSpec("JPEG_QUALITY") { qualityCandidates() },
            ProbeSpec("JPEG_THUMBNAIL_QUALITY") { qualityCandidates() },
            ProbeSpec("JPEG_THUMBNAIL_SIZE") { chars ->
                chars
                    .get(CameraCharacteristics.JPEG_AVAILABLE_THUMBNAIL_SIZES)
                    ?.distinct()
                    ?.let { listOf(it.first(), it.last()).distinct() }
                    ?.map { ProbeCandidate("${it.width}x${it.height}", it) }
                    .orEmpty()
            },
        )

    private fun sensorSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("NOISE_REDUCTION_MODE") { chars ->
                modes(chars, CameraCharacteristics.NOISE_REDUCTION_AVAILABLE_NOISE_REDUCTION_MODES)
            },
            ProbeSpec("SCALER_CROP_REGION") { chars ->
                cropRegions(chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE))
            },
            ProbeSpec("SCALER_ROTATE_AND_CROP") {
                listOf(
                    CaptureRequest.SCALER_ROTATE_AND_CROP_NONE,
                    CaptureRequest.SCALER_ROTATE_AND_CROP_90,
                    CaptureRequest.SCALER_ROTATE_AND_CROP_180,
                    CaptureRequest.SCALER_ROTATE_AND_CROP_270,
                ).map { ProbeCandidate("rotation $it", it) }
            },
            ProbeSpec("SENSOR_EXPOSURE_TIME") { chars -> exposureTimes(chars) },
            ProbeSpec("SENSOR_FRAME_DURATION") { chars -> frameDurations(chars) },
            ProbeSpec("SENSOR_SENSITIVITY") { chars ->
                intRangeCandidates(chars.get(CameraCharacteristics.SENSOR_INFO_SENSITIVITY_RANGE))
                    .map { it.copy(setup = listOf("CONTROL_AE_MODE" to CaptureRequest.CONTROL_AE_MODE_OFF)) }
            },
            ProbeSpec("SENSOR_TEST_PATTERN_MODE") { chars ->
                modes(chars, CameraCharacteristics.SENSOR_AVAILABLE_TEST_PATTERN_MODES)
            },
            ProbeSpec("SENSOR_PIXEL_MODE") {
                listOf(
                    CaptureRequest.SENSOR_PIXEL_MODE_DEFAULT,
                    CaptureRequest.SENSOR_PIXEL_MODE_MAXIMUM_RESOLUTION,
                ).map { ProbeCandidate("mode $it", it) }
            },
            ProbeSpec("BLACK_LEVEL_LOCK") { booleans() },
            ProbeSpec("TONEMAP_CURVE") { chars -> tonemapCurves(chars) },
            ProbeSpec("TONEMAP_GAMMA") {
                listOf(1f, GAMMA_TWO_POINT_TWO).map {
                    ProbeCandidate(
                        "gamma $it",
                        it,
                        setup = listOf("TONEMAP_MODE" to CaptureRequest.TONEMAP_MODE_GAMMA_VALUE),
                    )
                }
            },
            ProbeSpec("TONEMAP_MODE") { chars ->
                modes(chars, CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES)
            },
            ProbeSpec("TONEMAP_PRESET_CURVE") {
                listOf(
                    CaptureRequest.TONEMAP_PRESET_CURVE_SRGB,
                    CaptureRequest.TONEMAP_PRESET_CURVE_REC709,
                ).map {
                    ProbeCandidate(
                        "preset $it",
                        it,
                        setup = listOf("TONEMAP_MODE" to CaptureRequest.TONEMAP_MODE_PRESET_CURVE),
                    )
                }
            },
        )

    private fun statisticsSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec("STATISTICS_FACE_DETECT_MODE") { chars ->
                modes(chars, CameraCharacteristics.STATISTICS_INFO_AVAILABLE_FACE_DETECT_MODES)
            },
            ProbeSpec("STATISTICS_HOT_PIXEL_MAP_MODE") { booleans() },
            ProbeSpec("STATISTICS_LENS_SHADING_MAP_MODE") { booleans() },
            ProbeSpec("STATISTICS_OIS_DATA_MODE") { booleans() },
        )

    private fun skippedSpecs(): List<ProbeSpec> =
        listOf(
            ProbeSpec(
                "CONTROL_AE_PRECAPTURE_TRIGGER",
                skipReason = "one-shot 3A trigger; probing it would disturb the live stream",
            ),
            ProbeSpec(
                "CONTROL_AF_TRIGGER",
                skipReason = "one-shot 3A trigger; probing it would disturb the live stream",
            ),
            ProbeSpec(
                "JPEG_GPS_LOCATION",
                skipReason = "GPS metadata is not a camera capability",
            ),
            ProbeSpec(
                "LOGICAL_MULTI_CAMERA_ACTIVE_PHYSICAL_ID",
                skipReason = "switching physical cameras requires a new session configuration",
            ),
            ProbeSpec(
                "CONTROL_SETTINGS_OVERRIDE",
                skipReason = "vendor-dependent override bitmask; not safely probeable",
            ),
            ProbeSpec(
                "REPROCESS_EFFECTIVE_EXPOSURE_FACTOR",
                skipReason = "only meaningful for reprocess capture sessions",
            ),
        )

    private fun booleans(): List<ProbeCandidate> = listOf(ProbeCandidate("true", true), ProbeCandidate("false", false))

    private fun colorCorrectionSetup(): List<Pair<String, Any>> =
        listOf("COLOR_CORRECTION_MODE" to CaptureRequest.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX)

    private fun qualityCandidates(): List<ProbeCandidate> =
        listOf(1, JPEG_QUALITY_MID, JPEG_QUALITY_MAX).map { ProbeCandidate("quality $it", it) }

    private fun modes(chars: CameraCharacteristics, key: CameraCharacteristics.Key<IntArray>): List<ProbeCandidate> =
        chars
            .get(key)
            ?.distinct()
            ?.take(MAX_CANDIDATES)
            ?.map { ProbeCandidate("mode $it", it) }
            .orEmpty()

    private fun floats(
        chars: CameraCharacteristics,
        key: CameraCharacteristics.Key<FloatArray>,
    ): List<ProbeCandidate> =
        chars
            .get(key)
            ?.distinct()
            ?.take(MAX_CANDIDATES)
            ?.map { ProbeCandidate("$it", it) }
            .orEmpty()

    private fun intRangeCandidates(range: Range<Int>?): List<ProbeCandidate> {
        if (range == null) return emptyList()
        val middle = (range.lower + range.upper) / HALF
        return listOf(range.lower, middle, range.upper)
            .distinct()
            .take(MAX_CANDIDATES)
            .map { ProbeCandidate("$it", it) }
    }

    private fun fpsRanges(chars: CameraCharacteristics): List<ProbeCandidate> =
        chars
            .get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
            ?.distinct()
            ?.take(MAX_CANDIDATES)
            ?.map { ProbeCandidate("${it.lower}-${it.upper} fps", it) }
            .orEmpty()

    private fun regions(chars: CameraCharacteristics, maxRegions: Int?): List<ProbeCandidate> {
        if ((maxRegions ?: 0) <= 0) return emptyList()
        val active = chars.get(CameraCharacteristics.SENSOR_INFO_ACTIVE_ARRAY_SIZE) ?: return emptyList()
        val centre =
            MeteringRectangle(
                Rect(
                    active.left + active.width() / CENTER_DIVISOR,
                    active.top + active.height() / CENTER_DIVISOR,
                    active.right - active.width() / CENTER_DIVISOR,
                    active.bottom - active.height() / CENTER_DIVISOR,
                ),
                METERING_WEIGHT,
            )
        return listOf(ProbeCandidate("centre region", arrayOf(centre)))
    }

    private fun cropRegions(active: Rect?): List<ProbeCandidate> {
        if (active == null) return emptyList()
        val halfWidth = active.width() / HALF
        val halfHeight = active.height() / HALF
        val centre =
            Rect(
                active.left + active.width() / CENTER_DIVISOR,
                active.top + active.height() / CENTER_DIVISOR,
                active.left + active.width() / CENTER_DIVISOR + halfWidth,
                active.top + active.height() / CENTER_DIVISOR + halfHeight,
            )
        return listOf(
            ProbeCandidate("full ${active.width()}x${active.height()}", active),
            ProbeCandidate("half ${halfWidth}x$halfHeight", centre),
        )
    }

    private fun focusDistances(chars: CameraCharacteristics): List<ProbeCandidate> {
        val minimum = chars.get(CameraCharacteristics.LENS_INFO_MINIMUM_FOCUS_DISTANCE) ?: 0f
        if (minimum <= 0f) return emptyList()
        val setup = listOf("CONTROL_AF_MODE" to CaptureRequest.CONTROL_AF_MODE_OFF)
        return listOf(0f, minimum / HALF, minimum)
            .distinct()
            .map { ProbeCandidate("$it dioptre", it, setup) }
    }

    private fun exposureTimes(chars: CameraCharacteristics): List<ProbeCandidate> {
        val range = chars.get(CameraCharacteristics.SENSOR_INFO_EXPOSURE_TIME_RANGE) ?: return emptyList()
        val middle = min(range.upper, max(range.lower, EXPOSURE_CAP_NS))
        val setup = listOf("CONTROL_AE_MODE" to CaptureRequest.CONTROL_AE_MODE_OFF)
        return listOf(range.lower, middle, range.upper)
            .distinct()
            .take(MAX_CANDIDATES)
            .map { ProbeCandidate("$it ns", it, setup) }
    }

    private fun frameDurations(chars: CameraCharacteristics): List<ProbeCandidate> {
        val maximum = chars.get(CameraCharacteristics.SENSOR_INFO_MAX_FRAME_DURATION) ?: return emptyList()
        val setup = listOf("CONTROL_AE_MODE" to CaptureRequest.CONTROL_AE_MODE_OFF)
        return listOf(min(THIRTY_FPS_NS, maximum), maximum)
            .distinct()
            .map { ProbeCandidate("$it ns", it, setup) }
    }

    private fun tonemapCurves(chars: CameraCharacteristics): List<ProbeCandidate> {
        val available = chars.get(CameraCharacteristics.TONEMAP_AVAILABLE_TONE_MAP_MODES) ?: return emptyList()
        if (!available.contains(CaptureRequest.TONEMAP_MODE_CONTRAST_CURVE)) return emptyList()
        val points = floatArrayOf(0f, 0f, CURVE_MIDPOINT, CURVE_MIDPOINT, 1f, 1f)
        val setup = listOf("TONEMAP_MODE" to CaptureRequest.TONEMAP_MODE_CONTRAST_CURVE)
        return listOf(ProbeCandidate("linear 3-point", TonemapCurve(points, points, points), setup))
    }

    /** Identity matrix, or a warm matrix when [red] is greater than one. */
    private fun transform(red: Int): ColorSpaceTransform {
        val redScale = if (red > 1) red else 1
        return ColorSpaceTransform(
            intArrayOf(
                redScale,
                1,
                0,
                1,
                0,
                1,
                0,
                1,
                1,
                1,
                0,
                1,
                0,
                1,
                0,
                1,
                1,
                1,
            ),
        )
    }

    /** Human-readable name for common enum values, used by the matrix renderer. */
    fun valueName(field: String, value: Any?): String? {
        val number = value as? Int ?: return null
        return NAMES[field]?.get(number)
    }

    private val NAMES: Map<String, Map<Int, String>> =
        buildMap {
            put(
                "CONTROL_AE_MODE",
                mapOf(
                    CaptureRequest.CONTROL_AE_MODE_OFF to "OFF",
                    CaptureRequest.CONTROL_AE_MODE_ON to "ON",
                    CaptureRequest.CONTROL_AE_MODE_ON_AUTO_FLASH to "ON_AUTO_FLASH",
                    CaptureRequest.CONTROL_AE_MODE_ON_ALWAYS_FLASH to "ON_ALWAYS_FLASH",
                    CaptureRequest.CONTROL_AE_MODE_ON_AUTO_FLASH_REDEYE to "ON_AUTO_FLASH_REDEYE",
                    CaptureRequest.CONTROL_AE_MODE_ON_EXTERNAL_FLASH to "ON_EXTERNAL_FLASH",
                ),
            )
            put(
                "CONTROL_AF_MODE",
                mapOf(
                    CaptureRequest.CONTROL_AF_MODE_OFF to "OFF",
                    CaptureRequest.CONTROL_AF_MODE_AUTO to "AUTO",
                    CaptureRequest.CONTROL_AF_MODE_MACRO to "MACRO",
                    CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO to "CONTINUOUS_VIDEO",
                    CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE to "CONTINUOUS_PICTURE",
                    CaptureRequest.CONTROL_AF_MODE_EDOF to "EDOF",
                ),
            )
            put(
                "CONTROL_AWB_MODE",
                mapOf(
                    CaptureRequest.CONTROL_AWB_MODE_OFF to "OFF",
                    CaptureRequest.CONTROL_AWB_MODE_AUTO to "AUTO",
                    CaptureRequest.CONTROL_AWB_MODE_INCANDESCENT to "INCANDESCENT",
                    CaptureRequest.CONTROL_AWB_MODE_FLUORESCENT to "FLUORESCENT",
                    CaptureRequest.CONTROL_AWB_MODE_WARM_FLUORESCENT to "WARM_FLUORESCENT",
                    CaptureRequest.CONTROL_AWB_MODE_DAYLIGHT to "DAYLIGHT",
                    CaptureRequest.CONTROL_AWB_MODE_CLOUDY_DAYLIGHT to "CLOUDY_DAYLIGHT",
                    CaptureRequest.CONTROL_AWB_MODE_TWILIGHT to "TWILIGHT",
                    CaptureRequest.CONTROL_AWB_MODE_SHADE to "SHADE",
                ),
            )
            put(
                "CONTROL_MODE",
                mapOf(
                    CaptureRequest.CONTROL_MODE_OFF to "OFF",
                    CaptureRequest.CONTROL_MODE_AUTO to "AUTO",
                    CaptureRequest.CONTROL_MODE_USE_SCENE_MODE to "USE_SCENE_MODE",
                    CaptureRequest.CONTROL_MODE_OFF_KEEP_STATE to "OFF_KEEP_STATE",
                ),
            )
            put(
                "CONTROL_CAPTURE_INTENT",
                mapOf(
                    CaptureRequest.CONTROL_CAPTURE_INTENT_CUSTOM to "CUSTOM",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_PREVIEW to "PREVIEW",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_STILL_CAPTURE to "STILL_CAPTURE",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_VIDEO_RECORD to "VIDEO_RECORD",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_VIDEO_SNAPSHOT to "VIDEO_SNAPSHOT",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_ZERO_SHUTTER_LAG to "ZERO_SHUTTER_LAG",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_MANUAL to "MANUAL",
                    CaptureRequest.CONTROL_CAPTURE_INTENT_MOTION_TRACKING to "MOTION_TRACKING",
                ),
            )
            put(
                "TONEMAP_MODE",
                mapOf(
                    CaptureRequest.TONEMAP_MODE_CONTRAST_CURVE to "CONTRAST_CURVE",
                    CaptureRequest.TONEMAP_MODE_FAST to "FAST",
                    CaptureRequest.TONEMAP_MODE_HIGH_QUALITY to "HIGH_QUALITY",
                    CaptureRequest.TONEMAP_MODE_GAMMA_VALUE to "GAMMA_VALUE",
                    CaptureRequest.TONEMAP_MODE_PRESET_CURVE to "PRESET_CURVE",
                ),
            )
            put(
                "FLASH_MODE",
                mapOf(
                    CaptureRequest.FLASH_MODE_OFF to "OFF",
                    CaptureRequest.FLASH_MODE_SINGLE to "SINGLE",
                    CaptureRequest.FLASH_MODE_TORCH to "TORCH",
                ),
            )
            put(
                "COLOR_CORRECTION_MODE",
                mapOf(
                    CaptureRequest.COLOR_CORRECTION_MODE_TRANSFORM_MATRIX to "TRANSFORM_MATRIX",
                    CaptureRequest.COLOR_CORRECTION_MODE_FAST to "FAST",
                    CaptureRequest.COLOR_CORRECTION_MODE_HIGH_QUALITY to "HIGH_QUALITY",
                ),
            )
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                put(
                    "SCALER_ROTATE_AND_CROP",
                    mapOf(
                        CaptureRequest.SCALER_ROTATE_AND_CROP_NONE to "NONE",
                        CaptureRequest.SCALER_ROTATE_AND_CROP_90 to "90",
                        CaptureRequest.SCALER_ROTATE_AND_CROP_180 to "180",
                        CaptureRequest.SCALER_ROTATE_AND_CROP_270 to "270",
                    ),
                )
                put(
                    "DISTORTION_CORRECTION_MODE",
                    mapOf(
                        CaptureRequest.DISTORTION_CORRECTION_MODE_OFF to "OFF",
                        CaptureRequest.DISTORTION_CORRECTION_MODE_FAST to "FAST",
                        CaptureRequest.DISTORTION_CORRECTION_MODE_HIGH_QUALITY to "HIGH_QUALITY",
                    ),
                )
            }
        }
}
