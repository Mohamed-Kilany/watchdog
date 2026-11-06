# ADR-0005: Data directory layout — single root, platform config dir separate

- Status: Accepted
- Date: 2026-11-06

## Context

M1-T03 requires an OS-correct layout for config, keys, media, db, logs, cache and extracted
tools. Users must be able to inspect, back up or delete all watchdog state as one unit; the
ADB key pair must never touch `~/.android` (TASK.md H4).

## Decision

One **data root** per OS (`directories::BaseDirs::data_local_dir()/watchdog`), overridable
with `data_dir` / `WATCHDOG_DATA_DIR` / `--data-dir`:

```
<data_dir>/config keys media db logs cache tools tmp
```

- `keys/` holds `adbkey`/`adbkey.pub` (RSA-2048, created 0700 dir / 0600 files on Unix).
- `tools/` holds extracted bundled tools (ffmpeg) verified by SHA-256 before first use.
- The **platform config file** (`watchdog.toml`) lives in the OS config dir
  (`directories::ProjectDirs::config_dir()/watchdog.toml`), not under the data root, so it
  survives `data_dir` moves and is findable with conventional tooling
  (`watchdog config path`).
- `watchdog doctor` resolves the layout, creates it, and write-probes every directory.
- `tmp/` is used for in-progress transfers and extracted tool staging (atomic rename).

## Consequences

- Uninstallation is `rm -rf` of one directory (plus the platform config file).
- Windows: `%LOCALAPPDATA%\watchdog`, macOS: `~/Library/Application Support/watchdog`,
  Linux: `${XDG_DATA_HOME:-~/.local/share}/watchdog`.
- Portable mode is explicit: `--data-dir .` co-locates everything (documented, not a
  default).
- All tools extraction is checksum-verified; nothing is executed from `cache/`.
