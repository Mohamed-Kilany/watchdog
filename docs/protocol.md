# watchdog host ↔ watchdog app — wire protocol

Version: **1** (`PROTOCOL_VERSION = 1`)
Status: normative for both the Rust host (`crates/protocol`) and the Kotlin app
(`android/app`). Any change to framing or message semantics is a protocol bump.

Sources of truth, in order: this document → `crates/protocol` → golden vectors
in `protocol/vectors/`. All three must change together.

---

## 1. Transport model

- One TCP-like byte stream per session. Today: an **adb-forwarded** abstract
  socket (`localabstract:watchdog-<session>`) to the app's transport server.
  Over USB this tunnels through ADB; over LAN it goes over the wireless ADB
  connection. The protocol itself is transport-agnostic.
- Endianness: **little-endian** everywhere.
- Integers: unsigned unless noted.
- The host initiates; the app accepts at most **one** host session. A second
  connect from a "new" host (different `session_token` in `HOST_HELLO`) takes
  over; the old session is closed with `BYE { reason: "session_replaced" }`.
- The stream carries both directions multiplexed by message type; there is no
  request/response pairing at the framing layer (that is `id` in JSON payloads).

## 2. Framing

Every message on the wire is one **frame**:

```
offset  size  field
0       4     magic = bytes 57 44 47 31   (ASCII "WDG1", in this order)
4       1     protocol_version (currently 1)
5       1     message type (see §4)
6       1     flags (bit meanings per message type; 0 otherwise)
7       1     reserved, must be 0
8       4     payload_len, u32 LE, 0..=67108864 (64 MiB)
12      4     crc32 (IEEE, polynomial 0xEDB88320, init 0xFFFFFFFF, xorout 0xFFFFFFFF) of payload
16      N     payload
```

Header size: 16 bytes. Maximum frame: 64 MiB + 16 bytes. Implementations must
reject `magic` mismatch, `protocol_version != 1`, `payload_len > 64 MiB` and
CRC mismatch by closing the stream and logging; they must not parse the payload.

`decoder` behavior (normative): garbage bytes must never panic; an invalid
frame is reported as `ProtocolError` and the connection is dropped.

## 3. Version negotiation

1. Host connects, sends **`HOST_HELLO`** (payload JSON):
   `{ "protocol_min": 1, "protocol_max": 1, "host": {"name": "...", "version": "x.y.z"}, "session_token": "<random 128-bit hex>" }`
2. App picks `v = clamp(min(host_max, app_max), app_min?, app_max)` — simply the
   highest version both support. For v1 both are 1.
3. App replies **`HELLO_ACK`**: `{ "protocol_version": v, "app": {"version_code": int, "version_name": "..."}, "device": {...}, "capabilities": {...} }`.
4. If no common version exists, the app sends **`ERROR`** with
   `code = "PROTOCOL_VERSION_MISMATCH"` then **`BYE`**, and closes.

`HOST_HELLO` must be the first frame; anything else is a protocol error.

## 4. Message types

| type | direction   | name             | payload |
|------|-------------|------------------|---------|
| 0x01 | host → app  | `HOST_HELLO`     | JSON (§5.1) |
| 0x02 | app → host  | `HELLO_ACK`      | JSON (§5.2) |
| 0x10 | app → host  | `FRAME`          | binary (§5.3) |
| 0x20 | app → host  | `STATE`          | JSON (§5.4); `flags & 0x01` = delta |
| 0x21 | host → app  | `COMMAND`        | JSON (§5.5) |
| 0x22 | app → host  | `COMMAND_RESULT` | JSON (§5.6) |
| 0x30 | app → host  | `EVENT`          | JSON (§5.7) |
| 0x40 | app → host  | `MEDIA_MANIFEST` | JSON (§5.8) |
| 0x50 | app → host  | `LOG_CHUNK`      | binary (§5.9) |
| 0x60 | app → host  | `ERROR`          | JSON (§5.10) |
| 0x70 | host → app  | `PING`           | JSON (§5.11) |
| 0x71 | app → host  | `PONG`           | JSON (§5.11) |
| 0x7F | either      | `BYE`            | JSON (§5.12) |

Unrecognized type: close the stream (future versions may add types; that is
what `protocol_version` negotiation is for).

## 5. Payloads

### 5.1 HOST_HELLO

```json
{
  "protocol_min": 1,
  "protocol_max": 1,
  "host": { "name": "watchdog", "version": "0.1.0" },
  "session_token": "00112233445566778899aabbccddeeff"
}
```

### 5.2 HELLO_ACK

```json
{
  "protocol_version": 1,
  "app": { "version_code": 42, "version_name": "0.1.0", "package": "app.watchdog.remote" },
  "device": {
    "manufacturer": "Google", "model": "Pixel 7", "device": "panther",
    "sdk": 34, "release": "14", "abi": "arm64-v8a",
    "battery_percent": 87, "storage_free_mb": 104857600, "temperature_c": 29.5
  },
  "cameras": [
    { "id": "0", "facing": "back", "hardware_level": "FULL", "logical_multi_camera": true, "physical_ids": ["3", "4"] }
  ],
  "capabilities": { "1": { /* capability descriptor, §5.13 */ } }
}
```

`capabilities` maps camera id → descriptor (§5.13). The descriptor can be
large; that is allowed (it is sent once per connection and on
`COMMAND{command:"get_capabilities"}`).

### 5.3 FRAME — live viewfinder image

Binary payload, fixed 28-byte header + JPEG bytes:

```
offset  size  field
0       8     seq, u64 LE — monotonic per session, gap = dropped frame
8       8     sensor_timestamp_ns, u64 LE — CLOCK_BOOTTIME domain
16      4     width, u32 LE — actual JPEG pixel width
20      4     height, u32 LE
24      2     rotation, u16 LE — 0, 90, 180 or 270 (degrees CW, display hint for the JPEG)
26      1     flags — bit0: jpeg_color_transform present; bit1: is_snapshot
27      1     reserved = 0
28      ...   JPEG file bytes (JFIF/EXIF)
```

JPEG bytes are exactly one complete JPEG (FF D8 … FF D9). Hosts must tolerate
missing/corrupt frames by dropping them, never by blocking the pipeline.

### 5.4 STATE — camera/control state

`flags & 0x01` ⇒ **delta** (only changed controls); otherwise **full** snapshot.
Rate-limited to 10 Hz by the app unless `set_state_rate` was negotiated.

```json
{
  "seq": 1543,
  "timestamp_ms": 1730900000123,
  "camera_id": "0",
  "state": {
    "iso":            { "requested": 400,  "actual": 398,  "locked": true,  "supported": true, "source": "host" },
    "exposure_time_ns":{ "requested": 33333333, "actual": 33333333, "locked": true, "supported": true, "source": "host" },
    "awb_lock":       { "requested": true, "actual": true, "supported": true, "source": "host" },
    "fps_actual":     { "actual": 29.97, "supported": true, "source": "device" },
    "recording":      { "actual": false, "supported": true, "source": "device" },
    "battery_percent":{ "actual": 87, "supported": true, "source": "device" }
  },
  "dropped_frames": 3,
  "storage_free_mb": 104857600
}
```

Per-control object keys: `requested` (optional), `actual`, `locked`
(optional), `supported` (bool), `source` (`"host" | "device" | "auto"`).
Delta frames carry the same shape with only changed entries; a full snapshot
is always sent on connect, on reconnect and on request
(`COMMAND{get_state}`). "Last writer wins": any writer may change any
control; `actual` always reflects the camera's `TotalCaptureResult`.

`seq` increases monotonically; `camera_id`, `dropped_frames` and
`storage_free_mb` are repeated in every frame (delta or full) so a host can
render without waiting for the next full snapshot. The app sends a full frame
when a session opens, when the camera reaches `STREAMING`, and on
`get_state`; otherwise a 10 Hz timer emits a delta only when at least one
entry changed (`set_state_rate` accepts 1–30 Hz).

Control ids in v1 (the capability descriptor is authoritative for support,
modes and ranges; unsupported ids are never applied):

| id | Type | Camera2 mapping |
|---|---|---|
| `control_mode` | int | `CONTROL_MODE` |
| `ae_mode` | int | `CONTROL_AE_MODE` |
| `af_mode` | int | `CONTROL_AF_MODE` |
| `awb_mode` | int | `CONTROL_AWB_MODE` |
| `anti_banding` | int | `CONTROL_AE_ANTIBANDING_MODE` |
| `scene_mode` | int | `CONTROL_SCENE_MODE` |
| `effect` | int | `CONTROL_EFFECT_MODE` |
| `noise_reduction` | int | `NOISE_REDUCTION_MODE` |
| `edge_enhancement` | int | `EDGE_MODE` |
| `hot_pixel` | int | `HOT_PIXEL_MODE` |
| `tonemap` | int | `TONEMAP_MODE` |
| `tonemap_curve` | `{red:[…],green:[…],blue:[…]}` | `TONEMAP_CURVE`; flat `[in,out,…]` points, 2..`max_points` per channel, inputs increasing, values in `[0,1]`; implies `tonemap = CONTRAST_CURVE` |
| `tonemap_gamma` | double | `TONEMAP_GAMMA`; implies `tonemap = GAMMA_VALUE` |
| `tonemap_preset_curve` | int | `TONEMAP_PRESET_CURVE` (`0` = sRGB, `1` = Rec.709); implies `tonemap = PRESET_CURVE` |
| `test_pattern` | int | `SENSOR_TEST_PATTERN_MODE`; modes come from the descriptor |
| `aberration_correction` | int | `COLOR_CORRECTION_ABERRATION_MODE` |
| `video_stabilization` | int | `CONTROL_VIDEO_STABILIZATION_MODE` |
| `optical_stabilization` | int | `LENS_OPTICAL_STABILIZATION_MODE` |
| `lens_shading` | int | `SHADING_MODE` |
| `face_detect` | int | `STATISTICS_FACE_DETECT_MODE` |
| `ae_lock` / `awb_lock` | bool | `CONTROL_AE_LOCK` / `CONTROL_AWB_LOCK` |
| `iso` | int | `SENSOR_SENSITIVITY` |
| `exposure_time_ns` | long | `SENSOR_EXPOSURE_TIME` |
| `frame_duration_ns` | long | `SENSOR_FRAME_DURATION` |
| `ae_compensation` | int (steps) | `CONTROL_AE_EXPOSURE_COMPENSATION`; the descriptor's `ev_step` is the EV per step |
| `ae_fps_range` | `[min,max]` int pair | `CONTROL_AE_TARGET_FPS_RANGE`; must be one of the descriptor's `ranges` |
| `ae_regions` | array of `{x,y,w,h,weight}` | `CONTROL_AE_REGIONS`; normalized to the active array, 1..`max_regions`, weight −1000..1000 (default 1000) |
| `ae_precapture` | `"start"` / `"cancel"` | one-shot `CONTROL_AE_PRECAPTURE_TRIGGER`; never stored as requested state |
| `af_lock` | bool | one-shot AF via `CONTROL_AF_TRIGGER`; `af_state` and `actual` report the result |
| `af_regions` | array of `{x,y,w,h,weight}` | `CONTROL_AF_REGIONS`; same normalization as `ae_regions`, 1..`max_regions` |
| `af_trigger` | `"start"` / `"cancel"` | one-shot `CONTROL_AF_TRIGGER`; never stored as requested state |
| `focus_distance` | double (dioptres) | `LENS_FOCUS_DISTANCE` with `CONTROL_AF_MODE = OFF`; 0 = infinity, `max` = closest focus |
| `awb_regions` | array of `{x,y,w,h,weight}` | `CONTROL_AWB_REGIONS`; same normalization as `ae_regions` |
| `awb_gains` | `{r,g_even,g_odd,b}` | `COLOR_CORRECTION_GAINS` with `COLOR_CORRECTION_MODE = TRANSFORM_MATRIX`; components use the descriptor's `min`..`max` |
| `color_transform` | array of 9 numbers | `COLOR_CORRECTION_TRANSFORM` (row-major 3×3, sent as rationals); same implications as `awb_gains` |
| `flash_mode` | int | `FLASH_MODE` |
| `torch` | bool | `FLASH_MODE` (`TORCH` / `OFF`) |
| `zoom_ratio` | double | `CONTROL_ZOOM_RATIO` when the camera reports a zoom range (API 30+), otherwise a centred `SCALER_CROP_REGION`; the descriptor's `legacy_crop_region` selects the path |
| `filter_density` | double, one of `values` | `LENS_FILTER_DENSITY`; only advertised when the lens lists an adjustable ND choice |
| `distortion_correction` | int | `DISTORTION_CORRECTION_MODE` (API 33); modes come from the descriptor |

Read-only entries in `STATE` (never accepted in `set_controls`):
`fps_actual` (double), `recording` (bool), `battery_percent` (int),
`ae_state` / `af_state` / `awb_state` (strings), `focus_distance_diopters`
(double; also reported as the `focus_distance` actual), `awb_gains`
(`{r,g_even,g_odd,b}`), `color_transform` (row-major 3×3 matrix),
`zoom_actual` (double), `lens_focal_length` (mm), `lens_aperture` (f-number),
`lens_filter_density`, `lens_state` (`stationary` / `moving`), `face_count`
(int) and `faces` (array of `{id,score,x,y,w,h}` normalized to the active
array). The `tonemap_curve`, `tonemap_gamma` and `tonemap_preset_curve`
controls also carry the value the device used in their `actual`.

### 5.5 COMMAND — host → app

```json
{ "id": "c-000123", "command": "set_controls", "params": { "controls": [ { "id": "iso", "value": 400 } ] } }
```

`set_controls` validates every assignment against the capability descriptor and
applies the accepted subset atomically in the next repeating request. The
`COMMAND_RESULT` carries one entry per assignment (`applied` or `error_code` +
`message`); rejected controls do not block accepted siblings. Assignments are
processed in wire order, so duplicate ids resolve last-writer-wins. Setting
`iso`, `exposure_time_ns` or `frame_duration_ns` implies `ae_mode = OFF` in the
same batch when AE is supported, and the implicit change is reported as an
additional result. `ae_precapture` is a one-shot trigger: it is applied as a
single capture on top of the repeating request and is not retained as requested
state. Setting `focus_distance` implies `af_mode = OFF` and clears `af_lock`;
setting a tonemap parameter (`tonemap_curve`, `tonemap_gamma` or
`tonemap_preset_curve`) implies its matching `tonemap` mode in the same batch.
`af_lock = true` switches `af_mode` to `AUTO`, starts a one-shot AF scan and
remembers the previous mode, while `af_lock = false` restores it and sends
`af_trigger = cancel`. `af_regions` only positions the AF metering window; pair
it with `af_trigger = start` for tap-to-focus. Setting `awb_gains` or
`color_transform` implies `awb_mode = OFF`, clears `awb_lock` when set and
switches `COLOR_CORRECTION_MODE` to `TRANSFORM_MATRIX` in the same batch; the
implicit changes are reported as additional results.

Commands in v1: `set_controls`, `get_state`, `get_capabilities`,
`capture_still`, `capture_burst` (`{count, interval_ms, from_stream}`),
`record_start` (`{max_duration_secs?, segment_secs?, bitrate_bps?, codec?, size?}`),
`record_stop`, `set_state_rate` (`{hz}`), `ping`, `bye`.
Unknown command → `COMMAND_RESULT` with per-command error (or `ERROR` with
`code = "INVALID_REQUEST"` if the frame itself is malformed).

### 5.6 COMMAND_RESULT — app → host

```json
{
  "id": "c-000123",
  "timestamp_ms": 1730900000456,
  "results": [ { "control": "iso", "ok": true, "applied": 398 } ],
  "error": null
}
```

On failure: `{"id": "c-000123", "results": [{"control":"iso","ok":false,"error_code":"OUT_OF_RANGE","message":"ISO 51200 not supported (max 12800)"}]}`.

### 5.7 EVENT — asynchronous app → host notification

```json
{ "kind": "camera_disconnected", "timestamp_ms": 1730900001000, "data": { "camera_id": "0" } }
```

Kinds (v1, extensible): `camera_disconnected`, `camera_reopened`,
`recording_started`, `recording_stopped`, `storage_low`, `battery_low`,
`app_background`, `error`.

### 5.8 MEDIA_MANIFEST — an item is ready to pull

```json
{
  "item": {
    "id": "m-01J…ULID",
    "type": "video",
    "created_ms": 1730900000000,
    "duration_ms": 10042,
    "width": 1920, "height": 1080,
    "bytes": 15204301,
    "sha256": "9f1c…",
    "device_path": "files/media/m-01J….mp4",
    "mime": "video/mp4",
    "control_snapshot": { "iso": 400, "exposure_time_ns": 33333333 },
    "device": { "model": "Pixel 7", "sdk": 34 },
    "host_job_id": null
  }
}
```

The host pulls the file with adb **sync** (preferred) or a socket stream,
verifies `bytes` + `sha256`, then (config) deletes it from the device.

### 5.9 LOG_CHUNK

Binary: `ts_ms u64 LE | level u8 (0=trace,1=debug,2=info,3=warn,4=error) | UTF-8 text`.
Text may contain embedded newlines; it is raw, unredacted device log.

### 5.10 ERROR

```json
{ "code": "PROTOCOL_VERSION_MISMATCH", "message": "no common protocol version (host max 1, app min 2)", "hint": "upgrade watchdog on the host" }
```

`code` is a stable screaming-snake string. Same taxonomy as the host error
model (`docs/api.md`); app-side codes are a subset.

### 5.11 PING / PONG

```json
{ "ts_ms": 1730900002000, "nonce": "a1b2c3d4e5f6a7b8" }
```

Used for liveness (1 Hz default) and for glass-to-glass latency estimation.
`PONG` echoes `nonce` and adds the app's boottime (`app_ts_ns`). The host
estimates clock offset from the round trip; it is an estimate, not sync.

### 5.12 BYE

```json
{ "reason": "host_shutdown | session_replaced | app_shutdown | error", "detail": "optional" }
```

### 5.13 Capability descriptor (embedded in HELLO_ACK / get_capabilities)

`capabilities` maps camera id → descriptor. The app builds it from
`CameraCharacteristics` without opening the camera, so it is the contract for
every control the host may send. Example:

```json
{
  "hardware_level": "LEVEL_3",
  "lens_facing": "back",
  "sensor_orientation": 90,
  "capabilities": ["BACKWARD_COMPATIBLE", "MANUAL_SENSOR", "RAW"],
  "physical_ids": [],
  "lens": {"focal_lengths": [4.3], "apertures": [1.8], "filter_densities": [0.0],
           "focus_distance_calibration": "calibrated",
           "optical_zoom": {"min": 1.0, "max": 1.0},
           "hyperfocal_distance": 2.3, "minimum_focus_distance": 0.1},
  "control_mode": {"supported": true, "modes": [0, 1]},
  "ae_mode": {"supported": true, "modes": [0, 1, 2]},
  "iso": {"supported": true, "min": 50, "max": 12800, "step": 1},
  "exposure_time_ns": {"supported": true, "min": 1000, "max": 1000000000, "step": 1},
  "ae_fps_range": {"supported": true, "ranges": [[15, 30], [30, 30]]},
  "ae_regions": {"supported": true, "max_regions": 5},
  "ae_precapture": {"supported": true},
  "af_mode": {"supported": true, "modes": [0, 1, 3, 4]},
  "af_lock": {"supported": true},
  "af_regions": {"supported": true, "max_regions": 1},
  "af_trigger": {"supported": true},
  "focus_distance": {"supported": true, "min": 0.0, "max": 10.0, "step": 0.0},
  "zoom_ratio": {"supported": true, "min": 1.0, "max": 8.0, "step": 0.0,
                 "legacy_crop_region": false},
  "filter_density": {"supported": true, "values": [0.0, 0.5, 1.0]},
  "distortion_correction": {"supported": true, "modes": [0, 1, 2]},
  "tonemap": {"supported": true, "modes": [0, 1, 2]},
  "tonemap_curve": {"supported": true, "max_points": 64},
  "tonemap_gamma": {"supported": true, "min": 0.1, "max": 5.0, "step": 0.01},
  "tonemap_preset_curve": {"supported": true, "modes": [0, 1]},
  "test_pattern": {"supported": true, "modes": [0, 1, 2]},
  "face_detect": {"supported": true, "modes": [0, 1, 2]},
  "streams": {
    "available": true,
    "formats": {"YUV_420_888": [{"width": 1920, "height": 1080}],
                "JPEG": [{"width": 4032, "height": 3024}]},
    "fps_ranges": [[15, 30], [30, 30]],
    "high_speed_video": {"sizes": [], "fps_ranges": []}
  },
  "available_request_keys": [{"name": "android.sensor.iso", "type": "java.lang.Integer"}],
  "available_result_keys": [{"name": "android.sensor.iso", "type": "java.lang.Integer"}],
  "available_characteristics_keys": [
    {"name": "android.sensor.info.sensitivityRange", "type": "android.util.Range"}
  ],
  "unsupported": [{"control": "torch", "reason": "no flash unit"}]
}
```

- Control objects are either `{"supported": true, "modes": [...]}`,
  `{"supported": true, "min": …, "max": …, "step": …}` (numbers; `step: 0`
  means continuous) or, for discrete numeric choices such as ND filter
  densities, `{"supported": true, "values": […]}`. `supported: false` and
  every entry in `unsupported` mean the host must not send that control.
- `ae_fps_range` carries `ranges`, the allowed `[min,max]` FPS pairs;
  `ae_regions` carries `max_regions` (1..N normalized rectangles);
  `ae_precapture` is a one-shot trigger. `af_regions` carries `max_regions`
  too; `af_trigger` and `af_lock` are one-shot/boolean controls; `focus_distance`
  is a continuous range in dioptres (0 = infinity).
- Key inventories are the complete dump of the camera's available
  `CaptureRequest` / `CaptureResult` / `CameraCharacteristics` keys:
  `name` is the dotted Android key and `type` its Java class. Full dump, no
  silent drops; unknown/absent entries mean "unsupported", never "ignored".
- A camera that disappears between enumeration and introspection appears as
  `{"error": "<message>"}` instead of a partial descriptor.

## 6. Checksum

CRC-32 IEEE (a.k.a. CRC-32/ISO-HDLC, as in Ethernet/PNG/zip): poly `0xEDB88320`
reflected, init `0xFFFFFFFF`, refin/reffout true, xorout `0xFFFFFFFF`.
Reference: `crc32fast` crate; Kotlin: `java.util.zip.CRC32` — identical parameters.

## 7. Golden vectors

`protocol/vectors/NNNN-<name>.bin` — one complete frame each — plus
`manifest.json`:

```json
{
  "version": 1,
  "vectors": [ { "file": "0001-host-hello.bin", "message_type": "HOST_HELLO", "decode": { ... } } ]
}
```

Rules: both implementations must (a) decode every vector to exactly the
documented payload, (b) re-encode to the exact same bytes, (c) reject every
`invalid-*` vector without panicking. Regenerate with
`cargo run -p watchdog-protocol --example generate-vectors`; CI diffs.

## 8. Limits and backpressure

- `payload_len` ≤ 64 MiB (a 4K JPEG is ~10 MiB; manifests are KBs).
- Sender must honor transport backpressure: `FRAME` is dropped (never queued
  unboundedly) when the app's send queue is full — the host sees seq gaps.
  Control messages evict queued `FRAME`/`LOG_CHUNK` packets first, so a video
  burst cannot starve `STATE` or `COMMAND_RESULT`.
- `STATE` is rate-limited (default 10 Hz, max 30 Hz) and diff-compressed.
- `COMMAND`s are processed in order; a slow command never blocks `STATE`/`FRAME`.

## 9. Versioning rules

- v1 is frozen once shipped. New fields in JSON payloads are allowed only if
  old readers can ignore them (readers must tolerate unknown JSON keys).
- Removing/renaming a field, changing framing, adding a required field ⇒
  `protocol_version` bump and a new section in this file.
- Error `code` strings are additive-only.
