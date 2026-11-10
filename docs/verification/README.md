# Verification records

Physical-device verification runs for watchdog milestones, one file per milestone:
`docs/verification/<date>-<milestone>.md`.

Start from [TEMPLATE.md](TEMPLATE.md). Records must state: artifact id (CI run id + commit
SHA), checksums, device model(s) + Android version(s)/OEM(s), exact steps executed,
results, screenshots, and sample output reports. Failures are tracked as issues and listed
at the bottom of the record — never hidden.

Rules (from TASK.md §6/§7):

- Verify with **CI release artifacts**, never local debug builds. The `android` workflow
  uploads `watchdog-host-windows-x86_64` (release host with the embedded APK) and
  `watchdog-android-apk` from the same run.
- Machine must have **no adb, no ffmpeg, no Android SDK** installed (and, where runnable,
  a competing adb server on 5037 for the isolation test).
- Checklist per milestone is in TASK.md; the template expands it.
- State explicitly what was *not* verified and why.

| Date | Milestone | Artifact | Devices | Verdict |
|---|---|---|---|---|
| _pending_ | M1 (foundations) | n/a — CLI only | n/a | see PROGRESS.md |
| 2026-11-09 | M2 wireless ADB (pairing/TLS, install/update) | CI run 42576110391, commit `720e281` | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-09-m2-wireless.md) |
| _pending_ | M3 (Android MVP) | CI host + APK artifacts | real phone (USB + LAN) | method: [m3-measurements.md](m3-measurements.md) |
| 2026-11-09 | M4-T01 (capabilities) | CI run 42576110391, commit `720e281` | Samsung SM-A515F, Android 13 (4 cameras) | PASS — dumps in [2026-11-09-a51/](2026-11-09-a51/) |
| 2026-11-09 | M4-T02/T03 (state sync, command pipeline) + camera recovery | CI run 42585703374, commit `ac6ce1f` (merged `ba5c532`) | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-09-m4-controls.md) |
| 2026-11-09 | M4-T04 (exposure: AE FPS range, AE regions, precapture) | CI run 42589225330, commit `e494301` (merged `595e513`) | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-09-m4-exposure.md) |
| 2026-11-09 | M4-T05 (focus: AF mode, AF lock, tap-to-focus) | CI run 42599576938, commit `a1676c4` (merged `3f06d82`) | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-09-m4-focus.md) |
| 2026-11-09 | M4-T06 (white balance: AWB presets, lock, regions, colour effects) | CI run 42612624713, commit `8dbda76` | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-09-m4-wb.md) |
| 2026-11-10 | M4-T07 (zoom/lens: zoom ratio, lens metadata, stabilization, shading, distortion) | CI run 42780597519, commit `cc4d4aa` | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-10-m4-lens.md) |
| 2026-11-10 | M4-T08 (flash/torch, scene, image processing: tonemap, face detect, test pattern) | CI run 42825780915, commit `6937705` | Samsung SM-A515F, Android 13 | PASS — [record](2026-11-10-m4-flash-scene.md) |
