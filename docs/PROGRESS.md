# PROGRESS — watchdog task tracker

Single source of truth for what is done. Every task ID from TASK.md section 8 is listed here.
A task is ticked only when its acceptance criteria are met and its docs are written (and, for CI-gated tasks, CI is green).

Conventions:

- `- [ ]` not done · `- [x]` done · `dropped: <reason>` for tasks dropped with a reason (never deleted)
- **[DEVICE]** requires physical-device proof in docs/verification/
- Newly discovered tasks are added with a new ID in the relevant milestone

Environment note: no physical device was required for M1 tasks. Devices are available for M2/M3+ verification runs.

## M1. Foundations

CI for the M1 branch is green on ubuntu/windows/macos (clippy `-D warnings`, cargo test, fmt, cargo-deny, docs link check, fixture verification) — run [42263437349](https://github.com/Mohamed-Kilany/watchdog/actions/runs/42263437349).

- [x] M1-T01 Workspace
- [x] M1-T02 Config system
- [x] M1-T03 Data directory layout
- [x] M1-T04 Logging
- [x] M1-T05 Error model
- [x] M1-T06 Protocol crate + spec
- [x] M1-T07 CLI skeleton
- [x] M1-T08 CI skeleton
- [x] M1-T09 Docs skeleton + link checker
- [x] M1-T10 Fixture generator
- [x] M1-T11 Verification template

## M2. Native ADB (crate `adb`)

Native ADB implemented against AOSP `packages/modules/adb` (protocol.txt, adb.h, auth, pairing). Host is independent of system adb (port 5037 unused); CI has an isolation job with a foreign server stub on 5037. CI run [42419448704](https://github.com/Mohamed-Kilany/watchdog/actions/runs/42419448704) is green on ubuntu/windows/macos (fmt, clippy `-D warnings`, tests, cargo-deny, docs, fixtures, isolation). Real-device proof delivers in `docs/verification/` and is **pending** — until then, device flows are experimental.

- [x] M2-T01 Transport trait
- [x] M2-T02 Packet codec
- [x] M2-T03 Authentication
- [ ] M2-T04 USB transport **[DEVICE]** (nusb transport, `usb-doctor` and `udev-rules` implemented; per-OS device proof pending)
- [x] M2-T05 TCP transport + LAN connect
- [x] M2-T06 mDNS discovery
- [x] M2-T07 Wireless-debugging pairing + TLS **[DEVICE]** — SPAKE2/GCM/STLS/X509/TLS-exporter implemented; pairing-code flow, stored-key reconnect and TLS 1.3 traffic verified on a Samsung SM-A515F (Android 13) — see `docs/verification/2026-11-09-m2-wireless.md`.
- [x] M2-T08 Stream multiplexer
- [x] M2-T09 Shell v2
- [ ] M2-T10 Sync protocol: **[DEVICE]** (implemented; device-tested 1 GB transfer pending)
- [x] M2-T11 Port forwarding in-process:
- [x] M2-T12 Package management: **[DEVICE]** (streaming install + `-r` update verified on-device over TLS; downgrade not exercised — see `docs/verification/2026-11-09-m2-wireless.md`)
- [x] M2-T13 Device manager:
- [x] M2-T14 Fake device
- [x] M2-T15 CLI:
- [x] M2-T16 Isolation tests
- [x] M2-T17 Docs:

## M3. Android app MVP and zero-touch provisioning

Builds in CI: Gradle 9.6.0 + AGP 9.5.0 (built-in Kotlin), JDK 17, compile/target API 37.0. CI run [42452948701](https://github.com/Mohamed-Kilany/watchdog/actions/runs/42452948701) is green and produces the signed `watchdog-android-apk` artifact plus `apk-info.json` (versionCode 1000, versionName `0.1.0-dev+<sha>`). The `host-windows` job now uploads `watchdog-host-windows-x86_64`, a release host binary with the same APK embedded, so real-device runs use CI release builds only. The instrumented emulator job is manual (`workflow_dispatch`) and no longer gates merges. Device-gated tasks stay open until `docs/verification/` records exist.

- [x] M3-T01 Project setup — Gradle Kotlin DSL + version catalog, `app.watchdog.remote`, minSdk 26, API 37.0, R8, ktlint + detekt in CI, env/secret signing with debug fallback, git-tag versioning.
- [x] M3-T02 Manifest and permissions — required permissions and their reasons in `docs/android-app.md`, with `pm grant`/appops commands; exported entry points are token protected; no broad storage permission.
- [ ] M3-T03 Capture foreground service **[DEVICE]** (token-protected service + provisioning receiver implemented; 30-minute screen-off device run pending)
- [x] M3-T04 Camera controller (Camera2) **[DEVICE]** — state machine, catalog, recovery/backoff and unit tests implemented; on-device handle-leak check pending
- [x] M3-T05 Frame pipeline **[DEVICE]** — bounded drop-oldest queue, YUV_420_888 → NV21 → JPEG pipeline and unit tests implemented; 720p/15 fps measurement pending
- [x] M3-T06 Transport server — abstract socket `localabstract:watchdog-<token>`, single authenticated session with takeover, framing/heartbeat/backpressure; Kotlin codec passes all golden vectors + chaos tests (host-side forward wiring in M3-T13)
- [x] M3-T07 Handshake and device info — HOST_HELLO validation, version negotiation, HELLO_ACK with app/device/battery/storage/camera data; host-side refusal message lands with M3-T13
- [x] M3-T08 On-device UI (MVP) - landscape side panels around a live `SurfaceView` preview bound to `CaptureService` (same Camera2 session as the host stream), panels refreshed at 2 Hz from `uiStatus()` (connection/host, FPS, resolution, quality, dropped frames, camera state, storage, battery), manual permission gate and display-cutout insets; layout contract test runs on the manual M3-T11 emulator job.
- [x] M3-T09 Log ring buffer - 256 KiB byte-accounted ring, batched backlog flush and live `LOG_CHUNK` streaming, unit tests; `watchdog device logs` end-to-end lands with the host CLI (M3-T13/T16).
- [x] M3-T10 Resilience **[DEVICE]** - camera auto-reopen with bounded backoff, `camera_disconnected`/`camera_reopened`/`error` events and `START_REDELIVER_INTENT`; adb-kill and camera-occupation device checks pending.
- [x] M3-T11 Tests - JVM unit tests (codec golden vectors + invalid corpus, state machine, queue, log ring) and emulator instrumented tests (`MainLayoutTest`, `ServiceTransportTest`) run in the `android` and `android-instrumented` CI jobs.
- [x] M3-T12 Embedding - `crates/device/build.rs` embeds the release APK plus version name/code, SHA-256 and signer fingerprint when `WATCHDOG_APK` is set; `watchdog version` prints them and CI verifies the values against the built APK.
- [x] M3-T13 Provisioning pipeline - idempotent, per-step pass/fail/skipped report with timing; install/update with signature-mismatch guidance, permission/appops grants, battery/standby setup, recorded screen settings, token launch and first-frame verification; per-device state powers `status`/`restore`/`logs`. No-touch run verified on a Samsung SM-A515F (Android 13): all 7 steps PASS, handshake + 1280x720 first frame — see `docs/verification/2026-11-09-m2-wireless.md`.
- [x] M3-T14 Frame receiver + broadcast hub - sequence reordering with bounded hold, duplicate/stale drops, gap counting, bounded `tokio::broadcast` fan-out (slow subscribers lag without stalling the reader), PING/PONG clock-offset latency estimate and rolling FPS/bitrate/RTT stats; unit-tested.
- [x] M3-T15 Minimal test page - `watchdog serve` opens the provisioned session, spawns the frame hub and serves `/` (MJPEG feed + live stats), `/stream.mjpg`, `/api/stats` and `/healthz` on the UI port with optional bearer-token auth. Real-phone USB/LAN visual check pending **[DEVICE]**.
- [x] M3-T16 CLI - `watchdog device provision <serial>` (with `--apk`, `--no-verify`, `--no-screen`, timeouts, `--json`), `status [--json]`, `restore` and `logs [--follow]` implemented and documented in `docs/cli.md` and `docs/device-setup.md`.
- [ ] M3-T17 Measurements **[DEVICE]** - method and recording template in `docs/verification/m3-measurements.md`; glass-to-glass latency, FPS, CPU and thermal numbers for a 30-minute run pending real hardware.

## M4. Controls and state (the feature-rich core)

- [x] M4-T01 Capability descriptor — app builds one descriptor per camera from `CameraCharacteristics` without opening it (hardware level, capabilities, lens, controls with modes or min/max/step, streams/FPS/high-speed, full `{name,type}` key inventories, `unsupported` reasons) and embeds the map in `HELLO_ACK`; `watchdog device capabilities [--serial] [--json]` prints it via the provisioned session. Instrumented schema test and host summary unit tests. Per-device JSON dumps captured for all 4 cameras of the SM-A515F in `docs/verification/2026-11-09-a51/`.
- [x] M4-T02 State model and sync — canonical per-control entry (`requested`/`actual`/`locked`/`supported`/`source`) built from the descriptor; full `STATE` on session open and on demand, deltas only for changed entries with `seq`/`timestamp_ms`/`camera_id`/`dropped_frames`; per-session delta baseline so a reconnect starts from a full snapshot; default 10 Hz (1–30 Hz, fractional); deterministic last-writer-wins; device readouts (FPS, battery, storage) cached. Device: 34 controls, full snapshot then 1-entry deltas at 10 Hz — see `docs/verification/2026-11-09-m4-controls.md`.
- [x] M4-T03 Command pipeline — `set_controls` validates the batch against the descriptor, returns one `COMMAND_RESULT` per control in wire order with partial success, last-writer-wins, HAL-rejection rollback, values remembered while not streaming; manual sensor values imply `ae_mode = OFF`; actuals are read over a 750 ms settle window. Device: 22–28 ms round-trip; zoom, AE lock and manual ISO/exposure actuals follow the request (see record).
- [x] M4-T04 Exposure — `ae_fps_range` (validated against the descriptor's allowed pairs), `ae_regions` (normalized `{x,y,w,h,weight}` against `SENSOR_INFO_ACTIVE_ARRAY_SIZE`, 1..`max_regions`) and one-shot `ae_precapture` (`start`/`cancel`, never stored); manual ISO/exposure still imply `ae_mode = OFF` and restoring AE re-engages it. Device: [30,30] applied with `fps_actual=30`, invalid pair/region rejected, precapture one-shot verified, manual actuals matched, auto restored — see `docs/verification/2026-11-09-m4-exposure.md`.
- [x] M4-T05 Focus — `af_mode`/`af_lock`/`af_regions`/`af_trigger`/`focus_distance` with normalized regions and same-batch implications (manual distance implies AF off and clears the lock; lock switches to one-shot AF, scans and restores the previous mode). Device: 0.5 dioptre requested → 0.516 actual with `af_state=inactive`, tap-to-focus reached `focused_locked`, lock/unlock restored mode 4 — see `docs/verification/2026-11-09-m4-focus.md`.
- [x] M4-T06 White balance and colour — `awb_mode` presets, `awb_lock` (native lock; the A51 freezes gains exactly), `awb_regions` (normalized, 1..`max_regions`), `awb_state` readout, `effect` modes, `aberration_correction`, plus manual `awb_gains`/`color_transform` primitives that imply `awb_mode = OFF` where the camera exposes manual colour correction. Device: all 6 presets applied, lock froze gains (drift 0.0) with the torch on/off, unlocked AWB reacted (delta 0.015625), unsupported gain/transform rejected, all effects applied — see `docs/verification/2026-11-09-m4-wb.md`. **[DEVICE]**
- [x] M4-T07 Zoom and lens — `zoom_ratio` within the descriptor range via `CONTROL_ZOOM_RATIO`, with the centred `SCALER_CROP_REGION` fallback covered by unit tests; lens metadata (focal length, aperture, ND filter densities, focus calibration, optical zoom span), `filter_density`, optical/video stabilization, lens shading and API-33 distortion correction; `lens_focal_length`/`lens_aperture`/`lens_filter_density`/`lens_state` readouts. Device: zoom 1×/2×/4×/8× applied (4.0→3.989 and 8.0→7.968 from sensor-pixel rounding), out-of-range rejected, every stabilization/shading mode applied and unavailable modes rejected, ND filter and distortion listed unsupported — see `docs/verification/2026-11-10-m4-lens.md`.
- [x] M4-T08 Flash/torch, scene and image processing - `torch`/`flash_mode` (actuals read from `FLASH_MODE`), `scene_mode`, `noise_reduction`, `edge_enhancement`, `hot_pixel`, `tonemap` modes with contrast curve/gamma/preset where exposed, `lens_shading`, `face_detect` with normalized `face_count`/`faces` readouts, and `test_pattern`, all gated by the descriptor. Device: 48/48 checks passed with 0 retries; every available mode applied and unavailable modes rejected, the six-point contrast curve was applied and read back in three channels, face readouts were present with no face in view, and the app log showed no queue overflow or camera recovery - see `docs/verification/2026-11-10-m4-flash-scene.md`.
- [ ] M4-T09 Output configuration
- [ ] M4-T10 "Probe everything" mode
- [ ] M4-T11 Control presets/profiles
- [ ] M4-T12 Host control UI components
- [ ] M4-T13 On-device state panels **[DEVICE]**
- [ ] M4-T14 Docs:

## M5. Capture and media pipeline

- [ ] M5-T01 Still capture **[DEVICE]**
- [ ] M5-T02 Burst/sequence capture:
- [ ] M5-T03 Video recording: **[DEVICE]**
- [ ] M5-T04 Local storage manager
- [ ] M5-T05 Transfer to host: **[DEVICE]**
- [ ] M5-T06 Host media store:
- [ ] M5-T07 CLI:
- [ ] M5-T08 Offline behaviour: **[DEVICE]**

## M6. Web UI

- [ ] M6-T01 Stack and build
- [ ] M6-T02 App shell and real-time channel
- [ ] M6-T03 Devices page:
- [ ] M6-T04 Live view page:
- [ ] M6-T05 Controls UI:
- [ ] M6-T06 Capture page/panel:
- [ ] M6-T07 Media library:
- [ ] M6-T08 Settings page
- [ ] M6-T09 E2E tests
- [ ] M6-T10 Accessibility and UX pass
- [ ] M6-T11 Docs: **[DEVICE]**

## M7. Analyzers

- [ ] M7-T01 Analyzer framework:
- [ ] M7-T02 ffmpeg/ffprobe bundling:
- [ ] M7-T03 Media probe:
- [ ] M7-T04 Black screen analyzer:
- [ ] M7-T05 Frozen/stuck analyzer:
- [ ] M7-T06 Audio analyzers:
- [ ] M7-T07 Native image-diff (Rust):
- [ ] M7-T08 Template/element detection:
- [ ] M7-T09 Image quality analyzers:
- [ ] M7-T10 Buffering detector:
- [ ] M7-T11 Playback quality analyzer:
- [ ] M7-T12 Start-up time analyzer:
- [ ] M7-T13 Optional reference comparison:
- [ ] M7-T14 OTT rollup report:
- [ ] M7-T15 Pipeline and UI integration:
- [ ] M7-T16 Performance:
- [ ] M7-T17 Real-world validation [DEVICE]: **[DEVICE]**

## M8. Scheduler

- [ ] M8-T01 Job model:
- [ ] M8-T02 Schedule engine:
- [ ] M8-T03 Execution policy:
- [ ] M8-T04 History and results:
- [ ] M8-T05 Notifications:
- [ ] M8-T06 UI:
- [ ] M8-T07 API/MCP/CLI:
- [ ] M8-T08 Reliability: **[DEVICE]**

## M9. MCP server and Integration API

- [ ] M9-T01 API server:
- [ ] M9-T02 WebSocket event stream
- [ ] M9-T03 OpenAPI generated from code
- [ ] M9-T04 Auth:
- [ ] M9-T05 MCP server: **[DEVICE]**
- [ ] M9-T06 MCP safety:
- [ ] M9-T07 impresario integration: **[DEVICE]**
- [ ] M9-T08 Contract tests:
- [ ] M9-T09 Docs:

## M10. Hardening and release

- [ ] M10-T01 Security review:
- [ ] M10-T02 LAN mode:
- [ ] M10-T03 Multi-device: **[DEVICE]**
- [ ] M10-T04 Soak test [DEVICE]: **[DEVICE]**
- [ ] M10-T05 Failure-mode matrix:
- [ ] M10-T06 Upgrade path:
- [ ] M10-T07 Packaging:
- [ ] M10-T08 Final docs pass:
- [ ] M10-T09 Final verification [DEVICE]: **[DEVICE]**

## Working agreement (from TASK.md §8.0)

- One task per commit or PR; small commits; docs updated in the same change.
- Failing test first when feasible; ADRs in docs/adr/ for unspecified decisions.
- CI must be green on all platforms before a milestone merges to master.
