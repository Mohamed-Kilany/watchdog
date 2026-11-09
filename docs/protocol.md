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
| `flash_mode` | int | `FLASH_MODE` |
| `torch` | bool | `FLASH_MODE` (`TORCH` / `OFF`) |
| `zoom_ratio` | double | `CONTROL_ZOOM_RATIO` (API 30+) or `SCALER_CROP_REGION` fallback |

Read-only entries in `STATE` (never accepted in `set_controls`):
`fps_actual` (double), `recording` (bool), `battery_percent` (int),
`ae_state` / `af_state` / `awb_state` (strings), `focus_distance_diopters`
(double), `awb_gains` (`{r,g_even,g_odd,b}`) and `zoom_actual` (double).

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
additional result.

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
  "lens": {"focal_lengths": [4.3], "apertures": [1.8],
           "hyperfocal_distance": 2.3, "minimum_focus_distance": 0.1},
  "control_mode": {"supported": true, "modes": [0, 1]},
  "ae_mode": {"supported": true, "modes": [0, 1, 2]},
  "iso": {"supported": true, "min": 50, "max": 12800, "step": 1},
  "exposure_time_ns": {"supported": true, "min": 1000, "max": 1000000000, "step": 1},
  "zoom_ratio": {"supported": true, "min": 1.0, "max": 8.0, "step": 0.0,
                 "legacy_crop_region": false},
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

- Control objects are either `{"supported": true, "modes": [...]}` or
  `{"supported": true, "min": …, "max": …, "step": …}` (numbers; `step: 0`
  means continuous). `supported: false` and every entry in `unsupported` mean
  the host must not send that control.
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
- `STATE` is rate-limited (default 10 Hz, max 30 Hz) and diff-compressed.
- `COMMAND`s are processed in order; a slow command never blocks `STATE`/`FRAME`.

## 9. Versioning rules

- v1 is frozen once shipped. New fields in JSON payloads are allowed only if
  old readers can ignore them (readers must tolerate unknown JSON keys).
- Removing/renaming a field, changing framing, adding a required field ⇒
  `protocol_version` bump and a new section in this file.
- Error `code` strings are additive-only.
