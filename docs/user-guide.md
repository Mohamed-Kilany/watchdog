# User guide

> Status: skeleton (M1). Sections are filled in as features land, with screenshots
> captured from real devices during verification runs (see `docs/verification/`).
> Annotated screenshots arrive with M6/M10.

## Contents

1. Installation & first run
2. Configuration (`watchdog.toml`, env, flags) — see [configuration.md](configuration.md)
3. Device management: list / connect / pair / provision / disconnect / restore
4. Live view: viewfinder, stats (FPS, latency, bitrate), fullscreen
5. Camera controls: exposure, focus, white balance, zoom, flash, presets, actual-vs-requested
6. Capture: still, burst, video recording, transfer progress
7. Media library: browse, preview, metadata, analysis dispatch
8. Analysis: choosing analyzers, the OTT rollup report, pass/fail rules
9. Scheduler: cron syntax, presets, job history, run-now, enable/disable
10. Settings: ports, token, storage, retention, logs, about
11. CLI reference: [cli.md](cli.md)

## 1. Installation & first run

Download the release binary for your platform (no installer required), then:

```bash
watchdog doctor      # verifies paths, ports, USB permissions hints
watchdog config path # where watchdog.toml lives (or would live)
watchdog --help      # full CLI
```

Data lives in the OS data dir (see [architecture.md](architecture.md#key-decisions-adrs)
and [adr/0005-data-directory-layout.md](adr/0005-data-directory-layout.md)); logs are
rotated daily in `<data_dir>/logs/`.

## 3. Device management (summary)

- `watchdog device list` — connected USB devices
- `watchdog device list --discover` — mDNS-discovered wireless-debugging endpoints
- `watchdog device pair <host:port> <code>` — pair a wireless-debugging endpoint (Android 11+)
- `watchdog device provision` — zero-touch install/permissions/launch (M3)
- Linux: `watchdog device udev-rules` prints (or `--write` installs) udev rules

Full walkthrough with OEM quirks: [device-setup.md](device-setup.md).

## 4. Live view (web UI)

Provision first, then start the UI:

```bash
watchdog device provision --serial <serial>
watchdog serve --serial <serial>
# open http://127.0.0.1:7700/
```

The host embeds the static web UI (see [ui.md](ui.md)) and serves the live MJPEG feed at
`/stream.mjpg` with frame-hub stats at `/api/stats`. Builds without an embedded UI serve
the minimal M3 live page instead. The UI reuses the session token stored by provisioning
and works over USB or wireless debugging (`--serial host:port`). When a bearer token is
configured, append `?token=…` to the page URL; the token is moved into session storage
and removed from the address bar.

The default page is the live view:

- **Viewfinder.** The phone's MJPEG preview, decoded by the browser with native
  backpressure. The stream opens only while this page is visible and a device session is
  active. If the session is closed or the host is unreachable, the page says so and
  reopens the stream automatically when the session returns.
- **Stats.** FPS, bitrate, latency, dropped frames and total frames from the host's frame
  hub, refreshed once a second while the stream is live.
- **Snapshot.** Downloads the next frame as `watchdog-snapshot.jpg`. Overlays are never
  drawn into the file; it is the camera frame the app sent.
- **Fullscreen.** Expands the viewfinder; use the overlay button or Escape to leave it.
- **Overlays.** Show the rule-of-thirds grid and face boxes reported by the app, and draw
  the metering, focus and white-balance regions on top of the video. Pick the active
  overlay with the toolbar radio buttons; Clear regions removes all three. Rotation
  reported by the app is applied to the preview and its overlays.
- **Controls.** The left panel holds Exposure, Focus and White balance; the right panel
  holds Lens, Flash & scene, Output and Recording, mirroring the phone's own layout.
  Sliders, enums, switches and one-shot triggers all report the value requested by the
  host next to the value the camera actually used. Readouts such as battery, measured FPS,
  AE/AF/AWB state and face count are live-only. Controls stay available while the preview
  is connecting; commands are logged at the bottom of the page.
- **Presets.** Name the current setup and Save it, pick a preset and Apply to send every
  stored value as one batch, or Delete the selected preset. A preset stores writable
  controls only; camera selection and stream layout are not included. Shareable JSON files
  remain available through `watchdog device preset export|import`.
- **Reset all.** Returns exposure, focus and white balance to auto, clears manual values
  and all regions, turns torch and face detection off, and restores the default output
  settings in one batch.

If no host answers the page probe, it falls back to the offline fake-device demo so the
controls can still be explored. Capture and the media library are wired up in the
remaining M6 tasks.

## 9. Scheduler (summary)

Jobs combine device + capture plan + analysis profile + cron (or preset like `every-30m`,
`daily@02:30`, `weekdays@09:00`). One-off and recurring runs, run history, last verdict,
enable/disable, run-now; notifications via webhook on failure/rule violation.

## 11. CLI reference

See [cli.md](cli.md) (kept in sync with `--help`; regenerated by
`scripts/generate-cli-docs.py`).
