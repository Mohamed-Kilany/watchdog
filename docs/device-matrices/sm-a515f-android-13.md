# Control compatibility matrix — SM-A515F (Android 13, SDK 33)

- Serial: `192.168.93.76:41573`
- Camera: `0` (back), hardware level LIMITED
- App: `0.1.0-dev+d1bab57`
- Request keys: 36 available, 23 probed; result keys: 45 available
- Values: 60 accepted, 2 ignored, 6 error
- Keys: 3 skipped, 0 unprobed
- Aborted: 6 consecutive capture failures

`accepted` = the request was submitted and the capture result reflected the value; `ignored` = the request was submitted but the result did not reflect it; `error` = the HAL rejected the request or no result arrived.

| Key | Requested | Label | Outcome | Reflected | Result | ms |
|---|---|---|---|---|---|---|
| `CaptureRequest.Key(android.colorCorrection.aberrationMode)` | `0` | mode 0 | accepted | yes | `0` | 281 |
| `CaptureRequest.Key(android.control.aeAntibandingMode)` | `0` | mode 0 | accepted | yes | `0` | 288 |
| `CaptureRequest.Key(android.control.aeAntibandingMode)` | `3` | mode 3 | accepted | yes | `3` | 267 |
| `CaptureRequest.Key(android.control.aeExposureCompensation)` | `-20` | -20 | ignored | no | `0` | 265 |
| `CaptureRequest.Key(android.control.aeExposureCompensation)` | `0` | 0 | accepted | yes | `0` | 266 |
| `CaptureRequest.Key(android.control.aeExposureCompensation)` | `20` | 20 | ignored | no | `0` | 303 |
| `CaptureRequest.Key(android.control.aeLock)` | `true` | true | accepted | yes | `true` | 298 |
| `CaptureRequest.Key(android.control.aeLock)` | `false` | false | accepted | yes | `false` | 314 |
| `CaptureRequest.Key(android.control.aeMode)` | `0` | mode 0 (OFF) | accepted | yes | `0` | 282 |
| `CaptureRequest.Key(android.control.aeMode)` | `1` | mode 1 (ON) | accepted | yes | `1` | 268 |
| `CaptureRequest.Key(android.control.aeMode)` | `2` | mode 2 (ON_AUTO_FLASH) | accepted | yes | `2` | 266 |
| `CaptureRequest.Key(android.control.aeMode)` | `3` | mode 3 (ON_ALWAYS_FLASH) | accepted | yes | `3` | 285 |
| `CaptureRequest.Key(android.control.aeRegions)` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 280 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[15,15]` | 15-15 fps | accepted | yes | `[15,15]` | 265 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[15,20]` | 15-20 fps | accepted | yes | `[15,20]` | 299 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[20,20]` | 20-20 fps | accepted | yes | `[20,20]` | 267 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[24,24]` | 24-24 fps | accepted | yes | `[24,24]` | 283 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[8,30]` | 8-30 fps | accepted | yes | `[8,30]` | 275 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[10,30]` | 10-30 fps | accepted | yes | `[10,30]` | 267 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[15,30]` | 15-30 fps | accepted | yes | `[15,30]` | 294 |
| `CaptureRequest.Key(android.control.aeTargetFpsRange)` | `[30,30]` | 30-30 fps | accepted | yes | `[30,30]` | 270 |
| `CaptureRequest.Key(android.control.afMode)` | `0` | mode 0 (OFF) | accepted | yes | `0` | 265 |
| `CaptureRequest.Key(android.control.afMode)` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 262 |
| `CaptureRequest.Key(android.control.afMode)` | `3` | mode 3 (CONTINUOUS_VIDEO) | accepted | yes | `3` | 271 |
| `CaptureRequest.Key(android.control.afMode)` | `4` | mode 4 (CONTINUOUS_PICTURE) | accepted | yes | `4` | 269 |
| `CaptureRequest.Key(android.control.afRegions)` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 299 |
| `CaptureRequest.Key(android.control.awbLock)` | `true` | true | accepted | yes | `true` | 297 |
| `CaptureRequest.Key(android.control.awbLock)` | `false` | false | accepted | yes | `false` | 264 |
| `CaptureRequest.Key(android.control.awbMode)` | `0` | mode 0 (OFF) | accepted | yes | `0` | 268 |
| `CaptureRequest.Key(android.control.awbMode)` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 282 |
| `CaptureRequest.Key(android.control.awbMode)` | `2` | mode 2 (INCANDESCENT) | accepted | yes | `2` | 282 |
| `CaptureRequest.Key(android.control.awbMode)` | `3` | mode 3 (FLUORESCENT) | accepted | yes | `3` | 266 |
| `CaptureRequest.Key(android.control.awbMode)` | `5` | mode 5 (DAYLIGHT) | accepted | yes | `5` | 266 |
| `CaptureRequest.Key(android.control.awbMode)` | `6` | mode 6 (CLOUDY_DAYLIGHT) | accepted | yes | `6` | 270 |
| `CaptureRequest.Key(android.control.awbRegions)` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 265 |
| `CaptureRequest.Key(android.control.captureIntent)` | `0` | intent 0 (CUSTOM) | accepted | yes | `0` | 261 |
| `CaptureRequest.Key(android.control.captureIntent)` | `1` | intent 1 (PREVIEW) | accepted | yes | `1` | 286 |
| `CaptureRequest.Key(android.control.captureIntent)` | `2` | intent 2 (STILL_CAPTURE) | accepted | yes | `2` | 284 |
| `CaptureRequest.Key(android.control.captureIntent)` | `3` | intent 3 (VIDEO_RECORD) | accepted | yes | `3` | 264 |
| `CaptureRequest.Key(android.control.captureIntent)` | `4` | intent 4 (VIDEO_SNAPSHOT) | accepted | yes | `4` | 291 |
| `CaptureRequest.Key(android.control.captureIntent)` | `5` | intent 5 (ZERO_SHUTTER_LAG) | accepted | yes | `5` | 278 |
| `CaptureRequest.Key(android.control.captureIntent)` | `6` | intent 6 (MANUAL) | accepted | yes | `6` | 264 |
| `CaptureRequest.Key(android.control.captureIntent)` | `7` | intent 7 (MOTION_TRACKING) | accepted | yes | `7` | 276 |
| `CaptureRequest.Key(android.control.effectMode)` | `0` | mode 0 | accepted | yes | `0` | 299 |
| `CaptureRequest.Key(android.control.effectMode)` | `1` | mode 1 | accepted | yes | `1` | 290 |
| `CaptureRequest.Key(android.control.effectMode)` | `2` | mode 2 | accepted | yes | `2` | 270 |
| `CaptureRequest.Key(android.control.effectMode)` | `4` | mode 4 | accepted | yes | `4` | 322 |
| `CaptureRequest.Key(android.control.effectMode)` | `5` | mode 5 | accepted | yes | `5` | 277 |
| `CaptureRequest.Key(android.control.effectMode)` | `8` | mode 8 | accepted | yes | `8` | 299 |
| `CaptureRequest.Key(android.control.mode)` | `0` | mode 0 (OFF) | accepted | yes | `0` | 297 |
| `CaptureRequest.Key(android.control.mode)` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 301 |
| `CaptureRequest.Key(android.control.mode)` | `2` | mode 2 (USE_SCENE_MODE) | accepted | yes | `2` | 296 |
| `CaptureRequest.Key(android.control.postRawSensitivityBoost)` | `100` | 100 | accepted | yes | `100` | 266 |
| `CaptureRequest.Key(android.control.sceneMode)` | `1` | mode 1 | accepted | yes | `1` | 289 |
| `CaptureRequest.Key(android.control.videoStabilizationMode)` | `0` | mode 0 | accepted | yes | `0` | 276 |
| `CaptureRequest.Key(android.control.zoomRatio)` | `1` | zoom 1.0 | accepted | yes | `1` | 266 |
| `CaptureRequest.Key(android.control.zoomRatio)` | `2` | zoom 2.0 | accepted | yes | `2` | 350 |
| `CaptureRequest.Key(android.flash.mode)` | `0` | OFF (OFF) | accepted | yes | `0` | 318 |
| `CaptureRequest.Key(android.jpeg.orientation)` | `0` | 0 deg | accepted | yes | `0` | 267 |
| `CaptureRequest.Key(android.jpeg.orientation)` | `90` | 90 deg | accepted | yes | `90` | 284 |
| `CaptureRequest.Key(android.jpeg.orientation)` | `180` | 180 deg | accepted | yes | `180` | 286 |
| `CaptureRequest.Key(android.jpeg.orientation)` | `270` | 270 deg | accepted | yes | `270` | 295 |
| `CaptureRequest.Key(android.jpeg.quality)` | `1` | quality 1 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 2 |
| `CaptureRequest.Key(android.jpeg.quality)` | `50` | quality 50 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 5 |
| `CaptureRequest.Key(android.jpeg.quality)` | `100` | quality 100 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 8 |
| `CaptureRequest.Key(android.jpeg.thumbnailQuality)` | `1` | quality 1 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 2 |
| `CaptureRequest.Key(android.jpeg.thumbnailQuality)` | `50` | quality 50 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 1 |
| `CaptureRequest.Key(android.jpeg.thumbnailQuality)` | `100` | quality 100 | error | no | `error: UnsupportedOperationException: Expected native type 1, but got 0` | 1 |

## Keys not probed

| Key | Status | Reason |
|---|---|---|
| `CaptureRequest.Key(android.control.aePrecaptureTrigger)` | skipped | one-shot 3A trigger; probing it would disturb the live stream |
| `CaptureRequest.Key(android.control.afTrigger)` | skipped | one-shot 3A trigger; probing it would disturb the live stream |
| `CaptureRequest.Key(android.jpeg.gpsLocation)` | skipped | GPS metadata is not a camera capability |

## Known keys not advertised by this camera

- `COLOR_CORRECTION_GAINS` (`CaptureRequest.Key(android.colorCorrection.gains)`)
- `COLOR_CORRECTION_MODE` (`CaptureRequest.Key(android.colorCorrection.mode)`)
- `COLOR_CORRECTION_TRANSFORM` (`CaptureRequest.Key(android.colorCorrection.transform)`)
- `LENS_FOCUS_DISTANCE` (`CaptureRequest.Key(android.lens.focusDistance)`)
- `DISTORTION_CORRECTION_MODE` (`CaptureRequest.Key(android.distortionCorrection.mode)`)
- `EDGE_MODE` (`CaptureRequest.Key(android.edge.mode)`)
- `HOT_PIXEL_MODE` (`CaptureRequest.Key(android.hotPixel.mode)`)
- `SCALER_ROTATE_AND_CROP` (`CaptureRequest.Key(android.scaler.rotateAndCrop)`)
- `SENSOR_EXPOSURE_TIME` (`CaptureRequest.Key(android.sensor.exposureTime)`)
- `SENSOR_FRAME_DURATION` (`CaptureRequest.Key(android.sensor.frameDuration)`)
- `SENSOR_SENSITIVITY` (`CaptureRequest.Key(android.sensor.sensitivity)`)
- `SENSOR_PIXEL_MODE` (`CaptureRequest.Key(android.sensor.pixelMode)`)
- `BLACK_LEVEL_LOCK` (`CaptureRequest.Key(android.blackLevel.lock)`)
- `TONEMAP_GAMMA` (`CaptureRequest.Key(android.tonemap.gamma)`)
- `TONEMAP_MODE` (`CaptureRequest.Key(android.tonemap.mode)`)
- `TONEMAP_PRESET_CURVE` (`CaptureRequest.Key(android.tonemap.presetCurve)`)
- `STATISTICS_HOT_PIXEL_MAP_MODE` (`CaptureRequest.Key(android.statistics.hotPixelMapMode)`)
- `STATISTICS_LENS_SHADING_MAP_MODE` (`CaptureRequest.Key(android.statistics.lensShadingMapMode)`)
- `STATISTICS_OIS_DATA_MODE` (`CaptureRequest.Key(android.statistics.oisDataMode)`)
- `REPROCESS_EFFECTIVE_EXPOSURE_FACTOR` (`CaptureRequest.Key(android.reprocess.effectiveExposureFactor)`)
