# Control compatibility matrix — SM-A515F (Android 13, SDK 33)

- Serial: `192.168.93.76:41573`
- Camera: `1` (front), hardware level LIMITED
- App: `0.1.0-dev+361aa14`
- Request keys: 35 available, 32 probed; result keys: 44 available
- Values: 75 accepted, 2 ignored, 0 error
- Keys: 3 skipped, 0 unprobed
- Aborted: no

`accepted` = the request was submitted and the capture result reflected the value; `ignored` = the request was submitted but the result did not reflect it; `error` = the HAL rejected the request or no result arrived.

| Key | Requested | Label | Outcome | Reflected | Result | ms |
|---|---|---|---|---|---|---|
| `android.colorCorrection.aberrationMode` | `0` | mode 0 | accepted | yes | `0` | 489 |
| `android.control.aeAntibandingMode` | `0` | mode 0 | accepted | yes | `0` | 324 |
| `android.control.aeAntibandingMode` | `3` | mode 3 | accepted | yes | `3` | 284 |
| `android.control.aeExposureCompensation` | `-20` | -20 | ignored | no | `0` | 308 |
| `android.control.aeExposureCompensation` | `0` | 0 | accepted | yes | `0` | 288 |
| `android.control.aeExposureCompensation` | `20` | 20 | ignored | no | `0` | 302 |
| `android.control.aeLock` | `true` | true | accepted | yes | `true` | 298 |
| `android.control.aeLock` | `false` | false | accepted | yes | `false` | 303 |
| `android.control.aeMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 293 |
| `android.control.aeMode` | `1` | mode 1 (ON) | accepted | yes | `1` | 330 |
| `android.control.aeRegions` | `[{"rect":{"bottom":1836,"left":816,"right":2448,"top":612},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":1836,"left":816,"right":2448,"top":612},"weight":1000}]` | 300 |
| `android.control.aeTargetFpsRange` | `[15,15]` | 15-15 fps | accepted | yes | `[15,15]` | 291 |
| `android.control.aeTargetFpsRange` | `[15,20]` | 15-20 fps | accepted | yes | `[15,20]` | 338 |
| `android.control.aeTargetFpsRange` | `[20,20]` | 20-20 fps | accepted | yes | `[20,20]` | 323 |
| `android.control.aeTargetFpsRange` | `[24,24]` | 24-24 fps | accepted | yes | `[24,24]` | 314 |
| `android.control.aeTargetFpsRange` | `[8,30]` | 8-30 fps | accepted | yes | `[8,30]` | 333 |
| `android.control.aeTargetFpsRange` | `[10,30]` | 10-30 fps | accepted | yes | `[10,30]` | 280 |
| `android.control.aeTargetFpsRange` | `[15,30]` | 15-30 fps | accepted | yes | `[15,30]` | 301 |
| `android.control.aeTargetFpsRange` | `[30,30]` | 30-30 fps | accepted | yes | `[30,30]` | 295 |
| `android.control.afMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 303 |
| `android.control.awbLock` | `true` | true | accepted | yes | `true` | 301 |
| `android.control.awbLock` | `false` | false | accepted | yes | `false` | 299 |
| `android.control.awbMode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 294 |
| `android.control.awbMode` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 300 |
| `android.control.awbMode` | `2` | mode 2 (INCANDESCENT) | accepted | yes | `2` | 305 |
| `android.control.awbMode` | `3` | mode 3 (FLUORESCENT) | accepted | yes | `3` | 296 |
| `android.control.awbMode` | `5` | mode 5 (DAYLIGHT) | accepted | yes | `5` | 298 |
| `android.control.awbMode` | `6` | mode 6 (CLOUDY_DAYLIGHT) | accepted | yes | `6` | 299 |
| `android.control.awbRegions` | `[{"rect":{"bottom":1836,"left":816,"right":2448,"top":612},"weight":1000}]` | centre region | accepted | yes | `[{"rect":{"bottom":1836,"left":816,"right":2448,"top":612},"weight":1000}]` | 300 |
| `android.control.captureIntent` | `0` | intent 0 (CUSTOM) | accepted | yes | `0` | 298 |
| `android.control.captureIntent` | `1` | intent 1 (PREVIEW) | accepted | yes | `1` | 304 |
| `android.control.captureIntent` | `2` | intent 2 (STILL_CAPTURE) | accepted | yes | `2` | 288 |
| `android.control.captureIntent` | `3` | intent 3 (VIDEO_RECORD) | accepted | yes | `3` | 311 |
| `android.control.captureIntent` | `4` | intent 4 (VIDEO_SNAPSHOT) | accepted | yes | `4` | 296 |
| `android.control.captureIntent` | `5` | intent 5 (ZERO_SHUTTER_LAG) | accepted | yes | `5` | 293 |
| `android.control.captureIntent` | `6` | intent 6 (MANUAL) | accepted | yes | `6` | 308 |
| `android.control.captureIntent` | `7` | intent 7 (MOTION_TRACKING) | accepted | yes | `7` | 293 |
| `android.control.effectMode` | `0` | mode 0 | accepted | yes | `0` | 304 |
| `android.control.effectMode` | `1` | mode 1 | accepted | yes | `1` | 299 |
| `android.control.effectMode` | `2` | mode 2 | accepted | yes | `2` | 294 |
| `android.control.effectMode` | `4` | mode 4 | accepted | yes | `4` | 308 |
| `android.control.effectMode` | `5` | mode 5 | accepted | yes | `5` | 293 |
| `android.control.effectMode` | `8` | mode 8 | accepted | yes | `8` | 315 |
| `android.control.mode` | `0` | mode 0 (OFF) | accepted | yes | `0` | 284 |
| `android.control.mode` | `1` | mode 1 (AUTO) | accepted | yes | `1` | 307 |
| `android.control.mode` | `2` | mode 2 (USE_SCENE_MODE) | accepted | yes | `2` | 291 |
| `android.control.postRawSensitivityBoost` | `100` | 100 | accepted | yes | `100` | 305 |
| `android.control.sceneMode` | `1` | mode 1 | accepted | yes | `1` | 294 |
| `android.control.videoStabilizationMode` | `0` | mode 0 | accepted | yes | `0` | 290 |
| `android.control.zoomRatio` | `1` | zoom 1.0 | accepted | yes | `1` | 311 |
| `android.control.zoomRatio` | `2` | zoom 2.0 | accepted | yes | `2` | 277 |
| `android.flash.mode` | `0` | OFF (OFF) | accepted | yes | `0` | 290 |
| `android.jpeg.orientation` | `0` | 0 deg | accepted | yes | `0` | 296 |
| `android.jpeg.orientation` | `90` | 90 deg | accepted | yes | `90` | 300 |
| `android.jpeg.orientation` | `180` | 180 deg | accepted | yes | `180` | 310 |
| `android.jpeg.orientation` | `270` | 270 deg | accepted | yes | `270` | 290 |
| `android.jpeg.quality` | `1` | quality 1 | accepted | yes | `1` | 315 |
| `android.jpeg.quality` | `50` | quality 50 | accepted | yes | `50` | 324 |
| `android.jpeg.quality` | `100` | quality 100 | accepted | yes | `100` | 294 |
| `android.jpeg.thumbnailQuality` | `1` | quality 1 | accepted | yes | `1` | 294 |
| `android.jpeg.thumbnailQuality` | `50` | quality 50 | accepted | yes | `50` | 312 |
| `android.jpeg.thumbnailQuality` | `100` | quality 100 | accepted | yes | `100` | 285 |
| `android.jpeg.thumbnailSize` | `{"height":0,"width":0}` | 0x0 | accepted | yes | `{"height":0,"width":0}` | 317 |
| `android.jpeg.thumbnailSize` | `{"height":384,"width":512}` | 512x384 | accepted | yes | `{"height":384,"width":512}` | 282 |
| `android.lens.aperture` | `2.2` | 2.2 | accepted | yes | `2.2` | 324 |
| `android.lens.filterDensity` | `0` | 0.0 | accepted | yes | `0` | 303 |
| `android.lens.focalLength` | `3.72` | 3.72 | accepted | yes | `3.72` | 305 |
| `android.lens.opticalStabilizationMode` | `0` | mode 0 | accepted | yes | `0` | 304 |
| `android.noiseReduction.mode` | `0` | mode 0 | accepted | yes | `0` | 300 |
| `android.noiseReduction.mode` | `1` | mode 1 | accepted | yes | `1` | 288 |
| `android.noiseReduction.mode` | `2` | mode 2 | accepted | yes | `2` | 303 |
| `android.scaler.cropRegion` | `{"bottom":2448,"left":0,"right":3264,"top":0}` | full 3264x2448 | accepted | yes | `{"bottom":2448,"left":0,"right":3264,"top":0}` | 291 |
| `android.scaler.cropRegion` | `{"bottom":1836,"left":816,"right":2448,"top":612}` | half 1632x1224 | accepted | yes | `{"bottom":1836,"left":816,"right":2448,"top":612}` | 279 |
| `android.sensor.testPatternMode` | `0` | mode 0 | accepted | yes | `0` | 285 |
| `android.statistics.faceDetectMode` | `0` | mode 0 | accepted | yes | `0` | 300 |
| `android.statistics.faceDetectMode` | `1` | mode 1 | accepted | yes | `1` | 301 |
| `android.tonemap.curve` | `"TonemapCurve{R:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], G:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], B:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]}"` | linear 3-point | accepted | yes | `"TonemapCurve{R:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], G:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], B:[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]}"` | 296 |

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
- `CONTROL_AF_REGIONS` (`android.control.afRegions`)
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
