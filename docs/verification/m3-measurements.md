# M3 measurements — method and recording sheet

Status: **pending real hardware** (M3-T17 is a **[DEVICE]** task). This file fixes the
method and the fields to fill so the run is reproducible and comparable across devices.
Do not invent numbers: leave the result tables as `pending` until the run is executed.

## Artifact under test

- CI run id / workflow: `pending`
- Commit SHA: `pending`
- APK version / SHA-256 / signer (from `watchdog version`): `pending`
- Host OS + CPU + RAM: `pending`
- Phone model / OEM skin / Android SDK: `pending`
- Connection: USB and wireless debugging over LAN (`watchdog serve --serial host:port`)

## 1. Glass-to-glass latency

The number reported by `/api/stats` (`latency_ms`) is the **sensor-to-host** estimate
from the PING/PONG clock-offset fit. Glass-to-glass additionally includes the phone
display/capture path and the host browser's paint latency, so it must be measured
optically.

Method:

1. Open a full-screen millisecond counter on a monitor (a local HTML page using
   `performance.now()` or `Date.now()` rendered at 1 ms resolution). Keep the monitor
   at a fixed brightness and disable adaptive brightness.
2. Point the phone camera at the monitor and open `watchdog serve` on the host. Put the
   host display next to the monitor so both are visible to the measurement camera.
3. Film both screens with a 240 fps (or faster) camera. Extract frames where the
   counter is readable on the monitor **and** in the host page.
4. For each sample, `glass_to_glass = host_counter_ms - monitor_counter_ms` (modulo the
   counter wrap), corrected by the measurement camera's frame interval.
5. Take 30 samples per transport; report median, p95 and min/max. Do not cherry-pick.

| Transport | Samples | Median (ms) | p95 (ms) | Min (ms) | Max (ms) | `/api/stats` latency_ms (median) |
|---|---|---|---|---|---|---|
| USB | pending | pending | pending | pending | pending | pending |
| LAN (wireless debugging) | pending | pending | pending | pending | pending | pending |

## 2. FPS and bitrate

Sample `/api/stats` every 10 s for 30 minutes at 1280×720 (the M3-T05 target is a
sustained 15 fps, stretch 30 fps). Record the median and the 5th percentile; note every
drop below 15 fps with its timestamp.

| Metric | USB | LAN |
|---|---|---|
| Median FPS | pending | pending |
| 5th-percentile FPS | pending | pending |
| Median bitrate (Mbit/s) | pending | pending |
| Dropped frames (host `dropped`) | pending | pending |
| Duplicate/stale frames (`duplicates`) | pending | pending |
| Session uptime / reconnects | pending | pending |

## 3. CPU, memory and thermal behaviour (30 minutes)

Every 5 minutes record:

- host: watchdog process CPU % and RSS (OS task manager or `Get-Process`/`ps`);
- phone: app CPU % from `watchdog device shell -- dumpsys cpuinfo` (grep
  `app.watchdog.remote`) and memory from `dumpsys meminfo app.watchdog.remote`;
- phone battery temperature and level from `watchdog device status`;
- thermal throttling state from `dumpsys thermalservice` (or
  `cat /sys/class/thermal/thermal_zone*/temp` where available).

| Minute | Host CPU % | Host RSS (MiB) | App CPU % | App RSS (MiB) | Battery °C | Battery % | Thermal status | FPS |
|---|---|---|---|---|---|---|---|---|
| 0 | pending | pending | pending | pending | pending | pending | pending | pending |
| 5 | pending | pending | pending | pending | pending | pending | pending | pending |
| 10 | pending | pending | pending | pending | pending | pending | pending | pending |
| 15 | pending | pending | pending | pending | pending | pending | pending | pending |
| 20 | pending | pending | pending | pending | pending | pending | pending | pending |
| 25 | pending | pending | pending | pending | pending | pending | pending | pending |
| 30 | pending | pending | pending | pending | pending | pending | pending | pending |

## 4. Pass/fail criteria

- No app crash, no camera-handle leak and no session restart during the 30 minutes.
- FPS stays at or above 15 fps at 1280×720 except for isolated recovery events, each
  explained in the record.
- Glass-to-glass latency is measured for USB and LAN and the method above is followed.
- All numbers above are filled from the actual run, with raw logs/screenshots attached
  next to this file (`docs/verification/<date>-m3/`).

## Failures found

| # | Severity | Description | Issue link | Workaround |
|---|---|---|---|---|
| 1 | | | | |

## Verdict

- Release gate: pending
- What was explicitly NOT verified: everything above until hardware is available.
- Sign-off: pending
