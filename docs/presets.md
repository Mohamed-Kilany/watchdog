# Control presets

Presets are named, reusable camera-control configurations stored on the **host** (M4-T11).
A preset captures the values that would otherwise be typed into `watchdog device
set-control`, applies them as one `set_controls` batch and can be exported as plain JSON so a
test rig or a colleague can reproduce the same camera setup.

Presets are deliberately untyped: the device's capability descriptor remains the authority
for ranges, modes and combinations. A preset that is valid for one camera may be rejected by
another; `preset apply` reports the per-control results instead of silently dropping values.

## Storage

One JSON file per preset under the data directory (see
[ADR-0005](adr/0005-data-directory-layout.md)):

```
<data_dir>/presets/<slug>.json
```

`<slug>` is the lowercased display name with runs of spaces, `-`, `_` and `.` collapsed to a
single `-` and leading/trailing dashes removed (`TV screen capture` →
`tv-screen-capture.json`). Saving a preset whose slug already exists overwrites that file and
keeps its original `created_at`; `updated_at` is always refreshed. Files are written through a
temporary file and renamed into place, so a crash cannot leave a half-written preset.

The directory is included in `watchdog config paths` and is created on first use.

## Schema (version 1)

```json
{
  "schema_version": 1,
  "name": "TV screen capture",
  "description": "Manual exposure, locked white balance, fixed focus",
  "controls": {
    "ae_mode": 0,
    "af_mode": 0,
    "anti_banding": 3,
    "awb_lock": true,
    "exposure_time_ns": 33333333,
    "focus_distance": 2.5,
    "frame_duration_ns": 33333000,
    "iso": 100,
    "zoom_ratio": 1.0
  },
  "created_at": "2026-11-12T18:04:07.123Z",
  "updated_at": "2026-11-12T18:04:07.123Z"
}
```

| Field | Required | Description |
|---|---|---|
| `schema_version` | yes | Must be `1`. Unknown versions are rejected rather than guessed at. |
| `name` | yes | Display name, at most 64 bytes: ASCII letters, digits, spaces, `-`, `_` and `.`. No leading/trailing whitespace, no path separators or control characters. |
| `description` | no | Free text, at most 512 bytes. |
| `controls` | yes | Object mapping control id to value. 1–128 entries; ids are at most 64 bytes of ASCII letters, digits, `_`, `-` and `.`. Values are arbitrary non-null JSON and at most 64 KiB each. |
| `created_at` | no | RFC 3339 UTC timestamp written by the store on first save. |
| `updated_at` | no | RFC 3339 UTC timestamp written by the store on every save. |

Read-only `STATE` entries (`fps_actual`, `recording`, `battery_percent`, `ae_state`,
`af_state`, `awb_state`, `focus_distance_diopters`, `awb_gains`, `color_transform`,
`zoom_actual`, lens readouts, `face_count`, `faces`, …) and one-shot triggers
(`ae_precapture`, `af_trigger`) are rejected. A preset stores *requests*; actuals are read
back from the device after applying it.

Control ids and values are the same ones documented in
[camera-controls.md](camera-controls.md) and [protocol.md](protocol.md). `camera_id` and
`physical_camera_id` are device-specific (on the SM-A515F, `0`/`2` are back cameras and
`1`/`3` are front cameras), so check a shared preset before applying it to another phone.

## Applying

`preset apply` sends every entry as a single `set_controls` command, in this order:

1. `camera_id`, then `physical_camera_id` (so dependent controls are validated against the
   selected camera),
2. all remaining ids alphabetically.

The device validates the batch atomically against the current capability descriptor and
applies the accepted subset in the next repeating request, exactly like a hand-written
`set-control` call. The host then reads actual values over the usual 750 ms settle window.
`COMMAND_RESULT` reports one entry per control; the command exits `1` if any assignment was
rejected, after printing the results, so scripts can detect partial failure.

Applying a preset is **not transactional across camera rebuilds**: structural controls
(`camera_id`, `physical_camera_id`, `stream_size`, `still_size`, `video_size`,
`jpeg_quality`, stream rotation, still/video formats, bitrate, codec, container, audio) are
validated together with everything else, but a camera switch still resets dependent controls
as described in [camera-controls.md](camera-controls.md#output-configuration). Keep structural
controls out of filming presets unless the camera selection is part of the preset.

## CLI

All commands live under `watchdog device preset`; see [cli.md](cli.md) for the global flags.

```console
# Capture the current writable controls (read-only, one-shot and structural controls
# are skipped; `requested` wins over `actual`).
watchdog device preset save "TV screen capture" --description "Manual exposure, locked WB"

# Capture a specific set instead. Explicit ids may include structural controls.
watchdog device preset save "Wide framing" --controls zoom_ratio,camera_id

# Include structural controls in the automatic capture.
watchdog device preset save "Rig baseline" --include-structural

# Inspect.
watchdog device preset list
watchdog device preset list --json
watchdog device preset show "TV screen capture"
watchdog device preset show tv-screen-capture --json

# Apply as one batch, or preview the exact batch without a device.
watchdog device preset apply "TV screen capture"
watchdog device preset apply "TV screen capture" --dry-run
watchdog device preset apply "TV screen capture" --json

# Share and restore.
watchdog device preset export "TV screen capture" ./tv-screen-capture.json
watchdog device preset import ./tv-screen-capture.json
watchdog device preset import ./tv-screen-capture.json --name "TV screen capture A51"

# Delete.
watchdog device preset rm "TV screen capture"
```

`preset save` connects to the app and reads a full `STATE` snapshot. With no `--controls`, it
stores every supported control that is neither read-only, one-shot nor structural, preferring
the last requested value and falling back to the actual value for device-originated entries.
With `--controls`, the listed ids must exist, be supported and be writable; unsupported,
read-only or one-shot ids are errors. `--serial` selects a device when several are connected.

`preset apply --dry-run` prints the ordered batch as JSON and exits without connecting; this
is the quickest way to review an imported preset before touching hardware.

## Filming example

The following preset was used for the real-capture review in
[camera-controls.md](camera-controls.md#reviewed-against-real-captures-2026-11-12) on a
Samsung SM-A515F. Values are the *requested* ones; the phone reported
`exposure_time_ns = 33,333,000` (one sensor step), `focus_distance = 2.508` and frozen AWB
gains.

```json
{
  "schema_version": 1,
  "name": "TV screen capture",
  "description": "Manual exposure, locked white balance, fixed focus",
  "controls": {
    "ae_mode": 0,
    "af_mode": 0,
    "anti_banding": 3,
    "awb_lock": true,
    "exposure_time_ns": 33333333,
    "focus_distance": 2.5,
    "frame_duration_ns": 33333000,
    "iso": 100,
    "zoom_ratio": 1.0
  }
}
```

`anti_banding = 3` is AUTO because this camera exposes only OFF (`0`) and AUTO (`3`). On a
device with fixed 50/60 Hz modes, use the mode matching mains instead. The exposure time is
one 30 fps frame period (33.3 ms) to match a 60 Hz panel without dropping frames. See the
filming guide for the reasoning behind each value.
