# Control compatibility matrix — SM-A515F (Android 13, SDK 33)

- Serial: `192.168.93.76:41573`
- Camera: `0` (back), hardware level LIMITED
- App: `0.1.0-dev+361aa14`
- Request keys: 36 available, 33 probed; result keys: 45 available
- Values: 82 accepted, 2 ignored, 0 error
- Keys: 3 skipped, 0 unprobed
- Aborted: no

`accepted` = the request was submitted and the capture result reflected the value; `ignored` = the request was submitted but the result did not reflect it; `error` = the HAL rejected the request or no result arrived.

| Key | Requested | Label | Outcome | Reflected | Result | ms |
|---|---|---|---|---|---|---|
| `android.colorCorrection.aberrationMode` | `0` | mode 0 | accepted | yes | `0` | 278 |
| `android.control.aeAntibandingMode` | `0` | mode 0 | accepted | yes | `0` | 312 |
| `android.control.aeAntibandingMode` | `3` | mode 3 | accepted | yes | `3` | 280 |
| `android.control.aeExposureCompensation` | `-20` | -20 | ignored | no | `0` | 265 |
| `android.control.aeExposureCompensation` | `0` | 0 | accepted | yes | `0` | 266 |
| `android.control.aeExposureCompensation` | `20` | 20 | ignored | no | `0` | 274 |
| `android.control.aeLock` | `true` | true | accepted | yes | `true` | 293 |
| `android.control.aeLock` | `false` | false | accepted | yes | `false` | 295 |
| `android.control.aeMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 282 |
| `android.control.aeMode` | `1` | mode 1 (ON) | accepted | yes | `1` | 286 |
| `android.control.aeMode` | `2` | mode 2 (ON_AUTO_FLASH) | accepted | yes | `2` | 269 |
| `android.control.aeMode` | `3` | mode 3 (ON_ALWAYS_FLASH) | accepted | yes | `3` | 286 |
| `android.control.aeRegions` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 280 |
| `android.control.aeTargetFpsRange` | `[15,15]` | 15-15 fps | accepted | yes | `[15,15]` | 265 |
| `android.control.aeTargetFpsRange` | `[15,20]` | 15-20 fps | accepted | yes | `[15,20]` | 297 |
| `android.control.aeTargetFpsRange` | `[20,20]` | 20-20 fps | accepted | yes | `[20,20]` | 282 |
| `android.control.aeTargetFpsRange` | `[24,24]` | 24-24 fps | accepted | yes | `[24,24]` | 287 |
| `android.control.aeTargetFpsRange` | `[8,30]` | 8-30 fps | accepted | yes | `[8,30]` | 270 |
| `android.control.aeTargetFpsRange` | `[10,30]` | 10-30 fps | accepted | yes | `[10,30]` | 298 |
| `android.control.aeTargetFpsRange` | `[15,30]` | 15-30 fps | accepted | yes | `[15,30]` | 270 |
| `android.control.aeTargetFpsRange` | `[30,30]` | 30-30 fps | accepted | yes | `[30,30]` | 262 |
| `android.control.afMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 270 |
| `android.control.afMode` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 267 |
| `android.control.afMode` | `3` | mode 3 (CONTINUOUS_VIDEO) | accepted | yes | `3` | 265 |
| `android.control.afMode` | `4` | mode 4 (CONTINUOUS_PICTURE) | accepted | yes | `4` | 268 |
| `android.control.afRegions` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 267 |
| `android.control.awbLock` | `true` | true | accepted | yes | `true` | 265 |
| `android.control.awbLock` | `false` | false | accepted | yes | `false` | 265 |
| `android.control.awbMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 282 |
| `android.control.awbMode` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 284 |
| `android.control.awbMode` | `2` | mode 2 (INCANDESCENT) | accepted | yes | `2` | 267 |
| `android.control.awbMode` | `3` | mode 3 (FLUORESCENT) | accepted | yes | `3` | 298 |
| `android.control.awbMode` | `5` | mode 5 (DAYLIGHT) | accepted | yes | `5` | 298 |
| `android.control.awbMode` | `6` | mode 6 (CLOUDY_DAYLIGHT) | accepted | yes | `6` | 268 |
| `android.control.awbRegions` | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":2250,"left":1000,"right":3000,"top":750},"weight":1000}]` | 264 |
| `android.control.captureIntent` | `0` | intent 0 (CUSTOM) | accepted | yes | `0` | 271 |
| `android.control.captureIntent` | `1` | intent 1 (PREVIEW) | accepted | yes | `1` | 298 |
| `android.control.captureIntent` | `2` | intent 2 (STILL_CAPTURE) | accepted | yes | `2` | 299 |
| `android.control.captureIntent` | `3` | intent 3 (VIDEO_RECORD) | accepted | yes | `3` | 266 |
| `android.control.captureIntent` | `4` | intent 4 (VIDEO_SNAPSHOT) | accepted | yes | `4` | 320 |
| `android.control.captureIntent` | `5` | intent 5 (ZERO_SHUTTER_LAG) | accepted | yes | `5` | 277 |
| `android.control.captureIntent` | `6` | intent 6 (MANUAL) | accepted | yes | `6` | 273 |
| `android.control.captureIntent` | `7` | intent 7 (MOTION_TRACKING) | accepted | yes | `7` | 291 |
| `android.control.effectMode` | `0` | mode 0 | accepted | yes | `0` | 298 |
| `android.control.effectMode` | `1` | mode 1 | accepted | yes | `1` | 292 |
| `android.control.effectMode` | `2` | mode 2 | accepted | yes | `2` | 272 |
| `android.control.effectMode` | `4` | mode 4 | accepted | yes | `4` | 267 |
| `android.control.effectMode` | `5` | mode 5 | accepted | yes | `5` | 266 |
| `android.control.effectMode` | `8` | mode 8 | accepted | yes | `8` | 265 |
| `android.control.mode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 268 |
| `android.control.mode` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 297 |
| `android.control.mode` | `2` | mode 2 (USE_SCENE_MODE) | accepted | yes | `2` | 292 |
| `android.control.postRawSensitivityBoost` | `100` | 100 | accepted | yes | `100` | 276 |
| `android.control.sceneMode` | `1` | mode 1 | accepted | yes | `1` | 295 |
| `android.control.videoStabilizationMode` | `0` | mode 0 | accepted | yes | `0` | 292 |
| `android.control.zoomRatio` | `1` | zoom 1.0 | accepted | yes | `1` | 277 |
| `android.control.zoomRatio` | `2` | zoom 2.0 | accepted | yes | `2` | 378 |
| `android.flash.mode` | `0` | OFF (OFF) | accepted | yes | `0` | 324 |
| `android.jpeg.orientation` | `0` | 0 deg | accepted | yes | `0` | 294 |
| `android.jpeg.orientation` | `90` | 90 deg | accepted | yes | `90` | 271 |
| `android.jpeg.orientation` | `180` | 180 deg | accepted | yes | `180` | 265 |
| `android.jpeg.orientation` | `270` | 270 deg | accepted | yes | `270` | 262 |
| `android.jpeg.quality` | `1` | quality 1 | accepted | yes | `1` | 275 |
| `android.jpeg.quality` | `50` | quality 50 | accepted | yes | `50` | 262 |
| `android.jpeg.quality` | `100` | quality 100 | accepted | yes | `100` | 300 |
| `android.jpeg.thumbnailQuality` | `1` | quality 1 | accepted | yes | `1` | 295 |
| `android.jpeg.thumbnailQuality` | `50` | quality 50 | accepted | yes | `50` | 266 |
| `android.jpeg.thumbnailQuality` | `100` | quality 100 | accepted | yes | `100` | 267 |
| `android.jpeg.thumbnailSize` | `{"height":0,"width":0}` | 0x0 | accepted | yes | `{"height":0,"width":0}` | 285 |
| `android.jpeg.thumbnailSize` | `{"height":384,"width":512}` | 512x384 | accepted | yes | `{"height":384,"width":512}` | 281 |
| `android.lens.aperture` | `2` | 2.0 | accepted | yes | `2` | 270 |
| `android.lens.filterDensity` | `0` | 0.0 | accepted | yes | `0` | 320 |
| `android.lens.focalLength` | `4.6` | 4.6 | accepted | yes | `4.6` | 278 |
| `android.lens.opticalStabilizationMode` | `0` | mode 0 | accepted | yes | `0` | 295 |
| `android.lens.opticalStabilizationMode` | `1` | mode 1 | accepted | yes | `1` | 266 |
| `android.noiseReduction.mode` | `0` | mode 0 | accepted | yes | `0` | 267 |
| `android.noiseReduction.mode` | `1` | mode 1 | accepted | yes | `1` | 269 |
| `android.noiseReduction.mode` | `2` | mode 2 | accepted | yes | `2` | 268 |
| `android.scaler.cropRegion` | `{"bottom":3000,"left":0,"right":4000,"top":0}` | full 4000x3000 | accepted | yes | `{"bottom":3000,"left":0,"right":4000,"top":0}` | 303 |
| `android.scaler.cropRegion` | `{"bottom":2250,"left":1000,"right":3000,"top":750}` | half 2000x1500 | accepted | yes | `{"bottom":2250,"left":1000,"right":3000,"top":750}` | 341 |
| `android.sensor.testPatternMode` | `0` | mode 0 | accepted | yes | `0` | 320 |
| `android.statistics.faceDetectMode` | `0` | mode 0 | accepted | yes | `0` | 269 |
| `android.statistics.faceDetectMode` | `1` | mode 1 | accepted | yes | `1` | 265 |
| `android.tonemap.curve` | `"TonemapCurve{R:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], G:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], B:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]}"` | linear 3-point | accepted | yes | `"TonemapCurve{R:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], G:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], B:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]}"` | 266 |

## Keys not probed

| Key | Status | Reason |
|---|---|---|
| `android.control.aePrecaptureTrigger` | skipped | one-shot 3A trigger; probing it would disturb the live stream |
| `android.control.afTrigger` | skipped | one-shot 3A trigger; probing it would disturb the live stream |
| `android.jpeg.gpsLocation` | skipped | GPS metadata is not a camera capability |

## Known keys not advertised by this camera

- `COLOR_CORRECTION_GAINS` (`android.colorCorrection.gains`)
- `COLOR_CORRECTION_MODE` (`android.colorCorrection.mode`)
- `COLOR_CORRECTION_TRANSFORM` (`android.colorCorrection.transform`)
- `LENS_FOCUS_DISTANCE` (`android.lens.focusDistance`)
- `DISTORTION_CORRECTION_MODE` (`android.distortionCorrection.mode`)
- `EDGE_MODE` (`android.edge.mode`)
- `HOT_PIXEL_MODE` (`android.hotPixel.mode`)
- `SCALER_ROTATE_AND_CROP` (`android.scaler.rotateAndCrop`)
- `SENSOR_EXPOSURE_TIME` (`android.sensor.exposureTime`)
- `SENSOR_FRAME_DURATION` (`android.sensor.frameDuration`)
- `SENSOR_SENSITIVITY` (`android.sensor.sensitivity`)
- `SENSOR_PIXEL_MODE` (`android.sensor.pixelMode`)
- `BLACK_LEVEL_LOCK` (`android.blackLevel.lock`)
- `TONEMAP_GAMMA` (`android.tonemap.gamma`)
- `TONEMAP_MODE` (`android.tonemap.mode`)
- `TONEMAP_PRESET_CURVE` (`android.tonemap.presetCurve`)
- `STATISTICS_HOT_PIXEL_MAP_MODE` (`android.statistics.hotPixelMapMode`)
- `STATISTICS_LENS_SHADING_MAP_MODE` (`android.statistics.lensShadingMapMode`)
- `STATISTICS_OIS_DATA_MODE` (`android.statistics.oisDataMode`)
- `REPROCESS_EFFECTIVE_EXPOSURE_FACTOR` (`android.reprocess.effectiveExposureFactor`)
