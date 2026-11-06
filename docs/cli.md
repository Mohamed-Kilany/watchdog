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
create+write probes (config, keys, media, db, logs, cache, tools, tmp), port availability
probe (ui/mcp/api), ffmpeg status and the LAN-exposure/token warnings. Exits non-zero if
any required path is unusable.

### `watchdog config <subcommand>`
- `show` — effective config with the source of each key; token redacted; LAN warning.
- `path` — resolved `watchdog.toml` path + existence.
- `validate` — parse/validate all layers; write-probe data dir; prints ok + ports.

### `watchdog serve`
Start web UI (:7700), MCP (:7701) and integration API (:7702). *(lands with M3/M6/M9; currently reports the owning milestone.)*

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
| `provision` / `status` / `restore [--serial]` | zero-touch provisioning | M3 |
| `capabilities [--serial] [--json]` | dump capability descriptor | M4 |
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
watchdog capture still --serial 1a2b3c4d --output frame.jpg
watchdog schedule add --name "every 30 min" --preset every-30m --device 1a2b3c4d --record-secs 10
```
