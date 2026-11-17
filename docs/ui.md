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
URL-token consumption and reconnect/backoff.

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
| `/api/state` | merged control/camera snapshot, or `{ "waiting": true }` before the first `STATE` |
| `/api/command` | `POST` one command through the frame hub and return its `COMMAND_RESULT` |
| `/ws` | RFC 6455 WebSocket for state snapshots, events and command replies |
| `/stream.mjpg`, `/api/stats`, `/healthz` | live MJPEG stream, hub stats and liveness probe |

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

`GET /api/state` returns the merged snapshot the hub keeps:

```json
{
  "type": "state",
  "seq": 42,
  "timestamp_ms": 123456,
  "camera_id": "0",
  "state": { "iso": { "supported": true, "actual": 400, "source": "host" } },
  "dropped_frames": 0,
  "storage_free_mb": 2048
}
```

Before the app sends its first `STATE`, the response is
`{"type":"state","waiting":true,"seq":0,"timestamp_ms":0,"camera_id":"","state":{}}`.

`POST /api/command` takes a protocol `CommandMessage` body
(`{"id","command","params"}`) and returns the `COMMAND_RESULT` envelope
(`{"type":"result","id","timestamp_ms","results","error"}`). Invalid bodies and empty ids
are `400`, a closed app session is `503`, a command timeout is `504`, and other app
failures are `502`. Commands go through the frame hub because it owns the only app
session; opening a second `LiveSession` would replace the first.

### WebSocket

`GET /ws` performs the RFC 6455 handshake (token via `?token=` or
`Authorization: Bearer`) and then streams envelopes:

| `type` | Meaning |
|---|---|
| `hello` | channel opened, includes the protocol version |
| `state` | merged snapshot; `STATE` deltas are merged by the hub before forwarding |
| `event` | app `EVENT` (`camera_disconnected`, ...) |
| `result` | reply to a `command` message sent on the same socket |
| `error` | invalid client message or command failure |
| `closed` | the app session ended |

Clients may send `{"type":"command","id","command","params"}`, `{"type":"get_state"}` or
`{"type":"ping"}`. The shell itself uses REST for commands, so its socket only carries
state/events. The server rejects unmasked client frames, extensions, control frames that
are fragmented or longer than 125 bytes, and messages over 1 MiB. A slow subscriber that
lags the bounded broadcast gets a fresh snapshot instead of replaying every intermediate
delta.

### Reconnect and banner

`HostClient` reconnects with exponential backoff: 500 ms, 1 s, 2 s, ... capped at 15 s,
with ±20% jitter. The attempt counter resets after a successful open. A global banner
under the top bar shows `Connecting to the host...` / `Connection lost — reconnecting...`
and disappears once the socket is live. State snapshots replace the local map because the
hub already merged deltas; app events are appended to the command log.

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
| `wd-actual` | Requested vs actual readout with `match`/`differs`/`auto`/`pending` badge | `control-id`, `label`, `requested`, `actual`, `unit`, `precision`, `source`, `supported` | — |
| `wd-select` | Enum control | `control-id`, `label`, `options`, `value`, `disabled` | `wd-commit` `{id, value}` |
| `wd-reset` | Reset-to-auto button for a group of control ids | `label`, `targets`, `disabled` | `wd-reset` `{targets}` |
| `wd-control-panel` | Groups descriptors, renders rows and translates child events | `descriptors`, `state`, `heading`, `disabled` | `wd-control-change` `{id, value}`, `wd-control-reset` `{targets}` |

`ui/src/descriptors.ts` is the demo descriptor set. Its ids are the host's `state.<id>`
names (`iso`, `exposure_time_ns`, `awb_lock`, ...), so M6 can feed a real
`device controls --json` snapshot into the same panel. A slider descriptor's `scale`
converts UI units to device units (for example milliseconds to nanoseconds); the panel
applies it in both directions.

### Region overlay

The overlay normalizes coordinates to `0..1` and emits the same `{x, y, w, h, weight}`
shape as `ae_regions` / `af_regions`. Pointer drag draws a region; a click creates a
20% x 20% region centered on the tap. Keyboard: arrows move by 2%, Shift+arrows resize by
5%, Enter applies, Escape cancels. The demo sends `af_regions` plus `af_trigger=start`
for focus regions and `ae_regions` for metering.

### Accessibility

- Native range, number and select elements keep their platform keyboard behavior.
- Auto toggles use `role="switch"` and `aria-checked`; lock buttons use `aria-pressed`.
- Live badges are `role="status"` / `aria-live="polite"`.
- The region overlay is focusable only while editable and has an explicit keyboard
  contract in its accessible name.
- The app has a skip link, visible focus rings and a dark/light palette driven by
  `data-theme`; the layout is responsive to 768 px.

## Demo and fake device

`ui/index.html` + `ui/src/app.ts` render a fake 16:9 viewfinder, the metering/focus
overlays, the control panel, a command log, drift and reset-all buttons, and a theme
toggle. The shell is backend-agnostic: `FakeDevice` keeps the offline demo working and
`HostClient` drives the same panel, overlays and log from the host channel. The mode tag
shows `demo device` or `host`; the drift button is hidden in host mode. `window.__watchdog`
exposes the active backend, panel, state snapshot and command log for Playwright
assertions. The fake device is not a protocol implementation; it only mirrors the
state/implication semantics needed by the components.

Host resets are sent as one `set_controls` batch: exposure returns `ae_mode` to auto,
focus returns `af_mode` to continuous, white balance returns `awb_mode` to auto, lens
returns `zoom_ratio` to 1, and regions are cleared with a full-frame region of weight 0
(the app rejects an empty region array). Region edits are wrapped in an array because the
app's validator expects `ae_regions` / `af_regions` as arrays.
