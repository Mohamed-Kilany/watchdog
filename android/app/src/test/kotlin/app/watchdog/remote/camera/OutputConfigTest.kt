package app.watchdog.remote.camera

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * M4-T09 output configuration: defaults, descriptor-driven validation, atomic
 * rollback, dependent updates after a camera switch, and the RAW/stream
 * combination rule.
 */
class OutputConfigTest {
    private fun size(width: Int, height: Int): JSONObject =
        JSONObject().put("width", width).put("height", height)

    private fun sizes(vararg pairs: Pair<Int, Int>): JSONArray {
        val out = JSONArray()
        pairs.forEach { (width, height) -> out.put(size(width, height)) }
        return out
    }

    private fun streamSizes(vararg entries: Triple<Int, Int, Long>): JSONArray {
        val out = JSONArray()
        entries.forEach { (width, height, duration) ->
            out.put(size(width, height).put("min_frame_duration_ns", duration))
        }
        return out
    }

    private fun descriptor(
        cameraId: String,
        raw: Boolean = false,
        physical: List<String> = emptyList(),
        stream: JSONArray =
            streamSizes(
                Triple(4000, 3000, 66_666_666L),
                Triple(1920, 1080, 33_333_333L),
                Triple(1280, 720, 33_333_333L),
            ),
        still: JSONArray = sizes(4000 to 3000, 1920 to 1080),
    ): JSONObject {
        val formats = JSONArray().put(OutputConfigs.JPEG)
        val stillByFormat = JSONObject().put(OutputConfigs.JPEG, still)
        if (raw) {
            formats.put(OutputConfigs.RAW)
            stillByFormat.put(OutputConfigs.RAW, sizes(4000 to 3000))
        }
        val physicalSpec =
            if (physical.isEmpty()) {
                JSONObject().put("supported", false)
            } else {
                val values = JSONArray().put(OutputConfigs.LOGICAL)
                physical.forEach { values.put(it) }
                structural(JSONObject().put("values", values))
            }
        return JSONObject()
            .put("capabilities", JSONArray().apply { if (raw) put("RAW") })
            .put(OutputConfigs.CAMERA_ID, structural(JSONObject().put("values", JSONArray().put(cameraId))))
            .put(OutputConfigs.PHYSICAL, physicalSpec)
            .put(OutputConfigs.STREAM_SIZE, structural(JSONObject().put("sizes", stream)))
            .put(OutputConfigs.JPEG_QUALITY, structural(range(1, 100)))
            .put(
                OutputConfigs.STREAM_ROTATION,
                structural(JSONObject().put("modes", JSONArray().put(0).put(90).put(180).put(270))),
            )
            .put(OutputConfigs.STILL_FORMAT, structural(JSONObject().put("values", formats)))
            .put(OutputConfigs.STILL_SIZE, structural(JSONObject().put("sizes_by_format", stillByFormat)))
            .put(
                OutputConfigs.VIDEO_SIZE,
                structural(JSONObject().put("sizes", sizes(1920 to 1080, 1280 to 720))),
            )
            .put(OutputConfigs.VIDEO_BITRATE, structural(range(100_000, 100_000_000)))
            .put(
                OutputConfigs.VIDEO_CODEC,
                structural(JSONObject().put("values", JSONArray().put("h264").put("h265"))),
            )
            .put(
                OutputConfigs.VIDEO_CONTAINER,
                structural(JSONObject().put("values", JSONArray().put("mp4"))),
            )
            .put(OutputConfigs.AUDIO, structural(JSONObject()))
            .put(
                "ae_fps_range",
                JSONObject()
                    .put("supported", true)
                    .put(
                        "ranges",
                        JSONArray()
                            .put(JSONArray().put(15).put(15))
                            .put(JSONArray().put(30).put(30))
                            .put(JSONArray().put(15).put(30)),
                    ),
            )
    }

    private fun descriptors(vararg cameras: Pair<String, JSONObject>): JSONObject {
        val out = JSONObject()
        cameras.forEach { (id, descriptor) -> out.put(id, descriptor) }
        return out
    }

    private fun structural(spec: JSONObject): JSONObject =
        spec.put("supported", true).put("structural", true)

    private fun range(min: Int, max: Int): JSONObject =
        JSONObject().put("min", min).put("max", max).put("step", 1)

    private fun validate(
        current: OutputConfig,
        descriptors: JSONObject,
        vararg assignments: Pair<String, Any>,
    ): OutputValidation =
        OutputConfigValidator.validate(current, assignments.toList(), descriptors)

    @Test
    fun defaultsPreferStreamingFriendlyOutputs() {
        val descriptor = descriptor("0")
        val config = OutputConfigs.defaults("0", descriptor)
        assertEquals(OutputSize(1280, 720), config.streamSize)
        assertEquals(OutputConfigs.JPEG, config.stillFormat)
        assertEquals(OutputSize(4000, 3000), config.stillSize)
        assertEquals(OutputSize(1920, 1080), config.videoSize)
        assertEquals(OutputConfigs.DEFAULT_VIDEO_BITRATE, config.videoBitrate)
        assertEquals(OutputConfigs.H264, config.videoCodec)
        assertEquals(OutputConfigs.MP4, config.videoContainer)
        assertFalse(config.audio)
        assertNull(config.physicalId)
        assertEquals(0, config.rotation)
        assertEquals(OutputConfigs.DEFAULT_JPEG_QUALITY, config.jpegQuality)
    }

    @Test
    fun acceptsSupportedSizesAndRejectsOddOnes() {
        val descriptor = descriptor("0")
        val current = OutputConfigs.defaults("0", descriptor)
        val applied = validate(current, descriptors("0" to descriptor), OutputConfigs.STREAM_SIZE to size(1920, 1080))
        assertTrue(applied.ok)
        assertEquals(OutputSize(1920, 1080), applied.config.streamSize)

        val odd =
            validate(
                current,
                descriptors("0" to descriptor),
                OutputConfigs.STREAM_SIZE to size(1279, 720),
            )
        assertFalse(odd.ok)
        assertEquals("INVALID_VALUE", odd.errors[OutputConfigs.STREAM_SIZE]?.code)

        val unsupported =
            validate(
                current,
                descriptors("0" to descriptor),
                OutputConfigs.STREAM_SIZE to size(800, 600),
            )
        assertFalse(unsupported.ok)
        assertEquals("OUT_OF_RANGE", unsupported.errors[OutputConfigs.STREAM_SIZE]?.code)
    }

    @Test
    fun aFailedAssignmentRollsBackTheWholeBatch() {
        val descriptor = descriptor("0")
        val current = OutputConfigs.defaults("0", descriptor)
        val result =
            validate(
                current,
                descriptors("0" to descriptor),
                OutputConfigs.STREAM_SIZE to size(1920, 1080),
                OutputConfigs.JPEG_QUALITY to 0,
            )
        assertFalse(result.ok)
        assertEquals(current, result.config)
        assertTrue(result.applied.isEmpty())
        assertEquals("OUT_OF_RANGE", result.errors[OutputConfigs.JPEG_QUALITY]?.code)
    }

    @Test
    fun cameraSwitchResetsDependentControlsToTheNewDefaults() {
        val wide = descriptor("0")
        val narrow =
            descriptor(
                "1",
                stream = streamSizes(Triple(1280, 720, 33_333_333L)),
                still = sizes(1920 to 1080),
            )
        val map = descriptors("0" to wide, "1" to narrow)
        val initial = OutputConfigs.defaults("0", wide)
        val big = validate(initial, map, OutputConfigs.STREAM_SIZE to size(4000, 3000))
        assertTrue(big.ok)
        assertEquals(OutputSize(4000, 3000), big.config.streamSize)

        val switched = validate(big.config, map, OutputConfigs.CAMERA_ID to "1")
        assertTrue(switched.ok)
        assertEquals("1", switched.config.cameraId)
        assertEquals(OutputSize(1280, 720), switched.config.streamSize)
        assertEquals(OutputSize(1920, 1080), switched.config.stillSize)
    }

    @Test
    fun cameraSelectionIsResolvedBeforeDependentValues() {
        val wide =
            descriptor(
                "0",
                stream = streamSizes(Triple(4000, 3000, 66_666_666L), Triple(1920, 1080, 33_333_333L)),
            )
        val narrow = descriptor("1", stream = streamSizes(Triple(1280, 720, 33_333_333L)))
        val map = descriptors("0" to wide, "1" to narrow)
        val current = OutputConfigs.defaults("0", wide)
        // 1280x720 is only valid on camera 1, so this batch succeeds only when
        // the camera switch is resolved before the dependent stream size.
        val switched =
            validate(
                current,
                map,
                OutputConfigs.STREAM_SIZE to size(1280, 720),
                OutputConfigs.CAMERA_ID to "1",
            )
        assertTrue(switched.ok)
        assertEquals("1", switched.config.cameraId)
        assertEquals(OutputSize(1280, 720), switched.config.streamSize)
    }

    @Test
    fun cameraSwitchRejectsUnknownCameras() {
        val descriptor = descriptor("0")
        val current = OutputConfigs.defaults("0", descriptor)
        val result =
            validate(
                current,
                descriptors("0" to descriptor),
                OutputConfigs.CAMERA_ID to "9",
            )
        assertFalse(result.ok)
        assertEquals("UNKNOWN_CAMERA", result.errors[OutputConfigs.CAMERA_ID]?.code)
    }

    @Test
    fun physicalCameraSelectionUsesLogicalAndReportedIds() {
        val descriptor = descriptor("0", physical = listOf("2", "3"))
        val map = descriptors("0" to descriptor)
        val current = OutputConfigs.defaults("0", descriptor)

        val logical = validate(current, map, OutputConfigs.PHYSICAL to "none")
        assertTrue(logical.ok)
        assertNull(logical.config.physicalId)

        val physical = validate(current, map, OutputConfigs.PHYSICAL to "2")
        assertTrue(physical.ok)
        assertEquals("2", physical.config.physicalId)

        val invalid = validate(current, map, OutputConfigs.PHYSICAL to "4")
        assertFalse(invalid.ok)
        assertEquals("OUT_OF_RANGE", invalid.errors[OutputConfigs.PHYSICAL]?.code)
    }

    @Test
    fun stillFormatSwitchUpdatesTheStillSize() {
        val descriptor = descriptor("0", raw = true)
        val current = OutputConfigs.defaults("0", descriptor)
        assertEquals(OutputSize(4000, 3000), current.stillSize)

        val raw = validate(current, descriptors("0" to descriptor), OutputConfigs.STILL_FORMAT to "RAW_SENSOR")
        assertTrue(raw.ok)
        assertEquals(OutputConfigs.RAW, raw.config.stillFormat)
        assertEquals(OutputSize(4000, 3000), raw.config.stillSize)

        val jpeg = validate(raw.config, descriptors("0" to descriptor), OutputConfigs.STILL_FORMAT to "JPEG")
        assertTrue(jpeg.ok)
        assertEquals(OutputConfigs.JPEG, jpeg.config.stillFormat)
    }

    @Test
    fun rawWithLargeLiveStreamIsRejectedAsAnImpossibleCombination() {
        val descriptor = descriptor("0", raw = true)
        val map = descriptors("0" to descriptor)
        val current = OutputConfigs.defaults("0", descriptor)
        val raw = validate(current, map, OutputConfigs.STILL_FORMAT to "RAW_SENSOR")
        assertTrue(raw.ok)

        val large = validate(raw.config, map, OutputConfigs.STREAM_SIZE to size(4000, 3000))
        assertFalse(large.ok)
        assertEquals("UNSUPPORTED_COMBINATION", large.errors[OutputConfigs.STREAM_SIZE]?.code)
    }

    @Test
    fun rawIsNotOfferedWithoutTheRawCapability() {
        val descriptor = descriptor("0", raw = false)
        val current = OutputConfigs.defaults("0", descriptor)
        val result =
            validate(
                current,
                descriptors("0" to descriptor),
                OutputConfigs.STILL_FORMAT to "RAW_SENSOR",
            )
        assertFalse(result.ok)
        assertEquals("OUT_OF_RANGE", result.errors[OutputConfigs.STILL_FORMAT]?.code)
    }

    @Test
    fun rotationQualityAndVideoSettingsAreValidated() {
        val descriptor = descriptor("0")
        val map = descriptors("0" to descriptor)
        val current = OutputConfigs.defaults("0", descriptor)

        assertTrue(validate(current, map, OutputConfigs.STREAM_ROTATION to 90).ok)
        assertFalse(validate(current, map, OutputConfigs.STREAM_ROTATION to 45).ok)
        assertTrue(validate(current, map, OutputConfigs.JPEG_QUALITY to 95).ok)
        assertFalse(validate(current, map, OutputConfigs.JPEG_QUALITY to 101).ok)
        assertTrue(validate(current, map, OutputConfigs.VIDEO_SIZE to size(1920, 1080)).ok)
        assertFalse(validate(current, map, OutputConfigs.VIDEO_SIZE to size(800, 600)).ok)
        assertTrue(validate(current, map, OutputConfigs.VIDEO_BITRATE to 12_000_000).ok)
        assertFalse(validate(current, map, OutputConfigs.VIDEO_BITRATE to 1_000).ok)
        assertTrue(validate(current, map, OutputConfigs.VIDEO_CODEC to "h265").ok)
        assertFalse(validate(current, map, OutputConfigs.VIDEO_CODEC to "vp9").ok)
        assertTrue(validate(current, map, OutputConfigs.VIDEO_CONTAINER to "mp4").ok)
        assertFalse(validate(current, map, OutputConfigs.VIDEO_CONTAINER to "mkv").ok)
        assertTrue(validate(current, map, OutputConfigs.AUDIO to true).ok)
        assertFalse(validate(current, map, OutputConfigs.AUDIO to "yes").ok)
    }

    @Test
    fun fpsSupportComesFromTheStreamMinimumFrameDuration() {
        val descriptor = descriptor("0")
        val small = OutputSize(1280, 720)
        val large = OutputSize(4000, 3000)
        assertTrue(OutputConfigs.isFpsSupported(descriptor, small, JSONArray().put(30).put(30)))
        assertFalse(OutputConfigs.isFpsSupported(descriptor, large, JSONArray().put(30).put(30)))
        assertTrue(OutputConfigs.isFpsSupported(descriptor, large, JSONArray().put(15).put(15)))
        assertEquals("[30,30]", OutputConfigs.defaultFpsRange(descriptor, small).toString())
        assertEquals("[15,15]", OutputConfigs.defaultFpsRange(descriptor, large).toString())
    }

    @Test
    fun parseSizeRejectsIncompleteObjects() {
        assertEquals(OutputSize(1920, 1080), OutputConfigs.parseSize(size(1920, 1080)))
        assertNull(OutputConfigs.parseSize(JSONObject().put("width", 1920)))
        assertNull(OutputConfigs.parseSize("1920x1080"))
    }
}
