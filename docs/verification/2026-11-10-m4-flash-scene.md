# Verification record — 2026-11-10 — M4-T08 flash/torch, scene and image processing

## Artifact under test

- CI run id / workflow: [42825780915](https://github.com/Mohamed-Kilany/watchdog/actions/runs/42825780915) (`ci`, push to `master`)
- Commit SHA: `6937705` (merge of PR #27, on top of the M4-T08 feature merge `99be1f5`)
- Artifacts tested: `watchdog-host-windows-x86_64` (release host with the embedded APK from the same run) and `watchdog-android-apk`
- Host binary SHA-256: `06fcb1ee36aef793026ed006a40a219b98f178dafaa22bd6e86f917ba31b36c8`
- Embedded APK: `0.1.0-dev+6937705` (versionCode 1000), SHA-256
  `d3bcca7a6d4575b35dbe9218a90a8fa0193b181363c3fa3b44a2d68b663df5e8`, signer
  `6934ad7a1307ef1c8ca7481b0ae0e98385e78d4544d0fe2cd212ad3c8f8609d7`
- Which machine ran the test: Windows 10 Pro 10.0.19045, Intel i7-1165G7, 15.7 GB RAM.
  **Exception:** system `adb` 1.0.41 (36.0.0) and `ffmpeg` 7.1.1 are installed on this
  developer machine and `ANDROID_HOME` is set; watchdog never invoked them (`watchdog
  doctor`: "system adb not used"). The no-adb/no-ffmpeg/no-SDK guarantee is covered by the
  `isolation (no system adb, foreign server on 5037)` CI job.
- Was a competing adb server bound to 5037 during the run? yes (the developer machine's
  system adb server was already listening). It was not used by watchdog; the isolation CI
  job covers the clean-machine case.

## Devices used

| # | Model | OEM skin / version | Android SDK | USB | LAN (pairing) | Notes |
|---|---|---|---|---|---|---|
| 1 | Samsung SM-A515F (`a51`) | One UI 5.1 (Android 13) | 33 | deferred | wireless debugging `192.168.93.76:45393` | arm64-v8a, camera id 0, hardware level LIMITED |

Only one physical device was available. USB remains deferred by decision; the run used the
stored wireless-debugging connection.

## Environment

- Host: Windows 10 Pro 10.0.19045; watchdog data dir `C:\Users\user\AppData\Local\watchdog`.
- Network: phone and host on the same LAN; the host connects with its stored ADB key over
  wireless debugging. No USB was used.
- The APK is signed with a per-CI-run key by design; the previous install was uninstalled
  before provisioning this artifact.
- The phone screen was woken, the keyguard dismissed and `screen_off_timeout` set to
  1,800,000 ms before the run. Provisioning also applied its stay-on settings.

## Checklist — M4-T08

- [x] **Torch** is advertised (`{"supported":true}`) and `torch=true` drives
      `FLASH_MODE_TORCH`; the reported actuals follow: `torch=true`, `flash_mode=2`.
      `torch=false` returns `torch=false`, `flash_mode=0`.
- [x] **Flash mode** modes `[0,1,2]` are advertised. All three are accepted; `flash_mode=0`
      and `2` report the matching actual, and `flash_mode=1` (SINGLE) is retained as
      requested and reported as actual `1` on this HAL. The descriptor gates the control.
- [x] **Scene mode** lists only `[1]` (FACE_PRIORITY); it applies and reports actual `1`.
      Unavailable `scene_mode=0` is rejected with `OUT_OF_RANGE`.
- [x] **Noise reduction** modes `[0,1,2]` all apply and the actual follows each mode;
      unavailable mode `3` is rejected with `OUT_OF_RANGE`.
- [x] **Edge enhancement** modes `[0,1,2]` all apply and the actual follows each mode. The
      A51 exposes every mode in the probe range, so no unavailable mode remains to reject.
- [x] **Hot pixel** modes `[0,1,2]` all apply and the actual follows each mode.
- [x] **Tonemap mode** modes `[0,1,2]` all apply and the actual follows each mode;
      unavailable mode `3` is rejected with `OUT_OF_RANGE`.
- [x] **Contrast curve** is advertised with `max_points=128`. A six-point curve per channel
      is accepted, `tonemap=CONTRAST_CURVE` (mode `0`) is implied in the same batch, and the
      reported `tonemap_curve` contains three channels of six points in `[0,1]`. A malformed
      value is rejected with `INVALID_VALUE`.
- [x] **Gamma value** and **preset curve** are not exposed by this camera (no matching
      `TONEMAP_MODE`); both are listed under `unsupported` and rejected with `UNSUPPORTED`.
      Their validation and application paths are covered by JVM unit tests.
- [x] **Lens shading** modes `[0,1,2]` all apply and the actual follows each mode;
      unavailable mode `3` is rejected with `OUT_OF_RANGE`.
- [x] **Face detection** modes `[0,1]` both apply. `face_count` and `faces` readouts are
      present; with no face in view the run reports `face_count=0`, `faces=[]`. Boxes are
      normalized data only; the host overlay is M6-T04.
- [x] **Test pattern** lists only `[0]` (OFF); it applies and reports actual `0`.
      Unavailable mode `1` is rejected with `OUT_OF_RANGE`.
- [x] Every control is gated by the capability descriptor; unsupported ids are rejected
      before they reach the camera.
- [x] Clean restore: `torch=false`, `flash_mode=0`, `scene_mode=1`, `noise_reduction=0`,
      `edge_enhancement=0`, `hot_pixel=0`, `tonemap=1`, `lens_shading=0`, `face_detect=0`,
      `test_pattern=0`; the final snapshot reports the same values.
- [x] No transient `camera thread unavailable` replies and no `STREAM_CLOSED` retries in the
      whole run. The app log shows the camera `STREAMING` from startup and no
      `control send queue overflow`, `RECOVERING` or `CAMERA_IN_USE` entries.

## Results and evidence

### Provisioning

```
{
  "serial": "192.168.93.76:45393",
  "steps": [
    {"name":"device","status":"passed","duration_ms":375,
     "detail":"samsung SM-A515F (a51) — Android 13 (SDK 33), arm64-v8a"},
    {"name":"app","status":"passed","duration_ms":4924,
     "detail":"installed 0.1.0-dev+6937705 (versionCode 1000)"},
    {"name":"permissions","status":"passed","duration_ms":1328,
     "detail":"granted CAMERA, RECORD_AUDIO, POST_NOTIFICATIONS; appops allow CAMERA, RECORD_AUDIO, WAKE_LOCK, START_FOREGROUND, RUN_ANY_IN_BACKGROUND, POST_NOTIFICATION"},
    {"name":"battery","status":"passed","duration_ms":608,
     "detail":"Doze whitelist + active standby bucket (device reports 5); warnings: standby bucket is 5, expected 10 (active)"},
    {"name":"screen","status":"passed","duration_ms":484,
     "detail":"stay_on_while_plugged_in 7 -> 7 (original recorded for restore); screen awake"},
    {"name":"launch","status":"passed","duration_ms":586,
     "detail":"activity foregrounded; capture service started with a fresh session token"},
    {"name":"verify","status":"passed","duration_ms":1116,
     "detail":"handshake ok (app 0.1.0-dev+6937705 versionCode 1000, protocol 1); first frame 1280x720 JPEG (128239 bytes)"}
  ],
  "token": "e012f4e3a8ad1d36eabe104d7ee71d38"
}
```

The full report is [t08-provision-output.txt](2026-11-10-a51/t08-provision-output.txt).

### Capability descriptor (camera 0, T08 entries)

```
torch:                 {"supported": true}
flash_mode:            {"modes": [0, 1, 2], "supported": true}
scene_mode:            {"modes": [1], "supported": true}
noise_reduction:       {"modes": [0, 1, 2], "supported": true}
edge_enhancement:      {"modes": [0, 1, 2], "supported": true}
hot_pixel:             {"modes": [0, 1, 2], "supported": true}
tonemap:               {"modes": [0, 1, 2], "supported": true}
tonemap_curve:         {"max_points": 128, "supported": true}
tonemap_gamma:         null
tonemap_preset_curve:  null
lens_shading:          {"modes": [0, 1, 2], "supported": true}
face_detect:           {"modes": [0, 1], "supported": true}
test_pattern:          {"modes": [0], "supported": true}
unsupported: ['awb_gains', 'color_transform', 'distortion_correction',
              'filter_density', 'tonemap_gamma', 'tonemap_preset_curve']
```

The full descriptor is [t08-capabilities.json](2026-11-10-a51/t08-capabilities.json).

### Baseline live state

`torch=false`, `flash_mode=0`, `scene_mode=1`, `noise_reduction=1`,
`edge_enhancement=1`, `hot_pixel=1`, `tonemap=1`, `lens_shading=1`, `face_detect=0`,
`test_pattern=0`, `face_count=0`, `faces=[]`.

### Command pipeline (M4-T08)

The full transcript is [t08-output.txt](2026-11-10-a51/t08-output.txt).

| Command | Round-trip | Result |
|---|---|---|
| `scene_mode=1` | 59 ms | `ok applied=1`; actual 1 |
| `scene_mode=0` | 38 ms | `FAIL OUT_OF_RANGE mode 0 is not available for 'scene_mode'` |
| `noise_reduction=0,1,2` | 33–147 ms | all `ok`; actual follows each mode |
| `noise_reduction=3` | 22 ms | `FAIL OUT_OF_RANGE mode 3 is not available for 'noise_reduction'` |
| `edge_enhancement=0,1,2` | 55–81 ms | all `ok`; actual follows each mode |
| `hot_pixel=0,1,2` | 57–206 ms | all `ok`; actual follows each mode |
| `lens_shading=0,1,2` | 35–74 ms | all `ok`; actual follows each mode |
| `lens_shading=3` | 48 ms | `FAIL OUT_OF_RANGE mode 3 is not available for 'lens_shading'` |
| `face_detect=0,1` | 38–40 ms | both `ok`; actual follows each mode |
| `face_detect=2` | 58 ms | `FAIL OUT_OF_RANGE mode 2 is not available for 'face_detect'` |
| `test_pattern=0` | 43 ms | `ok applied=0`; actual 0 |
| `test_pattern=1` | 67 ms | `FAIL OUT_OF_RANGE mode 1 is not available for 'test_pattern'` |
| `flash_mode=0,1,2` | 23–62 ms | all `ok`; actual 0, 1, 2 |
| `torch=true` | 57 ms | `ok applied=true`; `torch=true`, `flash_mode=2` |
| `torch=false` | 56 ms | `ok applied=false`; `torch=false`, `flash_mode=0` |
| `tonemap=0,1,2` | 38–86 ms | all `ok`; actual follows each mode |
| `tonemap=3` | 218 ms | `FAIL OUT_OF_RANGE mode 3 is not available for 'tonemap'` |
| `tonemap_curve={red,green,blue}` | 104 ms | `ok`; implies `tonemap=0`; actual has 6 points per channel |
| malformed `tonemap_curve=1` | 27 ms | `FAIL INVALID_VALUE tonemap_curve expects an object with red, green and blue arrays` |
| `tonemap_gamma=2.2` | 29 ms | `FAIL UNSUPPORTED control 'tonemap_gamma' is not supported by this camera` |
| `tonemap_preset_curve=0` | 30 ms | `FAIL UNSUPPORTED control 'tonemap_preset_curve' is not supported by this camera` |
| Restore batch | 15 ms | `torch`, `flash_mode`, `scene_mode`, `noise_reduction`, `edge_enhancement`, `hot_pixel`, `tonemap`, `lens_shading`, `face_detect`, `test_pattern` all `ok` |
| Transient-error check | — | 0 `camera thread unavailable`, 0 retries |

### Contrast curve

The requested curve was `[0.0, 0.0, 0.5, 0.5, 1.0, 1.0]` for each channel. The implied
`tonemap` result was `{"applied":0,"control":"tonemap","ok":true}` — mode `0` is
`CONTRAST_CURVE` in Camera2 — and the reported `tonemap_curve` was:

```
{"red":[0,0,0.5,0.5,1,1],"green":[0,0,0.5,0.5,1,1],"blue":[0,0,0.5,0.5,1,1]}
```

### Face readouts

`face_detect=1` was accepted. The state stream reported `face_count=0` and `faces=[]`
because no face was in view; the readout keys were present and correctly typed. The
normalized-box format is covered by unit tests and the host overlay is M6-T04.

### Restored live state

`flash_mode=0`, `scene_mode=1`, `noise_reduction=0`, `edge_enhancement=0`, `hot_pixel=0`,
`tonemap=1`, `lens_shading=0`, `face_detect=0`, `test_pattern=0`, `face_count=0`.

### App log / transport stability

The app log is [t08-app-logs.txt](2026-11-10-a51/t08-app-logs.txt). It covers 78 host
sessions from the T08 run and shows:

- camera `OPEN → CONFIGURED → STREAMING` at startup and no later recovery;
- no `control send queue overflow` / `send_queue_overflow`;
- no `RECOVERING` or `CAMERA_IN_USE` entries;
- normal `host_closed` session teardown for every CLI command.

## Failures found (do not hide)

| # | Severity | Description | Issue link | Workaround |
|---|---|---|---|---|
| 1 | medium | The first T08 run (36/48, artifact `99be1f5`) reported no `flash_mode` or `torch` actuals: `CameraControls.extract` set `FLASH_MODE` requests but never read `CaptureResult.FLASH_MODE`. Fixed in PR #27 by reading the result key and deriving the `torch` actual. | PR #27 | re-run after the fix |
| 2 | medium | The first run hit six `STREAM_CLOSED` failures during rapid control commands. The app closed sessions with `send_queue_overflow` because preview frames filled the 8-slot outbound queue and starved `STATE`/`COMMAND_RESULT`. Fixed in PR #27: control packets evict the oldest queued `FRAME`/`LOG_CHUNK` before falling back to the timed wait. | PR #27 | re-run after the fix; the new run had 0 retries |
| 3 | low | The first run's device harness expected `tonemap_curve` to imply mode `1`. Camera2 defines `CONTRAST_CURVE` as mode `0`; the product behaviour was correct. The harness expectation was corrected to `0` (`GAMMA_VALUE=3`, `PRESET_CURVE=4`). | n/a (test harness) | corrected harness; unit tests assert the shared constants |

The first run's 12 failures map exactly to items 1–3; after the two product fixes and the
harness correction, the same test suite passed 48/48 with 0 retries.

## Verdict

- Release gate: **PASS** for M4-T08.
- What was explicitly NOT verified: gamma-value and preset-curve controls on a physical
  device (the A51 exposes only `[0,1,2]`, so both are rejected as `UNSUPPORTED`; their
  validation/application paths are covered by JVM unit tests); face boxes with a real face
  (no face was in view — only the readout structure and normalization tests); the host
  overlay rendering (M6-T04); physical camera selection (M4-T09); the per-device
  compatibility matrix (M4-T10); the on-device grouped T08 panel (M4-T13); instrumented
  emulator tests (manual-only workflow).
- Sign-off: Mohamed-Kilany, 2026-11-10.
