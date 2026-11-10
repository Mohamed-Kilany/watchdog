package app.watchdog.remote.camera

import org.json.JSONObject

/**
 * Validates a structural output-configuration batch against the capability
 * descriptor (M4-T09). A batch is atomic: if any assignment is invalid, the
 * previous configuration is kept and every assignment reports why.
 */
object OutputConfigValidator {
    private data class Step(val config: OutputConfig?, val applied: Any?, val error: OutputError? = null)

    fun validate(
        current: OutputConfig,
        assignments: List<Pair<String, Any?>>,
        descriptors: JSONObject,
    ): OutputValidation {
        var config = current
        val applied = LinkedHashMap<String, Any>()
        val errors = LinkedHashMap<String, OutputError>()
        // Camera and physical-camera selection are resolved first so that
        // dependent values in the same batch validate against the camera they
        // will actually run on, regardless of the wire order.
        val ordered = assignments.sortedBy { selectionRank(it.first) }
        ordered.forEach { (id, value) ->
            if (id !in OutputConfigs.CONTROL_IDS) return@forEach
            val step = applyOne(config, id, value, descriptors)
            if (step.error != null) {
                errors[id] = step.error
            } else {
                config = step.config ?: config
                step.applied?.let { applied[id] = it }
            }
        }
        val combination =
            if (errors.isEmpty()) {
                OutputConfigs.combinationError(config, descriptorFor(config.cameraId, descriptors))
            } else {
                null
            }
        combination?.let { errors[it.control] = it }
        return if (errors.isEmpty()) {
            OutputValidation(true, config, applied, emptyMap())
        } else {
            OutputValidation(false, current, emptyMap(), errors)
        }
    }

    private fun applyOne(current: OutputConfig, id: String, value: Any?, descriptors: JSONObject): Step =
        when (id) {
            OutputConfigs.CAMERA_ID -> camera(current, value, descriptors)
            OutputConfigs.PHYSICAL -> physical(current, value, descriptors)
            OutputConfigs.STREAM_SIZE -> streamSize(current, value, descriptors)
            OutputConfigs.JPEG_QUALITY -> jpegQuality(current, value, descriptors)
            OutputConfigs.STREAM_ROTATION -> rotation(current, value, descriptors)
            OutputConfigs.STILL_FORMAT -> stillFormat(current, value, descriptors)
            OutputConfigs.STILL_SIZE -> stillSize(current, value, descriptors)
            OutputConfigs.VIDEO_SIZE -> videoSize(current, value, descriptors)
            OutputConfigs.VIDEO_BITRATE -> videoBitrate(current, value, descriptors)
            OutputConfigs.VIDEO_CODEC -> videoCodec(current, value, descriptors)
            OutputConfigs.VIDEO_CONTAINER -> videoContainer(current, value, descriptors)
            OutputConfigs.AUDIO -> audio(current, value, descriptors)
            else -> Step(current, null, error(id, "UNKNOWN_CONTROL", "control '$id' is not an output control"))
        }

    private fun selectionRank(id: String): Int =
        when (id) {
            OutputConfigs.CAMERA_ID -> 0
            OutputConfigs.PHYSICAL -> 1
            else -> 2
        }

    private fun camera(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val id = OutputConfigs.asString(value)?.trim().orEmpty()
        if (id.isEmpty()) return invalid(OutputConfigs.CAMERA_ID, "camera_id expects a camera id")
        val descriptor =
            descriptors.optJSONObject(id)
                ?: return error(
                    OutputConfigs.CAMERA_ID,
                    "UNKNOWN_CAMERA",
                    "camera '$id' is not available",
                )
        if (descriptor.has("error")) {
            return error(
                OutputConfigs.CAMERA_ID,
                "CAMERA_UNAVAILABLE",
                "camera '$id' is unavailable: ${descriptor.optString("error")}",
            )
        }
        val next = if (id == current.cameraId) current else OutputConfigs.defaults(id, descriptor)
        return Step(next, id)
    }

    private fun physical(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.PHYSICAL)
                ?: return unsupported(OutputConfigs.PHYSICAL)
        val text = OutputConfigs.asString(value)?.trim().orEmpty()
        val id = if (text.isEmpty() || text.equals(OutputConfigs.LOGICAL, ignoreCase = true)) null else text
        val allowed = OutputConfigs.values(spec)
        if (id != null && id !in allowed) {
            return error(
                OutputConfigs.PHYSICAL,
                "OUT_OF_RANGE",
                "physical camera '$id' is not available; choose one of ${allowed.joinToString()}",
            )
        }
        return Step(current.copy(physicalId = id), id ?: OutputConfigs.LOGICAL)
    }

    private fun streamSize(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val size =
            OutputConfigs.parseSize(value)
                ?: return invalid(OutputConfigs.STREAM_SIZE, "stream_size expects {width, height}")
        if (size.width % 2 != 0 || size.height % 2 != 0) {
            return invalid(OutputConfigs.STREAM_SIZE, "stream_size must have even dimensions for the YUV pipeline")
        }
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.STREAM_SIZE)
                ?: return unsupported(OutputConfigs.STREAM_SIZE)
        val sizes = OutputConfigs.sizes(spec.optJSONArray("sizes"))
        if (size !in sizes) {
            return outOfRange(
                OutputConfigs.STREAM_SIZE,
                "stream size $size is not available; supported: ${sizes.joinToString()}",
            )
        }
        return Step(current.copy(streamSize = size), size.toJson())
    }

    private fun jpegQuality(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.JPEG_QUALITY)
                ?: return unsupported(OutputConfigs.JPEG_QUALITY)
        val quality =
            ControlValues.asInt(value)
                ?: return invalid(OutputConfigs.JPEG_QUALITY, "jpeg_quality expects an integer")
        val min = spec.optInt("min", OutputConfigs.MIN_JPEG_QUALITY)
        val max = spec.optInt("max", OutputConfigs.MAX_JPEG_QUALITY)
        if (quality !in min..max) {
            return outOfRange(OutputConfigs.JPEG_QUALITY, "jpeg_quality=$quality is outside [$min, $max]")
        }
        return Step(current.copy(jpegQuality = quality), quality)
    }

    private fun rotation(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        supported(descriptor, OutputConfigs.STREAM_ROTATION) ?: return unsupported(OutputConfigs.STREAM_ROTATION)
        val rotation =
            ControlValues.asInt(value)
                ?: return invalid(OutputConfigs.STREAM_ROTATION, "stream_rotation expects an integer")
        if (rotation !in OutputConfigs.ROTATIONS) {
            return outOfRange(
                OutputConfigs.STREAM_ROTATION,
                "stream_rotation=$rotation is not one of ${OutputConfigs.ROTATIONS.joinToString()}",
            )
        }
        return Step(current.copy(rotation = rotation), rotation)
    }

    private fun stillFormat(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.STILL_FORMAT)
                ?: return unsupported(OutputConfigs.STILL_FORMAT)
        val format =
            OutputConfigs.asString(value)?.uppercase()
                ?: return invalid(OutputConfigs.STILL_FORMAT, "still_format expects a string")
        val formats = OutputConfigs.values(spec)
        if (format !in formats) {
            return outOfRange(
                OutputConfigs.STILL_FORMAT,
                "still_format=$format is not one of ${formats.joinToString()}",
            )
        }
        val sizes = OutputConfigs.stillSizes(descriptor, format)
        val nextSize =
            if (current.stillSize in sizes) {
                current.stillSize
            } else {
                sizes.maxByOrNull { it.width * it.height } ?: current.stillSize
            }
        return Step(current.copy(stillFormat = format, stillSize = nextSize), format)
    }

    private fun stillSize(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        supported(descriptor, OutputConfigs.STILL_SIZE) ?: return unsupported(OutputConfigs.STILL_SIZE)
        val size =
            OutputConfigs.parseSize(value)
                ?: return invalid(OutputConfigs.STILL_SIZE, "still_size expects {width, height}")
        val sizes = OutputConfigs.stillSizes(descriptor, current.stillFormat)
        if (size !in sizes) {
            return outOfRange(
                OutputConfigs.STILL_SIZE,
                "still size $size is not available for ${current.stillFormat}; supported: ${sizes.joinToString()}",
            )
        }
        return Step(current.copy(stillSize = size), size.toJson())
    }

    private fun videoSize(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        supported(descriptor, OutputConfigs.VIDEO_SIZE) ?: return unsupported(OutputConfigs.VIDEO_SIZE)
        val size =
            OutputConfigs.parseSize(value)
                ?: return invalid(OutputConfigs.VIDEO_SIZE, "video_size expects {width, height}")
        val sizes = OutputConfigs.videoSizes(descriptor)
        if (size !in sizes) {
            return outOfRange(
                OutputConfigs.VIDEO_SIZE,
                "video size $size is not available; supported: ${sizes.joinToString()}",
            )
        }
        return Step(current.copy(videoSize = size), size.toJson())
    }

    private fun videoBitrate(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.VIDEO_BITRATE)
                ?: return unsupported(OutputConfigs.VIDEO_BITRATE)
        val bitrate =
            ControlValues.asInt(value)
                ?: return invalid(OutputConfigs.VIDEO_BITRATE, "video_bitrate expects an integer")
        val min = spec.optInt("min", OutputConfigs.MIN_VIDEO_BITRATE)
        val max = spec.optInt("max", OutputConfigs.MAX_VIDEO_BITRATE)
        if (bitrate !in min..max) {
            return outOfRange(OutputConfigs.VIDEO_BITRATE, "video_bitrate=$bitrate is outside [$min, $max]")
        }
        return Step(current.copy(videoBitrate = bitrate), bitrate)
    }

    private fun videoCodec(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec = supported(descriptor, OutputConfigs.VIDEO_CODEC) ?: return unsupported(OutputConfigs.VIDEO_CODEC)
        val codec =
            OutputConfigs.asString(value)?.lowercase()
                ?: return invalid(OutputConfigs.VIDEO_CODEC, "video_codec expects a string")
        val codecs = OutputConfigs.values(spec)
        if (codec !in codecs) {
            return outOfRange(OutputConfigs.VIDEO_CODEC, "video_codec=$codec is not one of ${codecs.joinToString()}")
        }
        return Step(current.copy(videoCodec = codec), codec)
    }

    private fun videoContainer(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec =
            supported(descriptor, OutputConfigs.VIDEO_CONTAINER)
                ?: return unsupported(OutputConfigs.VIDEO_CONTAINER)
        val container =
            OutputConfigs.asString(value)?.lowercase()
                ?: return invalid(OutputConfigs.VIDEO_CONTAINER, "video_container expects a string")
        val containers = OutputConfigs.values(spec)
        if (container !in containers) {
            return outOfRange(
                OutputConfigs.VIDEO_CONTAINER,
                "video_container=$container is not one of ${containers.joinToString()}",
            )
        }
        return Step(current.copy(videoContainer = container), container)
    }

    private fun audio(current: OutputConfig, value: Any?, descriptors: JSONObject): Step {
        val descriptor = descriptorFor(current.cameraId, descriptors)
        val spec = descriptor.optJSONObject(OutputConfigs.AUDIO)
        if (spec == null || !spec.optBoolean("supported")) return unsupported(OutputConfigs.AUDIO)
        val enabled =
            ControlValues.asBoolean(value)
                ?: return invalid(OutputConfigs.AUDIO, "audio expects a boolean")
        return Step(current.copy(audio = enabled), enabled)
    }

    private fun descriptorFor(cameraId: String, descriptors: JSONObject): JSONObject =
        descriptors.optJSONObject(cameraId) ?: JSONObject()

    private fun supported(descriptor: JSONObject, id: String): JSONObject? =
        descriptor.optJSONObject(id)?.takeIf { it.optBoolean("supported") }

    private fun invalid(id: String, message: String): Step = Step(null, null, error(id, "INVALID_VALUE", message))

    private fun unsupported(id: String): Step =
        Step(null, null, error(id, "UNSUPPORTED", "control '$id' is not supported by this camera"))

    private fun outOfRange(id: String, message: String): Step = Step(null, null, error(id, "OUT_OF_RANGE", message))

    private fun error(id: String, code: String, message: String): OutputError = OutputError(id, code, message)
}
