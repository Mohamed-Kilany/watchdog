# Verification records

Physical-device verification runs for watchdog milestones, one file per milestone:
`docs/verification/<date>-<milestone>.md`.

Start from [TEMPLATE.md](TEMPLATE.md). Records must state: artifact id (CI run id + commit
SHA), checksums, device model(s) + Android version(s)/OEM(s), exact steps executed,
results, screenshots, and sample output reports. Failures are tracked as issues and listed
at the bottom of the record — never hidden.

Rules (from TASK.md §6/§7):

- Verify with **CI release artifacts**, never local debug builds.
- Machine must have **no adb, no ffmpeg, no Android SDK** installed (and, where runnable,
  a competing adb server on 5037 for the isolation test).
- Checklist per milestone is in TASK.md; the template expands it.
- State explicitly what was *not* verified and why.

| Date | Milestone | Artifact | Devices | Verdict |
|---|---|---|---|---|
| _pending_ | M1 (foundations) | n/a — CLI only | n/a | see PROGRESS.md |
| _pending_ | M3 (Android MVP) | CI `android` job APK | real phone (USB + LAN) | method: [m3-measurements.md](m3-measurements.md) |
| _pending_ | M4-T01 (capabilities) | CI `android` job APK | every available phone | method: [m4-capabilities.md](m4-capabilities.md) |
