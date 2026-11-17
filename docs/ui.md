# Host web UI

The host UI lives in `ui/`. It is a static Vite/TypeScript build of standards-based Web
Components, embedded by the host with `rust-embed`. The stack decision and its trade-offs
are in [ADR-0007](adr/0007-web-ui-components.md).

## Build and test

```sh
cd ui
npm ci
npm run typecheck
npm run build
npm run check:bundle
npm run test
```

`npm run test` starts `vite preview` on `127.0.0.1:4173` and runs Playwright Chromium
against the built `dist/`, so run `npm run build` first. The demo page uses a fake device
and needs no phone or emulator. `ui/tests/channel.spec.ts` also covers the host channel
with Playwright's WebSocket routing: token prompt, state/event frames, command REST calls,
URL-token consumption and reconnect/backoff. `ui/tests/devices.spec.ts` covers the
Devices page with route-mocked host APIs: demo notice, listing and activation, pairing,
provisioning progress plus the update prompt, and disconnect. `ui/tests/live.spec.ts`
covers the live view: demo scene, split control panels, host MJPEG stream plus stats
polling, face boxes/grid/rotation, snapshot download, fullscreen and pausing the stream
when the Devices page is open. `ui/tests/controls.spec.ts` covers every control kind
(slider, enum, switch, lock, trigger, readout, reset) and
`ui/tests/presets.spec.ts` covers preset save/apply/delete; `ui/tests/regions.spec.ts`
covers metering, focus and white-balance region editing. `ui/tests/capture.spec.ts` covers
the Capture page: routing, still/burst/recording in the demo, transfer progress, host
command forwarding and unsupported commands. `ui/tests/media.spec.ts` covers the Media
library: routing, filters/sort/layouts, still and video previews with zoom and metadata,
multi-select analysis/deletion, capture-to-library integration, host `/api/media` and
`/api/analyses` calls with the bearer token, and the missing-route notice.
`ui/tests/settings.spec.ts` covers the Settings page: routing, demo configuration and
logs, retention persistence, log filtering plus capture events, the demo diagnostics
bundle, host `/api/settings`, `/api/logs` and `/api/diagnostics` with the bearer token,
and the missing-route notice.

The CI `web` job runs exactly this sequence (`npm ci`, typecheck, build, bundle budget,
Playwright). The Playwright HTML report is uploaded only when the job fails.

## Bundle budget

`ui/scripts/check-bundle-size.mjs` measures gzip sizes of `ui/dist` and fails the build if
any limit is exceeded:

| Part | Budget |
|---|---|
| JavaScript | 64 KiB |
| CSS | 16 KiB |
| HTML | 8 KiB |
| Any single file | 48 KiB |
| Total | 96 KiB |

Source maps are rejected. There are no CDN imports, external stylesheets, web fonts or
runtime network requests; the UI uses the platform UI font stack and works offline.

## Embedding and serving

`crates/ui-server` embeds `ui/dist` with `rust-embed`
(`#[folder = "../../ui/dist/"]`, `allow_missing = true`). Release builds copy every file
into the host binary, so the shipped host serves the UI with no files on disk and no
network access. Debug builds read `ui/dist` from disk, so a `npm run build` is visible
without rebuilding the host. Rust-only builds (fmt, clippy, test jobs) compile with no UI
folder and serve the minimal live page described below instead.

`watchdog serve` serves the build on the UI port with these HTTP semantics:

| Path | Behaviour |
|---|---|
| `/` and extensionless routes | app shell (`index.html`) for client-side routing |
| `assets/*` | fingerprinted Vite assets, `Cache-Control: public, max-age=31536000, immutable` |
| other static files | `Cache-Control: no-cache` plus an ETag for cheap revalidation |
| `/api/state` | merged control/camera snapshot, or a waiting snapshot before a session/`STATE` |
| `/api/command` | `POST` one command through the frame hub and return its `COMMAND_RESULT` |
| `/api/devices*` | list, discover, pair, activate, disconnect, restore, provision and status |
| `/ws` | RFC 6455 WebSocket for state snapshots, events, session changes and command replies |
| `/stream.mjpg` | multipart MJPEG viewfinder stream (`token` accepted as a query parameter) |
| `/api/snapshot` | next hub frame as a `watchdog-snapshot.jpg` attachment (`503` without a frame/session) |
| `/api/presets` | `GET` list/summaries or one preset (`?name=`), `POST` save, `DELETE` (`?name=`) |
| `/api/stats`, `/healthz` | hub frame counters and liveness probe |

Every static response carries a weak content-hash ETag (`W/"<sha256>"`), answers
`If-None-Match` with `304 Not Modified`, and is gzip-compressed when the client sends
`Accept-Encoding: gzip` (text, JSON, SVG and Wasm responses of at least 512 bytes).
Compressed bodies are cached by ETag, and compressible responses send
`Vary: Accept-Encoding`. Bearer-token auth applies to static files, the stream and the
API alike.

When no UI was embedded, `/` and extensionless routes serve a self-contained live page
(MJPEG view plus stats) with `X-Watchdog-UI: fallback`; `watchdog version` prints
`embedded UI   not embedded`. `watchdog serve` also prints a hint when that happens.

## Real-time channel

The app shell talks to the host that served it; there is no separate configuration step.
`ui/src/host.ts` probes `GET /api/state` once on boot:

- `401 Unauthorized` opens the token dialog. The token is stored in `sessionStorage` and
  retried; a rejected token asks again with an error.
- A JSON `{"type":"state",...}` envelope switches to host mode.
- Anything else, including `vite preview` answering the probe with its own `index.html`,
  keeps the offline fake-device demo.

A `?token=...` in the page URL is moved into `sessionStorage` and removed from the address
bar before the probe, so it does not leak through history or screenshots.

### REST

`GET /api/state` returns the merged snapshot the hub keeps, plus the current session:

```json
{
  "type": "state",
  "seq": 42,
  "timestamp_ms": 123456,
  "camera_id": "0",
  "state": { "iso": { "supported": true, "actual": 400, "source": "host" } },
  "dropped_frames": 0,
  "storage_free_mb": 2048,
  "session": { "active": true, "closed": false, "serial": "R58R41VKXTY", "device": { } }
}
```

Before a session is activated, or before the app sends its first `STATE`, the response is
`{"type":"state","waiting":true,"seq":0,"timestamp_ms":0,"camera_id":"","state":{},"session":{...}}`.
`GET /api/stats` adds the same `session` field and returns zeroed counters while no session
is open.

`POST /api/command` takes a protocol `CommandMessage` body
(`{"id","command","params"}`) and returns the `COMMAND_RESULT` envelope
(`{"type":"result","id","timestamp_ms","results","error"}`). Invalid bodies and empty ids
are `400`, no active session is `503 DEVICE_OFFLINE`, a command timeout is `504` and other
app failures are `502`. Commands go through the frame hub because it owns the only app
session; opening a second `LiveSession` would replace the first.

### Device API

The Devices page talks to the host over `/api/devices*`; every route uses the same bearer
token and returns `{ "error": { "code", "message", "hint"? } }` on failure.

| Route | Purpose |
|---|---|
| `GET /api/devices` | active session plus USB and cached mDNS devices |
| `POST /api/devices/discover` | refresh the mDNS cache (5 s browse), then list |
| `POST /api/devices/pair` | `{target, code}` wireless-debugging pairing |
| `POST /api/devices/activate` | `{serial}` open the app session (replaces the current one) |
| `POST /api/devices/disconnect` | close the active session |
| `POST /api/devices/restore` | `{serial}` revert provisioning's screen settings |
| `POST /api/devices/provision` | `{serial, verify?, screen?, apk?}` start a background job (`202`) |
| `GET /api/devices/provision/{id}` | poll one job: status, steps, error, resulting session |
| `GET /api/devices/status?serial=...` | app install/version, service pid, appops and restore record |

A job streams every `StepReport` as it completes, keeps the last 20 jobs and auto-activates
the session after a fully passed run; `session_error` reports an activation failure without
failing the provisioning result. Device list rows merge the active session first, then USB,
then mDNS, deduplicated by serial. A discovered row exposes its `pairing` endpoint so the
pair dialog can prefill it.

### Live view

The default `#/live` page is described in
[ADR-0008](adr/0008-live-view-mjpeg-and-overlays.md). The viewfinder is a plain
`<img>` on `/stream.mjpg` with the bearer token in the query string; Chromium decodes the
multipart JPEG stream natively and TCP backpressure reaches the hub's bounded broadcast,
so no frame queue lives in JavaScript. `ui/src/live.ts` owns the page state:

- The stream opens only while the live route is visible and the host reports an active
  session over a live WebSocket. Closing the session or switching to `#/devices` removes
  `src` and stops polling; returning reopens the stream with a new `rev` value.
- A load failure shows the synthetic scene and retries after 2 s. Status text and the
  corner badge report demo, no session, connecting, waiting for frames or live with the
  device model.
- `/api/stats` is polled once a second while live and visible (not while the tab is
  hidden) and renders FPS, bitrate, latency, drops and total frames.
- Snapshot downloads `/api/snapshot` as `watchdog-snapshot.jpg`; the endpoint returns the
  next hub frame, so overlays are never baked into the image.
- The rule-of-thirds grid, face boxes (`wd-face-overlay` from `state.faces.actual`) and
  the metering/focus/white-balance region overlays are DOM elements above the stream.
  `stream_rotation` rotates the `.vf-media` wrapper with a CSS transform and a computed
  fit scale.
- Fullscreen uses the Fullscreen API on the viewfinder; an overlay button exits it (the
  toolbar button also updates its label), and Escape keeps the browser's native exit.
- The controls are split into a left panel (Exposure, Focus, White balance) and a right
  panel (Lens, Flash & scene, Output, Recording) around the video; below 1200 px the
  video spans the top row and the panels share the second row, and below 720 px
  everything stacks. The right panel also holds the preset manager.

`ui/src/live-api.ts` is the typed client for the stream URL, `/api/stats` and
`/api/snapshot`, and reuses the same bearer token as the rest of the UI API.

### Capture

The `#/capture` page (`ui/src/capture.ts`) drives the capture commands from
`docs/protocol.md` through the same `DeviceBackend` contract as the control panels:

| Action | Command | Params |
|---|---|---|
| Still | `capture_still` | `{}` |
| Burst | `capture_burst` | `{count, interval_ms, from_stream}` |
| Start recording | `record_start` | `{max_duration_secs}` when set |
| Stop recording | `record_stop` | `{}` |

In host mode the commands go over the existing authenticated `POST /api/command`; the
applied payload (`media_id`, `media_ids`, `sequence_id`, dimensions, bytes, duration) is
normalized into the page readouts. Until the M5 capture pipeline lands the Android
`CaptureService` answers `UNSUPPORTED_COMMAND`, which the page shows in the status line
instead of leaving the button stuck.

The page has three cards and a transfer list:

- **Still** reports the media id, dimensions and size of the last capture.
- **Burst** sends a frame count (1–50) and interval (0–5000 ms) and updates a progress
  bar from `capture_progress` events (`done`/`total`). `from_stream` asks the app to pull
  frames from the live stream instead of a full-resolution capture.
- **Recording** toggles `record_start`/`record_stop`, shows a live `mm:ss` timer fed by
  `recording_started`/`recording_stopped` events and the merged `recording` state, and
  sends `max_duration_secs` when a limit is set.
- **Transfers** render host `transfer` envelopes keyed by `transfer_id` with the media
  id, state badge, byte counts and a progress bar. Capture and recording keep running
  when the user switches to another route.

The demo `FakeDevice` simulates the whole flow: media ids, burst progress events,
recording lifecycle and a stepped transfer (`queued` → `transferring` → `verifying` →
`done`). `window.__watchdog.capture` exposes the controller for Playwright.

### Media library

The `#/media` page (`ui/src/media.ts`) is backend-agnostic like the capture page. It
lists items with a text search over id/type/MIME/device, type and device filters,
newest/oldest/largest sorting, and grid/list layouts. Each card has a selection checkbox
and an open button; selecting one or more items reveals bulk download, analysis and
delete actions. The preview dialog shows a still with zoom (buttons, Ctrl+wheel and
`+`/`-`/`0`) or a video with a generated poster and native controls, plus a metadata grid
(type, capture time, dimensions, duration, bytes, MIME, SHA-256, device, host job) and
the captured control snapshot as JSON. Download uses the original file URL and delete
removes the item from the library; both single-item actions live in the dialog.

`ui/src/media-api.ts` defines the UI-facing host contract, documented in
[api.md](api.md#media-and-analysis-ui-contract-m6-t07):

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/media` | list; accepts `{items: [...]}` or a bare array |
| `GET` | `/api/media/{id}` | one item; accepts `{item: {...}}` or the item |
| `GET` | `/api/media/{id}/file` | original bytes; token via `?token=` |
| `DELETE` | `/api/media/{id}` | delete |
| `POST` | `/api/analyses` | `{media_ids: [...]}`, returns `{id, state}` |

All JSON calls send `Authorization: Bearer <token>` when a token is stored. The host
media store lands with M5 and the analysis queue with M7; until those routes exist the
page shows `The host media library is not available yet (M5).` with the host error instead
of an empty grid.

The demo `FakeDevice` seeds three items (`m-demo-still-1`, `m-demo-video-1`,
`m-demo-still-2`) and appends every demo still, burst frame and recording to the same
in-memory library. Stills use a generated JPEG scene, the video uses the bundled
`ui/src/assets/demo-clip.webm`, and analysis returns a fake queued id.
`window.__watchdog.media` exposes the controller for Playwright.

### Settings

The `#/settings` page (`ui/src/settings.ts`) shows the host configuration in six cards:

- **Host & network**: UI/MCP/API ports, listen mode (localhost/LAN) and per-port
  addresses, the UI URL with a copy button, and the bearer-token state with its hint.
  A LAN port without a token shows a prominent warning.
- **Storage**: data/media/logs directories, media item count and bytes, free/total disk
  with a usage meter.
- **Retention**: editable maximum age (days) and maximum size (MiB); `0` keeps media
  forever or removes the size limit. Save uses `PATCH /api/settings`.
- **Bundled ffmpeg**: availability, ffmpeg/ffprobe versions, extracted path, license and
  filter count. The host never uses ffmpeg from `PATH`.
- **Logs**: level filter (all/error/warn/info/debug/trace), manual refresh, line count and
  a monospace viewer with time, level, target and message.
- **About**: host version, commit, build, platform, protocol and UI version, plus a
  diagnostics bundle download (zip on the host, generated JSON in the demo).

`ui/src/settings-api.ts` defines the UI-facing contract, documented in
[api.md](api.md#settings-logs-and-diagnostics-ui-contract-m6-t08):

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/settings` | configuration, storage, retention, ffmpeg and about |
| `PATCH` | `/api/settings` | `{retention: {max_age_days, max_size_mb}}` |
| `GET` | `/api/logs?level=&limit=` | recent log lines |
| `GET` | `/api/diagnostics` | zip bundle; token via `?token=` |

All JSON calls send `Authorization: Bearer <token>` when a token is stored. The host
configuration/ffmpeg/system routes land with M7/M9; until they exist the page shows
`The host settings API is not available yet (M7/M9).` with the host error instead of empty
cards. The demo `FakeDevice` reports configuration-shaped values, seeded logs that grow
with capture and control events, persists retention edits in memory and generates a JSON
diagnostics file. `window.__watchdog.settings` exposes the controller for Playwright.

### Controls and presets

`ui/src/descriptors.ts` describes every control the Android app reports, grouped like the
phone's own panel. A descriptor is one of four kinds:

| Kind | Rendered by | Behaviour |
|---|---|---|
| `slider` | `wd-slider` | Numeric range plus entry; `scale` converts UI units to device units |
| `select` | `wd-select` | Enum; `format: "size"` sends `{width, height}`, `format: "fps-range"` sends `[lower, upper]` |
| `readout` | `wd-actual` | Live-only value with a `live` badge; no requested value and no writes |
| `toggle` | `wd-toggle` | Boolean switch with `role="switch"`; sends `true`/`false` |
| `trigger` | `wd-trigger` | One-shot button (`af_trigger`, `ae_precapture`) that reports pending state |

Auto/manual pairs keep their `wd-auto-toggle` and lock buttons; region controls are edited
directly on the viewfinder. Every writable control is reset by `Reset all`, which sends one
`set_controls` batch: exposure/focus/white balance return to auto, manual values are
cleared, lens zoom returns to 1, torch and face detection turn off, output settings return
to their defaults, and all three region arrays are cleared with a full-frame weight-0
rectangle. `tonemap_curve`/`tonemap_gamma` are read-only in both the UI and the host.

The preset manager (`wd-preset-manager`, `ui/src/presets.ts`) captures the requested value
of every supported writable control, skipping read-only entries, one-shot triggers and
structural ids (`camera_id`, `stream_size`, ...). Applying a preset sends a single
`set_controls` batch with camera ids first and the rest in alphabetical order, matching
`docs/presets.md`. In host mode presets live under `<data_dir>/presets` behind
`/api/presets`; the demo stores the same schema in `localStorage`. Save/apply/delete are
in the page; shareable JSON stays in `watchdog device preset export|import`.

### WebSocket

`GET /ws` performs the RFC 6455 handshake (token via `?token=` or
`Authorization: Bearer`) and then streams envelopes:

| `type` | Meaning |
|---|---|
| `hello` | channel opened, includes the protocol version and the current session |
| `state` | merged snapshot; `STATE` deltas are merged by the hub before forwarding |
| `event` | app `EVENT` (`camera_disconnected`, ...) |
| `session` | the active device session changed (activated, closed or disconnected) |
| `transfer` | media transfer progress: `transfer_id`, `media_id`, `state`, `bytes_total`, `bytes_done`, optional `error` (UI contract; emitted once the M5 capture pipeline lands) |
| `result` | reply to a `command` message sent on the same socket |
| `error` | invalid client message or command failure |
| `closed` | the app session ended; the socket stays open for the next session |

Clients may send `{"type":"command","id","command","params"}`, `{"type":"get_state"}` or
`{"type":"ping"}`. The shell itself uses REST for commands, so its socket only carries
state/events. The server rejects unmasked client frames, extensions, control frames that
are fragmented or longer than 125 bytes, and messages over 1 MiB. A slow subscriber that
lags the bounded broadcast gets a fresh snapshot instead of replaying every intermediate
delta. After a `session` or `closed` envelope the server follows it with a state snapshot;
activating another device switches the socket to the new hub without a page reload.

### Reconnect and banner

`HostClient` reconnects with exponential backoff: 500 ms, 1 s, 2 s, ... capped at 15 s,
with ±20% jitter. The attempt counter resets after a successful open. A global banner
under the top bar shows `Connecting to the host...`, `Connection lost — reconnecting...`,
`No device session — open Devices...` or `The device session closed — open Devices...`,
and disappears while the socket is live with an active session. State snapshots replace
the local map because the hub already merged deltas; app events are appended to the
command log.

## Pages

The shell uses hash routes without a reload: `#/live` (default) is the camera/controls
page, `#/capture` is still/burst/recording with transfer progress, `#/media` is the
media library, `#/settings` is host configuration and diagnostics, and `#/devices` is
device management. The top bar keeps the active-device selector visible on every page;
choosing another device activates it and choosing “No device” disconnects.
`HostClient.applySession()` adopts the REST result immediately, and the `session`
WebSocket envelope keeps every other tab in sync.

The Devices page shows the active session card (model, serial, app version, status badge,
check status, provision/update, restore and disconnect), a known-device list with
activate/pair/provision/status actions, and the provisioning panel with one row per step
(name, status, duration, detail, hint). The app update prompt appears when the installed
`versionCode` differs from the embedded build. Without a host the page renders a demo
notice and leaves the offline controls demo untouched.

## CI flow

The `web` job builds `ui/dist`, enforces the budget and uploads it as the `ui-dist`
artifact. The `host-windows` release job downloads that artifact into `ui/dist` before
`cargo build --release`, then fails unless `watchdog version` reports
`embedded UI   <n> files from ui/dist`. Rust-only jobs do not download the artifact and
keep compiling through `allow_missing`.

## Components

All components are custom elements with open shadow roots. They emit composed custom
events, so a consumer can listen on the panel or on `document`.

| Element | Purpose | Main inputs | Event |
|---|---|---|---|
| `wd-slider` | Range slider plus numeric entry and an actual-value marker | `control-id`, `label`, `min`, `max`, `step`, `value`, `actual`, `unit`, `precision`, `disabled` | `wd-commit` `{id, value}` |
| `wd-auto-toggle` | Auto/manual switch with a live source badge | `control-id`, `label`, `auto`, `source`, `disabled` | `wd-auto-change` `{id, auto}` |
| `wd-lock-button` | Lock/unlock button with a live badge | `control-id`, `label`, `locked`, `pending`, `supported`, `disabled` | `wd-lock-change` `{id, locked}` |
| `wd-region-overlay` | Metering/focus region drawn over the viewfinder | `mode`, `label`, `regions`, `editable`, `maxRegions` | `wd-region-change` `{mode, region, regions}` |
| `wd-face-overlay` | Read-only normalized face boxes from `state.faces.actual` | `faces` | — |
| `wd-actual` | Requested vs actual readout with `match`/`differs`/`auto`/`pending` badge, or a live-only readout | `control-id`, `label`, `requested`, `actual`, `unit`, `precision`, `source`, `supported`, `readonly`, `options` | — |
| `wd-select` | Enum control; optional `size`/`fps-range` value encoding | `control-id`, `label`, `options`, `value`, `disabled` | `wd-commit` `{id, value}` |
| `wd-toggle` | Boolean switch with a live on/off badge | `control-id`, `label`, `checked`, `disabled` | `wd-toggle-change` `{id, on}` |
| `wd-trigger` | One-shot trigger button with a pending badge | `control-id`, `label`, `pending`, `disabled` | `wd-trigger` `{id}` |
| `wd-preset-manager` | Save/apply/delete named presets with a status line | `source` (preset source), `controls` (current writable values) | `wd-preset-apply` `{preset}`, `wd-preset-saved`/`wd-preset-deleted` `{name}` |
| `wd-reset` | Reset-to-auto button for a group of control ids | `label`, `targets`, `disabled` | `wd-reset` `{targets}` |
| `wd-control-panel` | Groups descriptors, renders rows and translates child events | `descriptors`, `state`, `heading`, `groups`, `disabled` | `wd-control-change` `{id, value}`, `wd-control-reset` `{targets}` |

`ui/src/descriptors.ts` is the demo descriptor set. Its ids are the host's `state.<id>`
names (`iso`, `exposure_time_ns`, `awb_lock`, ...), so M6 can feed a real
`device controls --json` snapshot into the same panel. A slider descriptor's `scale`
converts UI units to device units (for example milliseconds to nanoseconds); the panel
applies it in both directions.

### Region overlay

The overlay normalizes coordinates to `0..1` and emits the same `{x, y, w, h, weight}`
shape as `ae_regions` / `af_regions` / `awb_regions`. Pointer drag draws a region; a click
creates a 20% x 20% region centered on the tap. Keyboard: arrows move by 2%, Shift+arrows
resize by 5%, Enter applies, Escape cancels. The demo sends `af_regions` plus
`af_trigger=start` for focus regions, `ae_regions` for metering and `awb_regions` for white
balance; only the selected overlay is editable.

### Accessibility

- Native range, number and select elements keep their platform keyboard behavior.
- Auto toggles use `role="switch"` and `aria-checked`; lock buttons use `aria-pressed`.
- Live badges are `role="status"` / `aria-live="polite"`.
- The region overlay is focusable only while editable and has an explicit keyboard
  contract in its accessible name.
- The app has a skip link, visible focus rings and a dark/light palette driven by
  `data-theme`; the layout is responsive to 768 px.

## Demo and fake device

`ui/index.html` + `ui/src/app.ts` render a fake 16:9 viewfinder, the
metering/focus/white-balance overlays, the two control panels, a preset manager, a command
log, drift and reset-all buttons, and a theme toggle. The shell is backend-agnostic:
`FakeDevice` keeps the offline demo working and `HostClient` drives the same panels,
overlays, presets, capture page, media library, settings page and log from the host
channel. The mode tag shows `demo device` or `host`; the drift button is hidden in host
mode. The Devices page works in host mode only and shows a demo notice otherwise. The
demo `FakeDevice` also simulates still/burst captures, recording and stepped media
transfers, so `#/capture` is usable without a phone, keeps an in-memory media library
seeded with three items, so `#/media` is usable without a host, and reports demo
configuration, seeded logs and a generated diagnostics file, so `#/settings` is usable
offline. `window.__watchdog` exposes the active backend, the left panel (and both panels
via `panels`), the live view controller, the capture controller (`capture`), the media
controller (`media`), the settings controller (`settings`), the preset manager
(`presets`), the state snapshot and the command log for Playwright assertions. The fake
device is not a protocol implementation; it only mirrors the state/implication semantics
needed by the components.

Host resets are sent as one `set_controls` batch: exposure returns `ae_mode` to auto,
focus returns `af_mode` to continuous, white balance returns `awb_mode` to auto, lens
returns `zoom_ratio` to 1, and regions are cleared with a full-frame region of weight 0
(the app rejects an empty region array). Region edits are wrapped in an array because the
app's validator expects `ae_regions` / `af_regions` / `awb_regions` as arrays.
