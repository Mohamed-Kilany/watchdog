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
compare; tokens are never logged; 401 with `error.code = UNAUTHORIZED`.

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
