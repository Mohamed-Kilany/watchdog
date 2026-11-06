# TASK.md — Build `watchdog`

> Audience: an autonomous coding agent. Read this whole file before writing code. Do not assume; if something is genuinely ambiguous and blocks you, stop and ask. Otherwise decide, and record the decision as an ADR in `docs/adr/`.

## 1. What we are building

`watchdog` is a tool that turns an Android phone into a **remotely controlled, observable camera** pointed at a screen (TV, phone, set-top box) so OTT/streaming playback can be **watched, captured and analyzed from outside the device under test**.

It has two parts:

- **Host** (Rust): a single self-contained binary for Windows, macOS and Linux. Serves a web UI, an MCP server and an integration API. Talks to the Android device through its **own native Rust implementation of the ADB protocol**. Receives the live feed and the captured media, and analyzes it.
- **Remote** (Android app, "the watchdog app"): a Camera2-based app with full manual/automatic camera controls, a live viewfinder streamed in real time to the host, and local recording that is later pushed to the host.

`watchdog` runs in three modes:

1. **Standalone**: a human uses the web UI (and CLI).
2. **MCP**: any AI client connects to the MCP port and can take images, change camera controls and analyze images.
3. **Integrated with `impresario`** (our cross-platform automated UI testing tool, an Appium alternative): `impresario` calls watchdog's integration API (REST + WebSocket) to start/stop captures and fetch analysis verdicts as part of a test play. watchdog stays independent; impresario is a client.

## 2. Fixed decisions (already made, do not re-litigate)

| Topic | Decision |
|---|---|
| Host language | Rust (stable), Cargo workspace |
| Host targets | Windows, macOS, Linux; x64 + arm64 each |
| Android camera API | Camera2, minSdk 26 (Android 8), Kotlin |
| Live video transport | JPEG frames (MJPEG-style) over an adb-forwarded socket, works over USB and LAN |
| impresario integration | impresario is a client of watchdog's integration port (REST + WebSocket) |
| Network exposure | All ports bind to `127.0.0.1` by default; LAN exposure is opt-in with optional bearer-token auth |
| Dependency on host state | **None.** No system adb, no Android SDK, no ffmpeg on PATH, no browser assumptions beyond the user opening the UI |

## 3. Requirements

### 3.1 Host

H1. **Single binary, any OS.** Releases for Win/macOS/Linux × x64/arm64. No installer required to run; `watchdog serve` must work after download. Also provide a CLI (`watchdog --help`).

H2. **Three ports** (all configurable via config file, env and flags; defaults below):
- `7700` Web UI (+ its backing UI API/WebSocket)
- `7701` MCP server (Streamable HTTP; also `watchdog mcp --stdio` for clients that spawn it)
- `7702` Integration API (REST + WebSocket, versioned `/api/v1`, OpenAPI spec generated from code and served at `/api/v1/openapi.json`)
If a port is busy, fail with a clear message naming the port and the flag to change it. Never silently pick another port.

H3. **Web UI.** A web page embedded in the binary (no runtime asset files, no CDN). It must provide:
- Device management: list, connect (USB/LAN), pair, disconnect, install/update the app, status.
- Live viewfinder with FPS/latency/bitrate stats.
- Full control panel over the remote app (see R4/R5), laid out on the **sides** of the video, never overlaid on it.
- Capture: still image, burst/sequence (N frames, interval), video record start/stop, with progress and download.
- Media library of everything received, with preview, metadata and one-click analysis.
- Analysis: choose analyzers, run, view the report (timeline of events, thumbnails of problem frames, pass/fail per rule).
- **Scheduler**: create recurring or one-off jobs (cron-like + simple presets), e.g. "every 30 min capture 10 s of video from device X and run analyzers A,B"; job history, last result, enable/disable, run-now. Persist across restarts.
- Settings: ports, auth token, storage location/retention, ffmpeg info, about/version.

H4. **Device connection via UI or CLI, native Rust ADB.**
- Implement the ADB protocol in Rust (host↔device transport over USB and TCP): CONNECT/AUTH (own RSA key pair generated and stored in watchdog's data dir, not `~/.android`), OPEN/WRTE/OKAY/CLSE streams, shell v2, sync (push/pull), port forward/reverse equivalents, package install (streaming install), and Android 11+ **wireless-debugging pairing** (TLS pairing code flow) plus `adb tcpip`-style LAN connect.
- USB access must work on all three OSes without requiring a system adb server (use a Rust USB crate; document OS-specific needs such as udev rules on Linux and WinUSB driver notes on Windows, and generate/print the udev rules via a CLI command).
- CLI: `watchdog device list|connect|pair|disconnect|install|shell|logs`.
- **Hard rule:** if `adb` exists on the machine it must not be used, required, or interfered with. CI must include a test that runs with no adb on PATH and a different adb server occupying port 5037.

H5. **Embedded assets.** The web UI bundle and the watchdog APK are embedded in the host binary (e.g. `rust-embed`/`include_bytes!`) and versioned together. The host verifies the app on the device matches the embedded APK version (version code + signature) and offers/auto-performs update.

H6. **Receiving media.** Accept recordings/stills pushed from the device (see R6), verify integrity (size + hash), store under a configurable data dir with an index (SQLite), and make them available to analyzers, UI, MCP and API.

H7. **Analysis engine** (pluggable analyzer trait; each analyzer declares inputs, parameters, outputs):
- **Video analyzers (bundled ffmpeg/ffprobe):** black screen (`blackdetect`), frozen/stuck frames (`freezedetect`), audio silence (`silencedetect`), loudness (`ebur128`), scene changes, per-frame luma/chroma/saturation stats (`signalstats`), duplicate/dropped frame detection, effective frame rate and variance, A/V basics, stream info via ffprobe. Optional reference comparison (PSNR/SSIM; VMAF if the bundled build supports it).
- **Image analyzers (native Rust, no ffmpeg):** image-diff (pixel and perceptual, with region masks and tolerance), black/white/solid-color frame detection, "stuck" detection across an image sequence, region-of-interest comparison, template match for a known element (e.g. buffering spinner, logo, error screen), sharpness/blur estimate, brightness/exposure histogram.
- **OTT-oriented rollup report** built from the above, reporting at minimum: **buffering events** (count, total/longest duration, via spinner template + freeze correlation), **black screens** (count, durations, start timestamps), **stuck/frozen screens**, **audio dropouts**, **start-up time** (trigger → first non-black, non-spinner frame, when a trigger timestamp is provided), **frame-rate stability**, **resolution/quality-change heuristics**, and a pass/fail verdict against user-defined thresholds. Output: JSON (stable schema, documented) + human-readable HTML/UI view.
- **ffmpeg bundling:** ship a per-OS/arch ffmpeg+ffprobe with the host (embedded and extracted to the data dir on first run with checksum verification, or bundled as a verified download step at *build* time, never at user runtime). Prefer an LGPL build; document the exact build, its license obligations and how to rebuild it in `docs/`. The host must run with no ffmpeg on PATH.

H8. **MCP server.** Exposes tools for AI clients. Images only for AI consumption (video is analyzed by the engine, and the AI receives the *report* plus selected frames). Minimum tools: `list_devices`, `connect_device`, `get_device_state`, `get_camera_controls` (with supported ranges/capabilities), `set_camera_controls`, `lock_white_balance`, `lock_focus`, `lock_exposure`, `capture_image` (returns image content), `capture_sequence`, `get_live_frame`, `start_recording`/`stop_recording`, `list_media`, `analyze_image`, `diff_images`, `analyze_video` (returns report + key frames, not the video), `run_schedule_now`. Include resources (media index, reports) and clear tool descriptions/JSON schemas. Document setup snippets for common MCP clients.

H9. **Integration API.** REST + WebSocket mirroring everything the UI can do (devices, controls, state stream, live frames, capture, media, analysis, schedules). Properly documented (OpenAPI + prose guide + examples in curl and a minimal client for impresario's use). WebSocket events: device connected/disconnected, control state changes, capture/recording lifecycle, analysis finished. Version the API; breaking changes need a new version prefix.

H10. **Security.** Localhost-only by default. `--listen 0.0.0.0` (or per-port setting) enables LAN, and when LAN is enabled the host strongly recommends and supports a bearer token (`--token` / config); UI, MCP and API all honor it. Never log tokens. Validate all inputs; limit upload sizes; no path traversal on media endpoints.

### 3.2 Remote (the watchdog Android app)

R1. **Platform:** Kotlin, Camera2, minSdk 26. Foreground service for capture/streaming so it survives screen-off, with the screen kept on while active.

R2. **Zero-touch provisioning.** From the host, with no taps on the device (beyond the unavoidable one-time "Allow USB/wireless debugging" dialog the OS itself requires):
1. install/update the embedded APK via the native adb implementation,
2. grant every runtime permission it needs (`pm grant`: camera, microphone, notifications, media/storage as required) and relevant app-ops,
3. exempt from battery optimization / doze, allow background start, keep screen awake while connected,
4. launch the app and bring it to a ready state (`am start` / service start),
5. open the adb-forwarded sockets.
If something cannot be automated on some OEM/OS version, detect it, report it precisely in UI/CLI with the manual step, and document it in a compatibility table.

R3. **Live viewfinder.** Real-time JPEG frames streamed to the host over the forwarded socket. Configurable resolution, JPEG quality and target FPS; adaptive degradation under a slow link (drop frames, never queue unbounded). The preview on the device itself is clear and full-quality. Report glass-to-glass latency estimate in UI stats.

R4. **Controls — as complete as the hardware allows.** On startup the app queries `CameraCharacteristics` and publishes a **capability descriptor** (what is supported, ranges, steps) so the UI only offers real options. Implement and expose, at least:
- Camera selection (front/back/logical/physical IDs), resolution, FPS range, video size/bitrate/codec/container, still format (JPEG; RAW/DNG when supported), JPEG quality/orientation.
- Exposure: AE mode (auto/manual), exposure time, ISO, frame duration, exposure compensation, **AE lock**, AE regions, anti-banding (50/60 Hz — important when filming screens).
- Focus: AF mode (continuous/auto/manual/off/macro), manual focus distance, **AF lock/trigger**, AF regions, tap-to-focus from the host.
- White balance: AWB mode presets, manual color temperature/color-correction gains, **AWB lock**.
- Zoom (including `zoomRatio` on Android 11+), flash/torch, scene mode, color effects, noise reduction, edge enhancement, tonemap, hot-pixel, optical + electronic stabilization, face detect, test pattern, color-correction transform/aberration modes, HDR/scene modes where exposed, lens aperture/filter density/focal length when adjustable.
- Anything else Camera2 exposes through `CaptureRequest` keys that the device lists as available: **try them all**, expose the ones that work, and record unsupported ones in the descriptor. Provide an "advanced raw keys" view that lists available request keys generically.
- Every control supports auto and manual states where the hardware allows, and the lock controls (AE/AWB/AF) are first-class.

R5. **State reporting and remote manipulation.** The app shows the **current state of every control** (the actual values reported in `TotalCaptureResult`, not just what was requested: e.g. real ISO, real shutter, real focus distance, AE/AWB/AF state, locks, FPS, resolution, recording status, storage left, battery, temperature, link status) and sends that state to the host continuously (rate-limited, diffed). The host UI reflects it exactly and every control can be changed from the host; the device UI reflects those changes immediately. Conflicts: last writer wins, and the state always shows the truth from the camera. **UI layout rule:** state/controls panels are placed at the **sides** of the video preview (landscape layout), never on top of it, so the picture stays unobstructed.

R6. **Local recording then push.** Videos and still images are saved on the device first (app-scoped storage), with a manifest (id, timestamps, control state snapshot, device info). The host pulls/receives them (via adb sync or the forwarded socket), verifies hash, and then (configurable) deletes them from the device. Interrupted transfers must resume or retry safely. Recording must not depend on the live link: if the link drops, recording continues and syncs later. Support capturing a still and a recording while the live stream runs where the hardware permits, and document limitations per device.

R7. **Robustness.** Handle camera disconnects, permissions revoked, app killed, USB unplugged, LAN IP change: detect, recover, and surface status. No crash on any of these.

## 4. Suggested architecture (you may improve it, document why in an ADR)

```
watchdog/
  Cargo.toml                (workspace)
  crates/
    adb/            native ADB: transport (usb, tcp), auth, pairing, shell, sync, forward, install
    protocol/       host<->app wire protocol (frames, control state, commands, manifest) shared spec
    device/         device manager, provisioning, app lifecycle
    media/          storage, index (SQLite), transfers
    analyzers/      trait + image analyzers (native), ffmpeg wrapper, OTT rollup
    scheduler/
    api/            REST/WebSocket integration server + OpenAPI
    mcp/            MCP server
    ui-server/      serves embedded UI + UI websocket
    host/           binary (CLI, wiring, config)
  ui/               web UI source (built in CI, embedded into host)
  android/          Kotlin app (Gradle)
  docs/
  .github/workflows/
```

Define the host↔app wire protocol once in `docs/protocol.md` (framing, message types, versioning, JPEG frame header with timestamp/sequence, control command and state schemas) and implement both sides against it. Add protocol version negotiation.

## 5. Documentation (very important, part of the deliverable)

Documentation is not an afterthought; write it as you go and keep it current. Required in `docs/`, all linked from `README.md`:

- `README.md`: what it is, 5-minute quick start (download, run, connect a phone, see live view), screenshots.
- `docs/architecture.md` with diagrams and data flow; `docs/adr/` for every significant decision.
- `docs/user-guide.md`: UI walkthrough, scheduler, analysis, CLI reference (generated from clap where possible).
- `docs/device-setup.md`: enabling USB/wireless debugging, per-OS USB notes, pairing, troubleshooting, OEM quirks table.
- `docs/camera-controls.md`: every control, what it does, auto/manual behavior, known device differences.
- `docs/analysis.md`: every analyzer, parameters, report schema, how OTT metrics are derived, limitations and false positive notes.
- `docs/api.md` + OpenAPI spec; `docs/mcp.md` with tool reference and client config examples; `docs/impresario-integration.md` with the exact calls impresario should make and a worked example.
- `docs/protocol.md`, `docs/development.md` (build, test, run, debug, adding an analyzer or control), `docs/release.md`, `docs/security.md`, `docs/licenses.md` (ffmpeg and all third-party licenses).
- `CHANGELOG.md`. Doc examples must be executed or tested where feasible (CI checks that documented commands and API examples still work).

## 6. Build, CI and testing strategy

**Use GitHub Actions for building and testing everything.** Do not rely on the local machine for builds when CI can do it.

CI workflows:
- `ci.yml` on every push/PR: `cargo fmt --check`, `clippy -D warnings`, `cargo test` on Win/macOS/Linux; web UI build and tests; Android `./gradlew lint test assembleDebug assembleRelease`; docs link check; API/OpenAPI and MCP schema snapshot tests; "no system adb / no system ffmpeg" test.
- `release.yml` on tags: builds the APK (signed with a CI-managed keystore secret), builds the UI, embeds both, cross-compiles the host for all 6 targets (win/mac/linux × x64/arm64), bundles ffmpeg per target, produces checksums, uploads artifacts and a GitHub Release.
- Prefer testing at the emulator level in CI where possible (Android emulator job for app provisioning, permission grant, control-state round trip, frame streaming, record/push), plus unit and integration tests with a fake device implementing the adb protocol for the `adb` crate. Camera behavior on emulators is limited; document what the emulator can and cannot prove.

**Real device verification.** Only when physical hardware is needed, download the CI artifacts (`gh run download` or the release assets) and run the **release builds** on the real machine with a real Android device attached. Never verify with a locally built debug binary as the final proof. Record exactly which artifact (run id, commit SHA, checksum) was tested.

## 7. Definition of Done

A change/milestone is **not done** until all of the following are true:

1. CI is green on all platforms for the commit in question, and the release artifacts exist.
2. Unit and integration tests cover the new code; analyzers have golden-file tests with fixture images/videos (include fixtures: clean video, black segment, frozen segment, buffering spinner segment, silent audio).
3. **Physical device verification has been performed and recorded**, using the CI **release** artifacts, on at least: one phone over USB and the same or another over LAN (wireless debugging), on at least two different Android versions/OEMs if available (state what was available). The record lives in `docs/verification/<date>-<milestone>.md` and includes: artifact id/checksum, device model + Android version, steps executed, results, screenshots, and sample output reports. Failures found go to a tracked issue list; do not hide them.
4. The verification covers, at minimum, this checklist on a real device: zero-touch install + permission grant (no taps, besides the OS debug-authorization prompt), live view with sane latency, every listed control changed from the host and reflected in the real device state, AE/AWB/AF locks verified by observing the actual state values, still capture, burst, video record, push to host with hash verified, link-drop-during-recording recovery, USB-unplug and reconnect, analysis of a recording of a real playing OTT/video screen (including a deliberately induced black screen and freeze) producing the expected report, scheduler run executing unattended, MCP `capture_image` + `analyze_image` from a real MCP client, and an end-to-end call from an `impresario` run (or the documented stand-in client) through the integration API.
5. It works with **no adb, no ffmpeg and no Android SDK installed** on the verification machine, and with a competing adb server running.
6. All documentation in section 5 is updated for the change, and docs examples were exercised.
7. Everything is released through the GitHub Actions release workflow, not hand-built.

## 8. Milestones and task breakdown

Deliver milestones in order. Every milestone ends with its own Definition of Done (section 7) and its own `docs/verification/` record.

### 8.0 Working agreement (applies to every task below)

- Create `docs/PROGRESS.md` in M1 containing every task ID from this section as a checkbox. Tick a task only when its acceptance criteria are met and its docs are written. Add newly discovered tasks with a new ID; never delete tasks, mark them `dropped` with a reason.
- For each task: write the failing test first when feasible, implement, document, run CI, update `PROGRESS.md`, commit. Small commits, one task per commit or PR.
- Each task lists **AC** (acceptance criteria). A task without all AC met is not done, even if the code compiles.
- Tasks marked **[DEVICE]** require physical-device proof recorded in `docs/verification/`.
- Anything you decide that is not specified here gets an ADR in `docs/adr/NNNN-title.md` (context, options, decision, consequences).

---

### M1. Foundations

- **M1-T01 Workspace.** Cargo workspace per section 4, `rust-toolchain.toml` pinned, `deny.toml` (cargo-deny: licenses, advisories), `rustfmt.toml`, `clippy` config. AC: `cargo build --workspace` and `cargo test --workspace` pass on 3 OSes in CI.
- **M1-T02 Config system.** Layered: defaults < `watchdog.toml` < env (`WATCHDOG_*`) < CLI flags. Keys: ports, listen addresses, token, data dir, retention, ffmpeg path override (debug only), log level. `watchdog config show|path|validate`. AC: unit tests for precedence and invalid values; documented table of every key.
- **M1-T03 Data directory layout** per OS (`directories` crate): config, keys, media, db, logs, cache, extracted tools. AC: `watchdog doctor` prints all paths and write-permission checks.
- **M1-T04 Logging.** `tracing` with console + rotating file, JSON option, secret redaction (tokens, pairing codes). AC: test proves token never appears in logs.
- **M1-T05 Error model.** One error taxonomy shared by CLI/API/MCP (stable error codes, human message, hint). AC: documented in `docs/api.md` skeleton.
- **M1-T06 Protocol crate + spec.** `crates/protocol` with the framing, message types and schemas from `docs/protocol.md`: HELLO/handshake (protocol version, app version code, device info, capability descriptor), FRAME (seq, timestamp ns, width, height, rotation, JPEG bytes), STATE (full + delta), COMMAND (set controls, capture, record, ping), EVENT, MEDIA_MANIFEST, LOG_CHUNK, ERROR, BYE. AC: spec doc, Rust codec, **golden test vectors** (binary files under `protocol/vectors/`) that the Kotlin implementation in M3 must also pass.
- **M1-T07 CLI skeleton** (clap): `serve`, `device`, `capture`, `analyze`, `schedule`, `mcp`, `config`, `doctor`, `version`. AC: `--help` for every command, generated CLI reference in docs.
- **M1-T08 CI skeleton** (ci.yml) with caching, matrix, artifact upload. AC: green run, link to it in PROGRESS.md.
- **M1-T09 Docs skeleton + link checker** (all files from section 5 created with outlines). AC: docs link check in CI.
- **M1-T10 Fixture generator.** Script (documented, using ffmpeg in CI only) creating deterministic test media: clean video, black segment, frozen segment, spinner segment, silence segment, solid-colour and gradient images. Commit small fixtures; generate larger ones in CI. AC: fixtures reproducible bit-for-bit or documented tolerance.
- **M1-T11 Verification template** `docs/verification/TEMPLATE.md` with the checklist from section 7. AC: template exists and is referenced from `docs/development.md`.

---

### M2. Native ADB (crate `adb`)

No external adb at any point. Implement against AOSP's protocol docs (`OVERVIEW.TXT`, `protocol.txt`, `SERVICES.TXT`, `SYNC.TXT`, pairing protocol docs); cite them in the crate README.

- **M2-T01 Transport trait** (`async`): `Usb`, `Tcp`, `Mock`. AC: all higher layers depend only on the trait.
- **M2-T02 Packet codec.** CNXN, AUTH, STLS, OPEN, OKAY, WRTE, CLSE; version/maxdata negotiation; checksum behavior per protocol version; large-payload handling. AC: fuzz test (cargo-fuzz or proptest) on the decoder; no panics on garbage.
- **M2-T03 Authentication.** RSA-2048 key generated on first run, stored in watchdog data dir (never `~/.android`), Android public-key format, AUTH token signing, AUTH_RSAPUBLICKEY flow, "always allow" handling. AC: unit tests with known vectors; on a real device the authorization prompt appears once and persists.
- **M2-T04 USB transport** (Rust USB crate): enumerate ADB interfaces (class 0xFF, subclass 0x42, protocol 0x01), claim, bulk in/out, hotplug events, serial number reading, clean release on exit/crash. AC: works on Windows (WinUSB), macOS, Linux; `watchdog device usb-doctor` explains permission problems and `watchdog device udev-rules` prints rules. **[DEVICE]** on each OS you have.
- **M2-T05 TCP transport + LAN connect.** `host:port` connect, timeouts, reconnect with backoff. AC: connect to a device with `adb tcpip`-style listener and to wireless-debugging endpoints.
- **M2-T06 mDNS discovery** for `_adb-tls-connect._tcp` and `_adb-tls-pairing._tcp`. AC: discovered devices appear in `device list --discover` and in the UI list. (Fallback: manual host:port entry.)
- **M2-T07 Wireless-debugging pairing + TLS** (Android 11+): pairing-code flow (SPAKE2 + TLS exporter keying + peer-info exchange), then STLS upgrade on connect. AC: `watchdog device pair <host:port> <code>` succeeds on a real device; documented step by step. **[DEVICE]**
- **M2-T08 Stream multiplexer.** Local/remote id allocation, flow control (OKAY/WRTE), clean CLSE, many concurrent streams, cancellation safe. AC: stress test with 100 concurrent streams on the mock.
- **M2-T09 Shell v2** (stdout/stderr split, exit code, stdin, PTY off by default) plus legacy shell fallback and `exec:`. AC: tests for exit codes, binary output, large output, timeouts.
- **M2-T10 Sync protocol:** STAT, LIST, SEND (with mode/mtime), RECV, DATA/DONE/FAIL, resume support by offset where possible (document the approach when sync cannot resume). AC: push/pull of 1 GB file on a real device with hash verification; progress callbacks. **[DEVICE]**
- **M2-T11 Port forwarding in-process:** host TCP listener ⇄ device `tcp:`/`localabstract:`/`localfilesystem:` services, lifecycle tied to device connection, no leaked listeners. AC: forward to the app's abstract socket works over USB and LAN.
- **M2-T12 Package management:** streaming install (`exec:cmd package install` / `abb_exec` where available, with `pm install` fallback), install flags for downgrade/replace, uninstall, query installed version code, install-failure messages translated into clear hints. AC: install, update and downgrade paths tested. **[DEVICE]**
- **M2-T13 Device manager:** tracks connected devices, serial, model, SDK, ABI, state (unauthorized/offline/ready), events stream, multi-device safe. AC: unplug/replug and Wi-Fi drop tested.
- **M2-T14 Fake device** (`adb-mock` test crate): implements enough of the protocol (auth, shell, sync, forward, install) for CI. AC: all `adb` crate integration tests run against it.
- **M2-T15 CLI:** `device list|discover|connect|pair|disconnect|info|shell|push|pull|install|uninstall|forward|logs`. AC: documented with examples and tested against the mock.
- **M2-T16 Isolation tests.** CI job with no adb on PATH and a foreign adb server bound to 5037 must pass; the real-device run repeats this. AC: explicit test names `no_system_adb_*`.
- **M2-T17 Docs:** `docs/device-setup.md` (per OS), troubleshooting table, protocol notes, security notes about key storage. AC: a new user can connect a phone by following only the doc.

---

### M3. Android app MVP and zero-touch provisioning

**Goal:** from a clean host with nothing installed and a phone in USB-debugging mode, one command installs the app, grants permissions, starts it and shows a live camera view in a test page, with no touches on the phone besides the OS debug-authorization prompt.

**App tasks (`android/`)**

- **M3-T01 Project setup.** Gradle Kotlin DSL, version catalog, application id `app.watchdog.remote` (change only via ADR), minSdk 26, compile/target latest stable SDK, R8/ProGuard rules, ktlint + detekt, deterministic builds, signing config read from env/secrets in CI (debug keystore fallback for local). AC: `./gradlew assembleRelease` yields a signed, installable APK in CI; app version code/name are derived from the git tag and exposed to the host.
- **M3-T02 Manifest and permissions.** CAMERA, RECORD_AUDIO, FOREGROUND_SERVICE, FOREGROUND_SERVICE_CAMERA, FOREGROUND_SERVICE_MICROPHONE (API 34+), POST_NOTIFICATIONS (33+), WAKE_LOCK, INTERNET, REQUEST_IGNORE_BATTERY_OPTIMIZATIONS; no broad storage permission (use app-specific storage). Exported components only where provisioning needs them, protected by a per-session token passed as an intent extra. AC: permission list documented with the reason for each; all grantable via `pm grant` or appops.
- **M3-T03 Capture foreground service.** Owns the camera and the transport; foreground notification with camera/microphone types; survives activity death and screen-off; wake lock only while active; clean shutdown on `BYE`. AC: service keeps streaming for 30 minutes with the screen off on a real device. **[DEVICE]**
- **M3-T04 Camera controller (Camera2).** Thread/handler model, explicit state machine (closed → opening → open → configured → streaming → error → recovering), safe open/close ordering, handles `onDisconnected`, `onError`, camera-in-use, and permission loss. Camera selection by ID, including logical multi-camera detection (just enumerate; no controls yet). AC: state machine unit-tested with fakes; fault injection tests; no leaked camera handles after 100 open/close cycles.
- **M3-T05 Frame pipeline.** Preview target (on-device viewfinder) plus an analysis `ImageReader` (YUV_420_888) → JPEG encoder → sender queue. Bounded queue with drop-oldest policy, frame sequence numbers and timestamps (sensor timestamp + host-sync offset), configurable size/quality/FPS, rotation metadata. Optimize conversion (avoid per-frame allocations; reuse buffers; consider native libjpeg-turbo if the Java path misses the FPS target, as an ADR). AC: sustained 15 fps at 1280x720 (target 30) on a mid-range device with bounded memory; measured and recorded.
- **M3-T06 Transport server.** Abstract local socket server (`localabstract:watchdog-<session>`), single authenticated host session, framing per `docs/protocol.md`, heartbeat/ping, backpressure, graceful reconnect (a new host session can take over). AC: Kotlin codec passes **all golden vectors** from M1-T06; chaos test with random disconnects.
- **M3-T07 Handshake and device info.** HELLO exchange: protocol version negotiation, app version code, device model/manufacturer/SDK/ABI, battery, storage, list of cameras with IDs/facing/hardware level. AC: host refuses incompatible protocol versions with a clear error.
- **M3-T08 On-device UI (MVP).** Landscape layout: video preview in the centre, **left panel** (connection status, host name/IP, session state, FPS, resolution, JPEG quality, dropped frames, latency) and **right panel** (camera ID/facing, camera state, recording state placeholder, storage, battery, temperature). Nothing is drawn over the video. A manual permission screen shown only if provisioning did not grant everything. AC: screenshot test (emulator) proves panels do not overlap the preview; works on phone sizes 5" to 7" and with display cutouts.
- **M3-T09 Log ring buffer** (last N KB) retrievable by the host (LOG_CHUNK) and shown in `watchdog device logs`. AC: verified end to end.
- **M3-T10 Resilience.** Auto-reopen the camera after interruptions (incoming call, other app takes camera), report events to host, restart the service from the host if killed. AC: tested by killing the app via adb and by occupying the camera with another app.
- **M3-T11 Tests.** JVM unit tests (codec, state machine, queue policy), instrumented tests on the CI emulator (virtual scene camera): service start, handshake, frames flowing, reconnect. AC: all run in `ci.yml`.

**Host tasks**

- **M3-T12 Embedding.** `build.rs`/CI step that embeds the release APK and records its version code, SHA-256 and signing cert fingerprint. Dev mode allows pointing to a local APK. AC: `watchdog version` prints host, protocol and embedded-APK versions.
- **M3-T13 Provisioning pipeline** (`device` crate), every step reported as pass/fail/skipped with timing, idempotent, resumable:
  1. detect and ensure authorized device; read props (SDK, ABI, model, OEM, `ro.build.version.release`);
  2. check installed app version and signature vs the embedded APK; install/update if needed (handle signature mismatch with an explicit, documented uninstall path);
  3. grant runtime permissions via `pm grant`; set appops (`cmd appops set <pkg> ...` e.g. camera, record audio, foreground start, run-in-background);
  4. battery: `dumpsys deviceidle whitelist +<pkg>`, standby bucket, background restriction off;
  5. screen: wake (`input keyevent`), dismiss keyguard where possible, keep awake while charging (`settings put global stay_on_while_plugged_in`), with **recorded original values and revert on disconnect or via `watchdog device restore`**;
  6. launch (`am start` / `am start-foreground-service` with session token);
  7. wait for socket, forward, handshake, verify first frame.
  AC: full flow works on a real device with no touches; failures give actionable hints; per-OEM quirks are collected in `docs/device-setup.md` compatibility table. **[DEVICE]**
- **M3-T14 Frame receiver + broadcast hub.** Receives frames, reorders/drops by sequence, publishes to subscribers (UI, MCP, recorder) via bounded broadcast; computes FPS, bitrate, latency (clock-offset estimation via ping). AC: slow subscriber cannot stall others; unit-tested.
- **M3-T15 Minimal test page** served on the UI port showing the live feed and stats (replaced by the real UI in M6). AC: visible live video from a real phone over USB and over LAN. **[DEVICE]**
- **M3-T16 CLI:** `watchdog device provision <serial>`, `watchdog device status`, `watchdog device restore`. AC: documented.
- **M3-T17 Measurements.** Record glass-to-glass latency (point at a running on-screen counter), FPS and CPU/thermal behaviour for 30 minutes on at least one real device, in the verification record. AC: numbers and method written down.

---

### M4. Controls and state (the feature-rich core)

**Rule:** each control is its own task with the same Definition: capability detection from `CameraCharacteristics`, request-key mapping, result-key readback from `TotalCaptureResult`, value validation/clamping against reported ranges, protocol messages, UI widget (host), on-device display, API/MCP exposure, unit test with a fake camera, docs entry in `docs/camera-controls.md` (what it does, auto/manual semantics, device differences), and real-device check that the **reported** value changes as requested. Unsupported controls are hidden/disabled by the capability descriptor and listed as unsupported, never silently ignored.

- **M4-T01 Capability descriptor.** Full snapshot per camera ID: hardware level, capabilities, available modes for every control, numeric ranges and steps, supported stream combinations, available request/result/characteristic keys (names + types) via reflection. AC: JSON dump saved per device into `docs/verification/` (also a `watchdog device capabilities --json` command).
- **M4-T02 State model and sync.** Canonical control state schema: `requested`, `actual` (from capture results), `locked`, `supported`, `source` (host/device/auto). Rate-limited diff updates (default 10 Hz, bounded burst), full resync on reconnect. AC: protocol spec updated; round-trip latency of a control change measured and reported.
- **M4-T03 Command pipeline.** Host sends control batches; the app applies them atomically in the next repeating request; acknowledgements carry the applied values or an error per control. Idempotent, ordered, last-writer-wins, host and device UI both can issue changes. AC: concurrency test (two writers) deterministic.
- **M4-T04 Exposure.** AE mode (off/on/on-flash variants where supported), exposure time, ISO (sensitivity), frame duration, exposure compensation (+ step), **AE lock**, AE metering regions, AE target FPS range, anti-banding mode (off/50 Hz/60 Hz/auto; default recommendation for filming screens documented), AE precapture. AC: with AE off and fixed ISO/time, actual values match requested within hardware step.
- **M4-T05 Focus.** AF mode (off/auto/macro/continuous-video/continuous-picture/EDOF), manual focus distance (diopters, with min/hyperfocal info), **AF lock** (trigger + hold), AF regions, tap-to-focus from the host viewfinder (normalized coordinates, rotation aware), AF state display (inactive/scanning/focused/not-focused/locked). AC: tap-to-focus verified on a real device; AF state transitions shown in UI. **[DEVICE]**
- **M4-T06 White balance and colour.** AWB mode presets, manual colour temperature (via colour-correction gains + transform when `COLOR_CORRECTION_MODE` supports it), manual RGGB gains, **AWB lock**, AWB regions, AWB state; colour effect modes; aberration correction. AC: AWB lock verified to freeze reported gains while the scene lighting changes. **[DEVICE]**
- **M4-T07 Zoom and lens.** `zoomRatio` (API 30+) with legacy crop-region fallback, optical zoom/focal length, aperture, filter density where adjustable, optical and video stabilization, lens shading, distortion correction. AC: ranges come from the descriptor.
- **M4-T08 Flash/torch, scene and image processing.** Torch on/off, flash mode, scene modes, noise reduction, edge, hot-pixel, tonemap mode (incl. contrast curve/gamma/preset where available), shading, face detection (overlay boxes **drawn in the host UI, not on the device video**), test pattern modes. AC: each gated by capabilities.
- **M4-T09 Output configuration.** Camera/physical camera selection, preview/stream size, FPS range, JPEG quality, stream orientation, still format/size (JPEG, RAW_SENSOR→DNG when supported), video size/bitrate/codec/container/audio on-off. Validate against supported stream combinations; return a clear error when a combination is impossible. AC: dependent controls update automatically after a configuration change.
- **M4-T10 "Probe everything" mode.** Generic key explorer: enumerate every `CaptureRequest.Key` the device lists as available, attempt to set it to each documented value in a safe sandboxed sequence, record accept/ignore/error and whether the result key reflected it. Output a per-device **control compatibility matrix**. AC: run on every real device available and commit the matrices to `docs/device-matrices/<model>-<android>.md`.
- **M4-T11 Control presets/profiles.** Save/load/export named presets (e.g. "TV screen capture": 50/60 Hz anti-banding, manual exposure, AWB locked, fixed focus, zoom framing). Presets applied atomically; stored on host, shareable as JSON. AC: documented schema; applying a preset yields the expected actual state.
- **M4-T12 Host control UI components** (consumed by M6): sliders with numeric entry, auto/manual toggles, lock buttons with live state badges, regions drawn on the viewfinder, reset-to-auto, per-control "actual vs requested" indicator. AC: Playwright tests with a fake device.
- **M4-T13 On-device state panels** completed: all current control states in the **left and right side panels** (never over the video), grouped (Exposure, Focus, WB, Lens, Output, Recording), updating live, readable at arm's length, with high-contrast mode. AC: screenshot tests; manual check on a real device that changes made from the host appear within the latency target. **[DEVICE]**
- **M4-T14 Docs:** `docs/camera-controls.md` complete, including the matrix links and a "filming screens" best-practice guide (flicker, moiré, exposure, focus). AC: reviewed against real captures.

---

### M5. Capture and media pipeline

- **M5-T01 Still capture** (JPEG full-res, optional RAW/DNG) with a snapshot of control state and capture result metadata embedded in the manifest (and EXIF where applicable). Capture coexists with the live stream when the stream-combination rules allow; otherwise a clearly documented fallback (briefly pause the stream) is used and reported. AC: still taken from host UI/CLI during live stream on a real device. **[DEVICE]**
- **M5-T02 Burst/sequence capture:** N frames at interval (or as fast as possible), with timestamps; sequence manifest. Option to build sequences from the live stream frames (cheap path) vs full-res stills (quality path). AC: both modes documented with their limits.
- **M5-T03 Video recording:** MediaRecorder or MediaCodec+MediaMuxer into app-specific storage, resolution/FPS/bitrate/codec/audio options, runs concurrently with live view when the hardware allows (otherwise documented fallback), start/stop/limit-duration, segment splitting for long recordings, crash-safe finalization (fragmented or recoverable output). AC: 10-minute recording playable; an app kill mid-recording leaves a usable file or a clean error. **[DEVICE]**
- **M5-T04 Local storage manager** on the device: manifest per item (id, type, timestamps, size, SHA-256, control snapshot, device info, host job id if any), free-space checks, quota, cleanup policy.
- **M5-T05 Transfer to host:** host pulls via sync (or the socket stream for small stills), verifies hash, resumes interrupted transfers, then optionally deletes from device (`delete_after_verify`, default on). Transfer progress events. Works while recording/streaming. AC: transfer interrupted by unplug resumes after reconnect with the correct final hash. **[DEVICE]**
- **M5-T06 Host media store:** SQLite schema + migrations (media, sequences, jobs, reports, devices), files under the data dir, integrity checks on startup, retention (age/size), export as zip, import existing files for analysis. AC: migration tests; concurrent access safe.
- **M5-T07 CLI:** `watchdog capture still|burst|record start|record stop`, `watchdog media list|get|export|rm`. AC: documented.
- **M5-T08 Offline behaviour:** recording keeps going when the link drops; items are synced on reconnect automatically. AC: tested on a real device by pulling the cable mid-recording. **[DEVICE]**

---

### M6. Web UI

- **M6-T01 Stack and build.** Choose a framework by ADR (static build; no CDN, no external fonts); built in CI, embedded via `rust-embed` with ETag and compression; works offline; dark/light theme; keyboard accessible; responsive down to tablet width. AC: bundle size budget documented and enforced in CI.
- **M6-T02 App shell and real-time channel.** WebSocket for state/events, REST for commands, reconnect with backoff, global connection banner, auth token prompt when LAN token is enabled.
- **M6-T03 Devices page:** list USB and discovered LAN devices, connect, pair (with pairing-code dialog), provision with step-by-step progress from M3-T13, status badges, restore/disconnect, app version/update prompt, multi-device selector.
- **M6-T04 Live view page:** viewfinder (canvas or `<img>` MJPEG, pick by ADR, with backpressure), stats (FPS, bitrate, latency, drops), fullscreen, snapshot button, overlays drawn **in the browser** (focus/metering regions, face boxes, grid) never baked into video. Control panels to the **left and right** of the video mirroring the device layout.
- **M6-T05 Controls UI:** all M4-T12 components wired up for every supported control, preset manager, "actual vs requested" indicators, reset all.
- **M6-T06 Capture page/panel:** still, burst, record controls with live recording timer and transfer progress.
- **M6-T07 Media library:** grid/list, filters, preview (image viewer with zoom, video player), metadata and control snapshot viewer, download, delete, multi-select, send-to-analysis.
- **M6-T08 Settings page** (ports, listen mode, token, storage, retention, ffmpeg info, logs viewer, about/version, diagnostics bundle download).
- **M6-T09 E2E tests** with Playwright against the host running a **mock device** (CI), covering every page. AC: all green in `ci.yml`.
- **M6-T10 Accessibility and UX pass** (labels, contrast, focus order), plus empty/error/loading states. AC: axe checks pass in CI.
- **M6-T11 Docs:** annotated screenshots in `docs/user-guide.md` (captured from the real running UI against a real device in the verification run). **[DEVICE]**

---

### M7. Analyzers

All analyzers implement one trait (`id`, `description`, `inputs`, `params schema`, `run(input, params) -> Report`), report JSON follows a versioned schema, every finding has timestamps/frame indexes, severity and an evidence thumbnail reference.

- **M7-T01 Analyzer framework:** registry, parameter validation, cancellation, progress events, report storage, JSON schema published in docs, HTML report renderer.
- **M7-T02 ffmpeg/ffprobe bundling:** per-OS/arch binaries produced in CI (documented, reproducible build recipe, LGPL configuration, checksums), embedded/extracted with verification, `watchdog doctor` shows versions and enabled filters. AC: runs with none installed; license notice in `docs/licenses.md`.
- **M7-T03 Media probe:** ffprobe wrapper (codec, resolution, fps, duration, bitrate, audio streams, rotation).
- **M7-T04 Black screen analyzer:** `blackdetect` + configurable luma/pixel thresholds, minimum duration; also detects "all-one-colour" screens (white/green/blue). AC: fixture tests with exact expected intervals ± tolerance.
- **M7-T05 Frozen/stuck analyzer:** `freezedetect` plus an independent native frame-hash/diff approach for image sequences; noise tolerance parameter; region mask support (ignore clock/ticker areas). AC: fixtures with subtle motion vs true freeze.
- **M7-T06 Audio analyzers:** silence (`silencedetect`), loudness (`ebur128`), clipping, dropouts.
- **M7-T07 Native image-diff (Rust):** pixel diff and perceptual metric (SSIM or similar implemented natively), per-region masks, tolerance/threshold, diff heatmap output, size-mismatch handling, batch compare over sequences. AC: golden tests; benchmark documented.
- **M7-T08 Template/element detection:** native template matching for known UI elements (buffering spinner, logos, error dialogs) with multi-scale option and confidence; template library management in UI/API. AC: fixtures including rotating spinner.
- **M7-T09 Image quality analyzers:** brightness/exposure histogram, clipping %, sharpness/blur score, colour cast, moiré/flicker banding indicator (important when filming screens).
- **M7-T10 Buffering detector:** correlates spinner detection, freeze periods and audio gaps into buffering events (count, total, longest, first start). AC: fixture with 3 known buffering events found exactly.
- **M7-T11 Playback quality analyzer:** effective FPS and variance, duplicate/dropped frames, stutter score, resolution-change/quality-switch heuristics (sharpness step detection), A/V sync estimate when a flash/beep marker is present (documented marker approach).
- **M7-T12 Start-up time analyzer:** from a supplied trigger timestamp (e.g. from impresario) to first non-black, non-spinner, stable frame.
- **M7-T13 Optional reference comparison:** PSNR/SSIM (and VMAF only if the bundled build supports it, otherwise documented as unavailable).
- **M7-T14 OTT rollup report:** one report combining all of the above with user-defined **rules** (e.g. `max_black_ms: 500`, `max_buffering_events: 0`), per-rule pass/fail, overall verdict, plus a plain-language summary. Rules stored as named profiles. AC: schema documented; sample reports in `docs/analysis.md`.
- **M7-T15 Pipeline and UI integration:** auto-analysis after capture (optional), analysis page with timeline view, problem-frame thumbnails, compare-two-images view, report download (JSON/HTML), re-run with different params.
- **M7-T16 Performance:** analyze a 10-minute 1080p recording in a documented time budget on a reference machine; memory bounded (streaming, no loading whole video).
- **M7-T17 Real-world validation [DEVICE]:** film a real screen playing video with induced black frames, freeze and buffering (e.g. pause network), run the rollup, and record precision/false-positive notes in the verification file.

---

### M8. Scheduler

- **M8-T01 Job model:** job = device + capture plan (still/burst/record + duration + control preset) + analysis profile + outputs/notifications + schedule. Stored in SQLite with migrations.
- **M8-T02 Schedule engine:** cron expressions plus friendly presets (every N minutes/hours, daily at, weekdays), one-off jobs, time zone aware, DST-safe, jitter option, enable/disable, run-now.
- **M8-T03 Execution policy:** concurrency per device (queue/skip/replace), missed-run policy after downtime (skip/run once/catch up), timeouts, retries with backoff, auto-provision if the device reconnected, graceful behaviour when the device is absent.
- **M8-T04 History and results:** run log, durations, status, link to media and report, trend metrics (e.g. black screens per run), retention.
- **M8-T05 Notifications:** webhook (JSON), plus a documented hook for email via webhook; configurable on failure/on rule violation/always. AC: signed payloads optional.
- **M8-T06 UI:** schedule editor with validation and next-run preview, job list, history, charts of metrics over time.
- **M8-T07 API/MCP/CLI:** `watchdog schedule add|list|run|enable|disable|rm`, REST endpoints, MCP `run_schedule_now`.
- **M8-T08 Reliability:** survives host restart and device unplug, with tests using a fake clock. **[DEVICE]** unattended run overnight with a report of all runs.

---

### M9. MCP server and Integration API

- **M9-T01 API server:** versioned `/api/v1`, resources for devices, controls, state, capture, media, analyses, reports, schedules, presets, templates, system; consistent error model from M1-T05; pagination, filtering; idempotency keys for capture/start calls; request size limits; CORS off by default and configurable.
- **M9-T02 WebSocket event stream** `/api/v1/events` and live frames `/api/v1/devices/{id}/live`; event schema documented; replay-last-state on connect.
- **M9-T03 OpenAPI generated from code** (e.g. `utoipa`), served and checked by snapshot tests in CI; Swagger/Redoc-style viewer embedded.
- **M9-T04 Auth:** bearer token on all three ports when configured; constant-time compare; token rotation command; clear 401/403 behaviour.
- **M9-T05 MCP server:** Streamable HTTP on the MCP port and `--stdio` mode. Tools (each with precise JSON schema, descriptions written for an LLM, small structured outputs): `list_devices`, `connect_device`, `provision_device`, `get_device_state`, `get_camera_controls`, `set_camera_controls`, `apply_preset`, `lock_white_balance`, `lock_focus`, `lock_exposure`, `capture_image` (returns image content, size-limited and downscaled option), `capture_sequence`, `get_live_frame`, `start_recording`, `stop_recording`, `list_media`, `get_media_image`, `analyze_image`, `diff_images`, `analyze_video` (returns report + selected key frames, never the video), `run_schedule_now`. Resources: media index, latest reports. AC: tested with a real MCP client against a real device (capture, change exposure, re-capture, diff). **[DEVICE]**
- **M9-T06 MCP safety:** destructive tools (delete media, restore device) require explicit parameters; image payload size caps; rate limits; clear error messages that tell the model what to do next.
- **M9-T07 impresario integration:** a thin documented contract (start capture with trigger timestamp and correlation id, wait for completion via WebSocket or poll, fetch verdict JSON, fetch evidence frames) in `docs/impresario-integration.md`, a reference client (small Rust crate and curl examples), and a **proposed** impresario DSL mapping (for example a check step that references a watchdog analysis profile) written as a proposal only, not changes inside impresario. AC: example script drives a full capture+analyze loop against the real host and device. **[DEVICE]**
- **M9-T08 Contract tests:** API and MCP schema snapshot tests so accidental breaking changes fail CI; example requests in docs are executed in CI against the mock device.
- **M9-T09 Docs:** `docs/api.md`, `docs/mcp.md` (with client config examples), integration guide complete.

---

### M10. Hardening and release

- **M10-T01 Security review:** threat model in `docs/security.md`, dependency audit (cargo-deny/audit, Gradle dependency check), input fuzzing for adb codec/protocol/API, path traversal tests, upload limits, token/log redaction tests.
- **M10-T02 LAN mode:** `--listen` handling per port, token enforcement test matrix, warnings when exposed without a token, optional TLS (self-signed generation or user-supplied cert) documented.
- **M10-T03 Multi-device:** two devices simultaneously (UI, API, MCP, scheduler) without cross-talk; resource limits and per-device queues. **[DEVICE]** if two devices are available, otherwise document untested.
- **M10-T04 Soak test [DEVICE]:** at least 1 hour continuous streaming plus scheduled captures and analyses on a real device; record memory/CPU/thermal and any dropped sessions; fix leaks found.
- **M10-T05 Failure-mode matrix:** unplug, Wi-Fi loss, app crash, camera taken by another app, host sleep/wake, disk full, ffmpeg crash, DB lock. Each has an automated test where possible and a real-device check where not, with documented behaviour.
- **M10-T06 Upgrade path:** host updates the app, DB migrations forward-only with backups, config migration, protocol compatibility matrix.
- **M10-T07 Packaging:** release workflow produces six host binaries + checksums + SBOM + signed APK + attached docs; optional OS-specific niceties (macOS notarization/Gatekeeper notes, Windows SmartScreen notes, Linux AppImage/tar.gz) documented honestly if not done.
- **M10-T08 Final docs pass:** every doc in section 5 reviewed by following it from scratch on a clean machine (new user test), fix gaps.
- **M10-T09 Final verification [DEVICE]:** full section 7 checklist on the **tagged release artifacts**, results in `docs/verification/<date>-release.md`, `CHANGELOG.md` and release notes written.

## 9. Constraints and rules of engagement

- No runtime dependency on anything installed on the host machine (adb, ffmpeg, SDK, browser extensions, Python, Node).
- No unbounded memory growth on the live path; drop frames instead of buffering.
- Public interfaces (CLI, API, MCP tool schemas, protocol, report JSON) are contracts: version them and test against snapshots.
- Keep commits small, with meaningful messages; update docs in the same change as the code.
- If you find a requirement impossible on some devices or OS versions, do not silently skip it: document the limitation, detect it at runtime and report it in the UI.
- If a decision is not covered here and it is expensive to reverse, ask before building on it.
