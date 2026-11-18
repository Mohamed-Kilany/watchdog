# Integration API

> Status: skeleton (M1). Stable contracts: base path `/api/v1`, JSON bodies, error model
> below. OpenAPI is generated from code (`utoipa`) during M9 and served at
> `/api/v1/openapi.json`; this page is the prose contract until then.

## Base and versioning

- Base URL: `http://127.0.0.1:7702/api/v1`
- Versioning: URI prefix. Breaking changes ⇒ `/api/v2` (v1 remains served for the
  supported lifecycle). Additive changes (new fields/endpoints) do not bump.
- Content type: `application/json`; WebSocket upgrade for events and live frames.

## Authentication (LAN mode)

Localhost-only by default. When `--listen 0.0.0.0` (or per-port) is used, a bearer token
is required on **every port** (UI, MCP, API): `Authorization: Bearer <token>`. Constant-time
compare; tokens are never logged; 401 with `error.code = UNAUTHORIZED`. The UI's static
shell and assets are public so the token prompt can load; every dynamic UI route
(`/api/*`, `/stream.mjpg`, `/ws`, `/healthz`) still requires the token.

## Error model (M1-T05)

Every non-2xx response has this body:

```json
{
  "error": {
    "code": "PORT_IN_USE",
    "message": "port 7702 is already bound",
    "hint": "stop the other process or pass --api-port <port>"
  }
}
```

`code` is a stable screaming-snake string (see `ErrorCode` in
`crates/core/src/error.rs`); `hint` is optional but always actionable. Codes added over
time; existing codes never change meaning. Full code list lands with M9 (as does the
OpenAPI document enumerating them).

Initial codes: `CONFIG_INVALID`, `CONFIG_NOT_FOUND`, `DATA_DIR_NOT_WRITABLE`, `PORT_IN_USE`,
`IO_ERROR`, `INTERNAL`, `DEVICE_NOT_FOUND`, `DEVICE_UNAUTHORIZED`, `DEVICE_OFFLINE`,
`PAIRING_FAILED`, `TRANSPORT_ERROR`, `STREAM_CLOSED`, `PROTOCOL_ERROR`,
`PROTOCOL_VERSION_MISMATCH`, `INSTALL_FAILED`, `CONNECTION_LOST`, `APP_VERSION_MISMATCH`,
`PROVISIONING_FAILED`, `MEDIA_NOT_FOUND`, `MEDIA_INTEGRITY_MISMATCH`, `ANALYZER_ERROR`,
`ANALYSIS_FAILED`, `SCHEDULE_INVALID`, `JOB_NOT_FOUND`, `INVALID_REQUEST`, `NOT_FOUND`,
`UNAUTHORIZED`, `FORBIDDEN`, `PAYLOAD_TOO_LARGE`, `CONFLICT`, `NOT_IMPLEMENTED`.

## Resources (target, M9)

| Resource | Notes |
|---|---|
| `GET /devices` · `GET /devices/{id}` | list/detail with state badges |
| `POST /devices/{id}/provision` | runs the zero-touch pipeline; step events via WS |
| `GET /devices/{id}/controls` · `POST /devices/{id}/controls` | capability descriptor / set batch (idempotent via `Idempotency-Key`) |
| `GET /devices/{id}/state` · WS `/devices/{id}/events` | state snapshot + live event stream |
| `GET /devices/{id}/live` (WS) | live frames (binary JPEG) + metadata |
| `POST /devices/{id}/capture/still|burst|record` | capture lifecycle; returns media id |
| `GET /media` · `GET /media/{id}` · `GET /media/{id}/file` | library; no path traversal; size-limited |
| `POST /analyses` · `GET /analyses/{id}` | run analysis; fetch report (JSON) + evidence frames |
| `GET /schedules` · `POST /schedules` · `POST /schedules/{id}/run` | scheduler |
| `/api/v1/events` (WS) | typed event stream (see below) |
| `GET /openapi.json` | generated from code |

## Media and analysis UI contract (M6-T07)

The web UI uses the unprefixed `/api/media...` and `/api/analyses` routes, like the other
host UI routes (`/api/state`, `/api/command`, `/api/stats`). The media store (M5) and
analysis engine (M7) will back both this UI contract and the `/api/v1` integration API.
Until those routes exist the Media page shows a clear M5 notice instead of an empty
library. `watchdog serve --mock-device` implements the contract in memory against the
synthetic device (three seeded items, still/video captures, a queued fake analysis and
deletes) so the Playwright end-to-end suite can exercise the page; those mock routes are
not available with a real device session.

`GET /api/media` returns either `{"items": [item, ...]}` or a bare array. An item is:

```json
{
  "id": "m-01H...",
  "type": "still",
  "created_ms": 1700000000000,
  "duration_ms": 12400,
  "width": 1920,
  "height": 1080,
  "bytes": 1842000,
  "sha256": "<64 hex chars>",
  "mime": "image/jpeg",
  "device": {"model": "Pixel 7", "serial": "1A2B...", "sdk": 34},
  "control_snapshot": {"iso": 400, "exposure_time_ns": 8333333},
  "host_job_id": null
}
```

`duration_ms` is present for videos; `control_snapshot` and `host_job_id` are optional.
`GET /api/media/{id}` returns the same item, optionally wrapped as `{"item": item}`.

| Method | Path | Behaviour |
|---|---|---|
| `GET` | `/api/media/{id}/file` | Streams the original bytes with the item's `mime` and an attachment filename. Media elements cannot send headers, so the UI passes the bearer token as `?token=`. Missing media → 404 `MEDIA_NOT_FOUND`; a size/hash mismatch → 422 `MEDIA_INTEGRITY_MISMATCH`. |
| `DELETE` | `/api/media/{id}` | Removes the item and its stored file; 204 on success, 404 `MEDIA_NOT_FOUND` when absent. |
| `POST` | `/api/analyses` | Accepts `{"media_ids": ["m-...", ...]}` and returns `{"analysis_id": "an-...", "state": "queued"}` (`id` is accepted as an alias). Missing media → 404 `MEDIA_NOT_FOUND`; an engine with no analyzers → 501 `NOT_IMPLEMENTED`. |

All JSON calls send `Authorization: Bearer <token>` when a token is configured; file URLs
carry it as `?token=` so `<img>`/`<video>` can load them. Non-2xx responses use the error
model above, and the UI shows `error.message` (plus `error.hint`) in its status line.

## Settings, logs and diagnostics UI contract (M6-T08)

The web UI uses the unprefixed `/api/settings`, `/api/logs` and `/api/diagnostics` routes.
The host configuration (M1), log ring buffer (M1-T04), ffmpeg bundle (M7) and system
resources (M9) will back them. Until those routes exist the Settings page shows a clear
M7/M9 notice instead of empty cards. `watchdog serve --mock-device` serves in-memory
mock settings, a seeded log ring buffer, editable retention and a stored-zip diagnostics
bundle for the Playwright end-to-end suite; the mock reports ffmpeg as missing and uses
fixed disk totals.

`GET /api/settings` returns either the object below or `{"settings": {...}}`:

```json
{
  "ports": {"ui": 7700, "mcp": 7701, "api": 7702},
  "listen": {
    "mode": "localhost",
    "addresses": {"ui": "127.0.0.1", "mcp": "127.0.0.1", "api": "127.0.0.1"}
  },
  "token": {"configured": false, "hint": null},
  "storage": {
    "data_dir": "<data dir>",
    "media_dir": "<data dir>/media",
    "logs_dir": "<data dir>/logs",
    "media_items": 12,
    "media_bytes": 104857600,
    "free_bytes": 68719476736,
    "total_bytes": 274877906944
  },
  "retention": {"max_age_days": 30, "max_size_mb": 10240},
  "ffmpeg": {
    "available": true,
    "ffmpeg_version": "7.1",
    "ffprobe_version": "7.1",
    "path": "<data dir>/tools/ffmpeg",
    "license": "LGPL-2.1-or-later",
    "filters": 418
  },
  "about": {
    "version": "0.1.0",
    "commit": "<short sha>",
    "build": "2026-11-17T18:00:00Z",
    "os": "windows",
    "arch": "x86_64",
    "protocol": 1
  }
}
```

`listen.mode` is `localhost`, `lan` or `mixed`; the UI also derives it from the addresses
when omitted. `token.hint` is a non-secret suffix such as `…c0de`; the token value is
never returned. `free_bytes`/`total_bytes` and the ffmpeg strings are optional, as are
`commit`, `build` and `protocol`.

| Method | Path | Behaviour |
|---|---|---|
| `GET` | `/api/settings` | Reads the effective configuration, storage usage, retention, ffmpeg and about data. |
| `PATCH` | `/api/settings` | Accepts `{"retention": {"max_age_days": 14, "max_size_mb": 2048}}` and returns the updated settings or `{"retention": {...}}`. Negative values are rejected with 422 `INVALID_SETTING`; persistence is host-defined (config file or runtime override). |
| `GET` | `/api/logs?level=warn&limit=200` | Returns `{"lines": [{"ts_ms": 1700000000000, "level": "warn", "target": "stream", "message": "..."}]}` (or a bare array). Levels: trace/debug/info/warn/error; `level=all` and a missing level return everything; `limit` defaults to 200. |
| `GET` | `/api/diagnostics` | Streams a zip bundle (`application/zip`, attachment `watchdog-diagnostics-<timestamp>.zip`) with the effective config (token redacted), recent logs, versions and storage summary. The UI passes the bearer token as `?token=` because a download link cannot send headers. |

All JSON calls send `Authorization: Bearer <token>` when a token is configured. Non-2xx
responses use the error model above, and the UI shows `error.message` (plus `error.hint`)
in its status line.

## WebSocket events

Event envelope: `{ "event": "device.connected" | "device.disconnected" | "state.changed" |
"capture.started" | "capture.finished" | "recording.started" | "recording.stopped" |
"analysis.finished" | "provision.step", "timestamp_ms": <int>, "device_id"?: <id>,
"data": { ... } }`. On connect, the server replays the last full state snapshot per device
("replay-last-state") so a late subscriber converges. Event names are stable; additive.

## Conventions

- Pagination: `?limit=&cursor=`; responses carry `next_cursor`.
- Idempotency: capture/start endpoints accept `Idempotency-Key` header; replays return the
  original response with `Idempotent-Replayed: true`.
- Limits: request bodies ≤ 1 MiB (uploads stream); media responses are streamed; range
  requests supported for video preview.
- CORS: off by default; `api_cors_origins` (list) to opt in.

## Guarantees impresario can rely on

See [impresario-integration.md](impresario-integration.md) — including `POST /api/v1/analyses`
with `trigger_ms` and `correlation_id`, and the verdict polling/WebSocket flow.
