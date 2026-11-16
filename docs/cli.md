# CLI reference

Curated from the clap definitions (`crates/host/src/main.rs`); kept current in the same PR
that changes the CLI. For the raw `--help` output of every command, run
`python scripts/generate-cli-docs.py` (requires a built binary) which writes
`docs/cli-raw.md`. Conventions: exit code `0` on success, `1` on runtime error, `2` on CLI
misuse (clap). Every command accepts the global flags below.

## Global flags

| Flag | Env | Description |
|---|---|---|
| `--config <PATH>` | `WATCHDOG_CONFIG` | use this `watchdog.toml` (must exist) |
| `--data-dir <DIR>` | `WATCHDOG_DATA_DIR` | override data directory |
| `--token <TOKEN>` | `WATCHDOG_TOKEN` | bearer token for UI/MCP/API |
| `--log-level <LEVEL>` | `WATCHDOG_LOG_LEVEL` | trace/debug/info/warn/error |
| `--ui-port <PORT>` | `WATCHDOG_UI_PORT` | default 7700 |
| `--mcp-port <PORT>` | `WATCHDOG_MCP_PORT` | default 7701 |
| `--api-port <PORT>` | `WATCHDOG_API_PORT` | default 7702 |
| `--ui-listen <ADDR>` / `--mcp-listen` / `--api-listen <ADDR>` | `WATCHDOG_*_LISTEN` | IP literal; `0.0.0.0` exposes LAN |
| `-h`, `--help` | — | help for every command |
| `-V`, `--version` | — | print version |

## Commands

### `watchdog version`
Print host version, protocol version and embedded-APK version (APK is embedded in
release builds; dev builds print `not embedded`).

### `watchdog doctor`
Print OS/arch, host/protocol versions, resolved config path, per-directory
create+write probes (config, keys, presets, media, db, logs, cache, tools, tmp), port availability
probe (ui/mcp/api), ffmpeg status and the LAN-exposure/token warnings. Exits non-zero if
any required path is unusable.

### `watchdog config <subcommand>`
- `show` — effective config with the source of each key; token redacted; LAN warning.
- `path` — resolved `watchdog.toml` path + existence.
- `validate` — parse/validate all layers; write-probe data dir; prints ok + ports.

### `watchdog serve [--serial S]`
Start the web UI on `--ui-port` (default 7700). M3 serves the minimal test page:
`/` is a live MJPEG view of the provisioned phone camera with FPS, bitrate, latency,
RTT, sequence, drop and duplicate counters polled from `/api/stats`; `/stream.mjpg`
is the multipart MJPEG stream and `/healthz` is a liveness probe. The page reuses the
session token stored by `watchdog device provision` (run it first). `--serial` selects
a USB serial or `host[:port]` (wireless debugging over LAN). When a bearer token is
configured, every request needs `?token=…` or `Authorization: Bearer …`. MCP (:7701)
and the integration API (:7702) land with M6/M9.

### `watchdog device <subcommand>`
| Command | Purpose | Status |
|---|---|---|
| `list [--discover] [--json]` | USB devices (+ mDNS) | M2 |
| `discover` | mDNS scan for wireless debugging endpoints | M2 |
| `connect <target>` | USB serial or `host[:port]` | M2 |
| `pair <host:port> <code>` | wireless-debugging pairing | M2 |
| `disconnect [target]` | disconnect one/all | M2 |
| `info [target]` | model, SDK, ABI, state | M2 |
| `shell [--serial S] [-- cmd…]` | shell v2 (exit code, stderr) | M2 |
| `push <local> <remote> [--serial]` | adb sync push | M2 |
| `pull <remote> <local> [--serial]` | adb sync pull | M2 |
| `install [--serial] [--apk PATH]` | install/update embedded app | M2/M3 |
| `uninstall [package] [--serial]` | uninstall (default app.watchdog.remote) | M2 |
| `forward <local_port> <remote> [--serial]` | host port → device socket | M2 |
| `logs [--serial] [--follow]` | app log ring buffer | M3 |
| `provision [--serial] [--apk PATH] [--no-verify] [--no-screen] [--socket-timeout S] [--frame-timeout S] [--json]` | zero-touch provisioning | M3 |
| `status [--serial] [--json]` | installed version, permissions, battery/screen state and session token age | M3 |
| `restore [--serial]` | revert screen settings recorded by provisioning | M3 |
| `capabilities [--serial] [--json]` | dump capability descriptor (handshake, no camera open) | M4 |
| `controls [--serial] [--json]` | print the live control state (`get_state` round trip) | M4 |
| `set-control <id=value>… [--serial] [--json]` | apply a control batch atomically; prints per-control results, actual values and round-trip ms | M4 |
| `probe [--serial] [--camera ID] [--key KEY] [--json] [--output FILE]` | probe every advertised request key and print/write the per-device compatibility matrix | M4-T10 |
| `preset list [--json]` | list host-stored control presets | M4-T11 |
| `preset show <name> [--json]` | show one preset (raw JSON with `--json`) | M4-T11 |
| `preset save <name> [--serial] [--controls IDS] [--include-structural] [--description TEXT]` | capture the device's current state as a preset | M4-T11 |
| `preset apply <name> [--serial] [--dry-run] [--json]` | apply a preset as one `set_controls` batch | M4-T11 |
| `preset export <name> <FILE>` | write a preset to a shareable JSON file | M4-T11 |
| `preset import <FILE> [--name NAME]` | import a preset JSON file | M4-T11 |
| `preset rm <name>` | delete a saved preset | M4-T11 |
| `watch-state [--serial] [--count N] [--json]` | stream STATE full/delta messages (one JSON object per line with `--json`) | M4 |
| `udev-rules [--write]` | print/install Linux udev rules | **done (this release)** |
| `usb-doctor` | diagnose USB permission problems | M2 |

### `watchdog capture <subcommand>`
`still [--serial] [--output PATH]` · `burst [--serial] [--count N] [--interval-ms MS] [--from-stream]` · `record start [--serial] [--duration-secs N]` · `record stop [--serial]` *(M5)*

### `watchdog analyze <subcommand>`
`run <media-id> [--analyzers IDS] [--profile NAME] [--json]` · `report <id> [--format json|html]` · `list-analyzers [--json]` *(M7)*

### `watchdog schedule <subcommand>`
`add --name N (--cron CRON | --preset P) --device SERIAL [--record-secs N] [--profile P]` ·
`list [--json]` · `run <id>` · `enable <id>` · `disable <id>` · `rm <id>` *(M8)*

### `watchdog media <subcommand>`
`list [--device SERIAL] [--json]` · `get <id> [--output PATH]` · `export <ids…> --output ZIP` · `rm <ids…>` *(M5)*

### `watchdog mcp [--stdio]`
Serve MCP over stdio (for spawned clients). Without `--stdio`, same as the MCP port. *(M9)*

## Examples

```bash
watchdog doctor
watchdog config show
sudo watchdog device udev-rules --write
watchdog device provision --serial 1a2b3c4d
watchdog serve --serial 1a2b3c4d          # live test page at http://127.0.0.1:7700/
watchdog device status --serial 1a2b3c4d
watchdog device restore --serial 1a2b3c4d
watchdog device controls --serial 1a2b3c4d
watchdog device set-control iso=400 exposure_time_ns=33333333 --serial 1a2b3c4d
watchdog device probe --serial 1a2b3c4d --output docs/device-matrices/sm-a515f-android-13.md
watchdog device probe --serial 1a2b3c4d --key android.control.aeMode --json
watchdog device watch-state --count 20 --json --serial 1a2b3c4d
watchdog capture still --serial 1a2b3c4d --output frame.jpg
watchdog schedule add --name "every 30 min" --preset every-30m --device 1a2b3c4d --record-secs 10
```
