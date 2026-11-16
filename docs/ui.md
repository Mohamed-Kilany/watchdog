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
and needs no phone or emulator.

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

`ui/index.html` + `ui/src/demo.ts` render a fake 16:9 viewfinder, the metering/focus
overlays, the control panel, a command log, drift and reset-all buttons, and a theme
toggle. `window.__watchdog` exposes the fake device, panel, state snapshot and command log
for Playwright assertions. The fake device is not a protocol implementation; it only
mirrors the state/implication semantics needed by the components.
