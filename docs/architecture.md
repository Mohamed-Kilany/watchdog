# Architecture

> Status: M1 skeleton. Sections marked *(target)* describe the committed design from
> `TASK.md`; update them as milestones land. "AS-IS" = implemented today.

## System context

```
        ┌─────────────────────────── host machine ───────────────────────────┐
        │                                                                    │
human ─▶│ Web UI (7700) ─▶ UI API/WS                                         │
AI    ─▶│ MCP server    (7701)  ─▶ tool dispatch ─▶ device/media/analysis    │
test  ─▶│ Integration API (7702, /api/v1, REST + WS) ─▶ OpenAPI from code     │
run   │        │                                                            │
        │        ▼                                                          │
        │  device manager ── native ADB (USB via rusb-class crate, TCP/TLS)  │──▶ Android device
        │        │                                                          │    (watchdog app,
        │        ▼                                                          │     Camera2)
        │  media store (SQLite index + files under data dir)                 │
        │        ▼                                                          │
        │  analyzer engine ── ffmpeg (bundled, extracted with checksum) /    │
        │  native Rust image analyzers / OTT rollup rules                    │
        │        ▼                                                          │
        │  scheduler ── cron engine + job store (SQLite)                     │
        └────────────────────────────────────────────────────────────────────┘
```

## Data flow (live view)

1. App (Camera2) → `ImageReader` (YUV) → JPEG encoder → bounded per-client sender queue
   (drop-oldest) → `FRAME` messages over the adb-forwarded abstract socket.
2. Host frame hub verifies CRC (transport), orders by `seq` (drops/reorders), computes
   FPS/bitrate/drop-rate, publishes to subscribers via bounded broadcast.
3. UI consumes MJPEG-style frames; MCP `get_live_frame` returns a single downscaled JPEG;
   recorder stores nothing unless explicitly capturing.

## Data flow (capture → analysis)

1. Host sends `COMMAND capture_still|record_start`; app writes to app-specific storage and
   emits `MEDIA_MANIFEST` (id, bytes, SHA-256, control snapshot).
2. Host pulls via adb **sync** (or socket stream for small stills), verifies hash, indexes
   in SQLite (`media` table), optionally deletes from device.
3. Analyzer runs (ffmpeg subprocess or native Rust); evidence thumbnails extracted and
   stored; report JSON (versioned schema) + HTML; verdict per user-defined rules.

## Crate map (workspace)

| Crate | Role | Status |
|---|---|---|
| `watchdog-core` | config, data-dir layout, logging + redaction, error taxonomy | AS-IS (M1) |
| `watchdog-protocol` | wire protocol codec + golden vectors | AS-IS (M1) |
| `watchdog-adb` | native ADB (USB/TCP/TLS pairing/shell/sync/forward/install) | M2 |
| `watchdog-device` | device manager, provisioning, frame hub | M3 |
| `watchdog-media` | media store, SQLite, transfers | M5 |
| `watchdog-analyzers` | analyzer trait + implementations | M7 |
| `watchdog-scheduler` | cron engine, job store | M8 |
| `watchdog-api` | integration REST/WS, OpenAPI | M9 |
| `watchdog-mcp` | MCP server | M9 |
| `watchdog-ui-server` | embedded UI serving | M6 |
| `watchdog` (host) | CLI wiring, servers | AS-IS (M1 partial) |

## Assumptions and constraints

- **No system dependencies at runtime**: no adb server (own port-independent implementation;
  if 5037 is occupied by another adb server, watchdog is unaffected), no ffmpeg on PATH
  (bundled per-OS/arch, LGPL build, extracted with SHA-256 verification), no node/python.
- **Unbounded memory growth is forbidden**: frame queues are bounded; slow consumers drop;
  analysis streams video through ffmpeg rather than loading files.
- **Loopback by default**; LAN requires explicit opt-in and strongly recommends a bearer
  token, honored identically by UI, MCP and API.
- **Contracts are versioned**: CLI, `/api/v1`, MCP tool schemas, protocol v1, report schema.

## Key decisions (ADRs)

See [adr/](adr/): workspace layout + core crate, wire protocol framing/CRC, committed
fixtures, clippy gate, data-directory layout, layered config.
