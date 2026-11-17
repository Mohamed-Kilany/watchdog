# ADR-0008: Live view transport, snapshot and browser overlays

- Status: Accepted
- Date: 2026-11-17

## Context

M6-T04 requires a live view page with a viewfinder, FPS/bitrate/latency/drop
stats, fullscreen, a snapshot button and overlays drawn **in the browser**
(focus/metering regions, face boxes, rule-of-thirds grid). The controls must sit
to the left and right of the video, mirroring the Android layout. The page is
part of the static, framework-free UI from [ADR-0007](0007-web-ui-components.md)
and is served by the host's own origin; the device sends JPEG frames through the
`FrameHub` and the host already exposes them as a multipart MJPEG stream at
`/stream.mjpg`.

The viewfinder must not build an unbounded frame queue in JavaScript, must keep
working when the tab is busy, and must stay within the small bundle budget.

## Decision

1. **The viewfinder is a plain `<img>` on `/stream.mjpg`.**
   The host serves `multipart/x-mixed-replace` JPEG parts. Chromium decodes the
   stream natively and applies TCP backpressure; when the page cannot keep up,
   the socket stalls and the host's bounded broadcast drops old frames instead
   of buffering them in the browser. There is no JavaScript frame loop, no
   `Blob`/`createImageBitmap` pipeline and no extra decoder dependency.
   `LiveApi.streamUrl()` adds the bearer token as `?token=` (the `<img>` element
   cannot send an `Authorization` header) plus a `rev` cache-buster. The stream
   is opened only while the live route is visible and the host reports a live
   session, and is closed by removing `src` when either changes. A failed load
   shows the synthetic scene and retries after 2 s.

2. **Canvas is rejected for the viewfinder.** It would require parsing the
   multipart stream (or a second WebSocket frame channel) in JavaScript,
   decoding each frame, scheduling paints and bounding memory. That adds code,
   CPU and failure modes without improving latency over the browser's own image
   decoder. Canvas is also unnecessary for snapshots (below).

3. **Snapshots are server-side.** `GET /api/snapshot` subscribes to the hub,
   waits up to 2 s for the next frame and returns it as
   `image/jpeg` with `Content-Disposition: attachment;
   filename="watchdog-snapshot.jpg"`. The UI downloads the blob. This returns
   the authoritative camera frame even when the tab is hidden or the stream is
   reconnecting, and it never captures DOM overlays into the image. A canvas
   `toBlob()` would capture the currently painted frame plus overlays and would
   fail when the stream is paused.

4. **Overlays live in the DOM.** `wd-face-overlay` draws normalized boxes from
   `state.faces.actual`; the rule-of-thirds grid is a CSS background; the
   existing `wd-region-overlay` elements draw metering and focus regions. All of
   them sit above the `<img>` inside `.vf-media`, so they scale with the
   viewfinder and are never encoded into the stream or a snapshot. The app's
   `stream_rotation` value rotates `.vf-media` with a CSS transform and a
   JS-computed fit scale; the corner badge and status banner stay upright
   outside the rotated wrapper.

5. **Stats are polled, not inferred.** `GET /api/stats` is polled once a second
   while the stream is live; the interval stops when the route changes and
   individual polls are skipped while the document is hidden. FPS, bitrate,
   latency, drops and frames come from the hub, not from `<img>` events,
   because the element has no per-frame callback contract.

## Consequences

- The live view has no audio and no client-side recording. Preview quality and
  bandwidth are exactly the app's MJPEG output; recorded files stay the source
  of truth for analysis (M6-T06/M7).
- The host does the backpressure work through the existing bounded broadcast;
  lagging stream readers see the newest frame, not a growing backlog.
- A snapshot is the next frame after the click (within the 2 s timeout), not
  necessarily the exact frame on screen. That is acceptable for evidence
  captures and keeps the endpoint independent of the browser's paint state.
- Rotation is display-only. The metering/focus overlays rotate with the preview,
  but their pointer math assumes an unrotated frame, so interactive region
  editing is intended for the app's normal landscape orientation (0°). A
  portrait workflow would need the pointer mapping rotated too.
- The page adds no runtime dependency and stays inside the ADR-0007 bundle
  budget.
