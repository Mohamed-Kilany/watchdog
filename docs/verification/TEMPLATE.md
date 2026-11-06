# Verification record — <date> — <milestone>

Use for any milestone that ships device-facing functionality. Copy this file to
`docs/verification/<YYYY-MM-DD>-<milestone>.md` and fill it in completely. Sections may be
marked `N/A — reason` but never deleted.

## Artifact under test

- CI run id / workflow: `…`
- Commit SHA: `…`
- Artifacts tested: `watchdog-linux-x64-<version>.tar.gz` (etc.)
- Checksums (from release `SHA256SUMS`) — paste `sha256sum -c` output:
- Which machine ran the test: OS, CPU, RAM; **adb/ffmpeg/Android SDK NOT installed** (state versions if any exception was needed)
- Was a competing adb server bound to 5037 during the run? yes/no (expectation: unaffected)

## Devices used

| # | Model | OEM skin / version | Android SDK | USB | LAN (pairing) | Notes |
|---|---|---|---|---|---|---|
| 1 | | | | | | |
| 2 | | | | | | |

If fewer than two OS versions/OEMs were available, state exactly what was available.

## Environment

- Host OS + version; kernel; watchdog binary SHA-256; ffmpeg bundled version (`watchdog doctor` output — paste); data dir used.
- Network: same VLAN / isolated; router notes if pairing used.

## Checklist — universal (from TASK.md §7)

- [ ] Zero-touch install + permission grant completed with no device taps (besides OS debug-authorization prompt)
- [ ] Live view runs with sane latency (state measured ms)
- [ ] Every listed control changed from host and reflected in **reported** device state
- [ ] AE/AWB/AF locks verified by observing reported state values
- [ ] Still capture; burst (N frames); video record (duration: …)
- [ ] Push to host with hash verified (paste SHA-256 match output)
- [ ] Link drop during recording → recovered; sync completed later
- [ ] USB unplug → reconnect → re-provision/reconnect works
- [ ] Analysis of a recording incl. induced black screen and freeze → expected report attached
- [ ] Scheduler run executed unattended (log attached)
- [ ] MCP `capture_image` + `analyze_image` from a real MCP client (client name/version; transcript attached)
- [ ] End-to-end call from impresario (or documented stand-in) through the integration API
- [ ] Runs with no adb, no ffmpeg, no Android SDK on the machine; and with a competing adb server on 5037

## Checklist — <milestone-specific>

(Copy the milestone's [DEVICE] criteria from TASK.md and tick each.)

## Results and evidence

- Provisioning log (step pass/fail + timings):
- Latency measurements (method + numbers):
- FPS/CPU/thermal over 30 min / soak:
- Screenshots: `./<date>-<milestone>/<name>.png` (committed)
- Sample reports: `./<date>-<milestone>/report-*.json|html`
- Log excerpts: `./<date>-<milestone>/…`

## Failures found (do not hide)

| # | Severity | Description | Issue link | Workaround |
|---|---|---|---|---|
| 1 | | | | |

## Verdict

- Release gate: PASS / PASS WITH ISSUES / FAIL
- What was explicitly NOT verified:
- Sign-off:
