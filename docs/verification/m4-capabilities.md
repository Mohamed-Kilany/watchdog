# M4 capability capture — method

Records what each physical device reports; **no numbers or descriptors are invented here**.
Paste real command output into a dated record (`docs/verification/<YYYY-MM-DD>-m4.md`).

## Prerequisites

- CI artifacts only: download `watchdog-host-windows-x86_64` (release host with the
  embedded APK) and `watchdog-android-apk` from the same workflow run. Record the run id,
  commit SHA, `watchdog version` output and APK checksum.
- Host machine with **no system adb, no ffmpeg, no Android SDK** installed.
- Device provisioned by the same binary: `watchdog device provision --serial <serial>`.
  Capabilities reuse the session token recorded by provisioning.

## Steps

1. Capture the raw descriptor and the human summary:

   ```sh
   watchdog device capabilities --serial <serial> --json > <serial>-capabilities.json
   watchdog device capabilities --serial <serial> > <serial>-capabilities.txt
   ```

2. Confirm the map contains every camera id the device exposes (front/back/logical/
   physical as applicable) and that a re-run produces the same descriptor while no app
   state has changed.

3. Check the schema contract per camera:

   - `hardware_level`, `lens_facing`, `sensor_orientation`, `capabilities`,
     `physical_ids`, `lens`, `streams` are present;
   - every control is `{"supported": true, "modes": [...]}` or
     `{"supported": true, "min": …, "max": …, "step": …}`, or listed in `unsupported`
     with a non-empty reason;
   - `available_request_keys`, `available_result_keys` and
     `available_characteristics_keys` are non-empty lists of `{name, type}`;
   - `streams.formats` lists the advertised sizes and `streams.fps_ranges` the AE FPS
     ranges; high-speed entries include per-size FPS ranges.

4. Save the files under `docs/verification/<date>-m4/<serial>-capabilities.{json,txt}` and
   link them from the dated record. Include `watchdog version` output so the embedded APK
   is traceable.

## Failure handling

- `no session token recorded for <serial>` → run `watchdog device provision` first.
- `camera <id> disappeared` / `{"error": ...}` → note the event, rerun once; if it
  persists, file an issue and record it in the failures table.
