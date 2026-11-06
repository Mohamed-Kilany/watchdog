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

- [ ] M2-T01 Transport trait
- [ ] M2-T02 Packet codec
- [ ] M2-T03 Authentication
- [ ] M2-T04 USB transport **[DEVICE]**
- [ ] M2-T05 TCP transport + LAN connect
- [ ] M2-T06 mDNS discovery
- [ ] M2-T07 Wireless-debugging pairing + TLS **[DEVICE]**
- [ ] M2-T08 Stream multiplexer
- [ ] M2-T09 Shell v2
- [ ] M2-T10 Sync protocol: **[DEVICE]**
- [ ] M2-T11 Port forwarding in-process:
- [ ] M2-T12 Package management: **[DEVICE]**
- [ ] M2-T13 Device manager:
- [ ] M2-T14 Fake device
- [ ] M2-T15 CLI:
- [ ] M2-T16 Isolation tests
- [ ] M2-T17 Docs:

## M3. Android app MVP and zero-touch provisioning

- [ ] M3-T01 Project setup
- [ ] M3-T02 Manifest and permissions
- [ ] M3-T03 Capture foreground service **[DEVICE]**
- [ ] M3-T04 Camera controller (Camera2)
- [ ] M3-T05 Frame pipeline
- [ ] M3-T06 Transport server
- [ ] M3-T07 Handshake and device info
- [ ] M3-T08 On-device UI (MVP)
- [ ] M3-T09 Log ring buffer
- [ ] M3-T10 Resilience
- [ ] M3-T11 Tests
- [ ] M3-T12 Embedding
- [ ] M3-T13 Provisioning pipeline
- [ ] M3-T14 Frame receiver + broadcast hub
- [ ] M3-T15 Minimal test page **[DEVICE]**
- [ ] M3-T16 CLI:
- [ ] M3-T17 Measurements

## M4. Controls and state (the feature-rich core)

- [ ] M4-T01 Capability descriptor
- [ ] M4-T02 State model and sync
- [ ] M4-T03 Command pipeline
- [ ] M4-T04 Exposure
- [ ] M4-T05 Focus **[DEVICE]**
- [ ] M4-T06 White balance and colour **[DEVICE]**
- [ ] M4-T07 Zoom and lens
- [ ] M4-T08 Flash/torch, scene and image processing
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
