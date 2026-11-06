# Configuration reference

> Full user-facing guide: run `watchdog config show|path|validate`. This file is the
> authoritative key table for `watchdog.toml`.

## Precedence

`built-in defaults` < `watchdog.toml` < `WATCHDOG_*` environment variables < CLI flags.

Config file location resolution:

1. `--config <path>` or `WATCHDOG_CONFIG` (explicit; must exist — otherwise `CONFIG_NOT_FOUND`),
2. platform config dir + `watchdog.toml` (e.g. `~/.config/watchdog/watchdog.toml`),
3. built-in defaults.

## Keys

| Key | Env | CLI flag | Default | Notes |
|---|---|---|---|---|
| `ui_port` | `WATCHDOG_UI_PORT` | `--ui-port` | `7700` | web UI + UI API/WS |
| `mcp_port` | `WATCHDOG_MCP_PORT` | `--mcp-port` | `7701` | MCP Streamable HTTP |
| `api_port` | `WATCHDOG_API_PORT` | `--api-port` | `7702` | integration API |
| `ui_listen` | `WATCHDOG_UI_LISTEN` | `--ui-listen` | `127.0.0.1` | IP literal; `0.0.0.0` exposes LAN (token advised) |
| `mcp_listen` | `WATCHDOG_MCP_LISTEN` | `--mcp-listen` | `127.0.0.1` | same rules |
| `api_listen` | `WATCHDOG_API_LISTEN` | `--api-listen` | `127.0.0.1` | same rules |
| `token` | `WATCHDOG_TOKEN` | `--token` | unset | bearer token for all three ports; required for LAN in practice |
| `data_dir` | `WATCHDOG_DATA_DIR` | `--data-dir` | OS default (see ADR-0005) | moves media/db/logs/keys/tools |
| `retention_max_age_days` | `WATCHDOG_RETENTION_MAX_AGE_DAYS` | — | `30` | `0` keeps media forever |
| `retention_max_size_mb` | `WATCHDOG_RETENTION_MAX_SIZE_MB` | — | `10240` | `0` unlimited |
| `ffmpeg_path` | `WATCHDOG_FFMPEG_PATH` | — | unset | **debug builds only**; ignored (with warning) in release |
| `log_level` | `WATCHDOG_LOG_LEVEL` | `--log-level` | `info` | trace/debug/info/warn/error |
| `log_json` | `WATCHDOG_LOG_JSON` | — | `false` | JSON lines for console + file sinks |

Ports must be distinct. Listen addresses must be IP literals. `log_json` accepts true/false.
Unknown keys are rejected (`deny_unknown_fields`). `watchdog config show` prints the
source of every key with the token redacted.

## Example

```toml
# watchdog.toml
api_listen = "127.0.0.1"
ui_port = 7700
token = "choose-a-long-random-string"   # never commit; prefer WATCHDOG_TOKEN
retention_max_age_days = 14
log_level = "info"
```

## Security notes

- Token values are registered with the log redactor at startup; they never appear in logs.
- Enabling LAN (`0.0.0.0`) without a token triggers prominent warnings from
  `watchdog doctor` and the UI. See [security.md](security.md).
