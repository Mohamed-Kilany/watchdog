# Changelog

All notable changes to this project are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace (M1): `watchdog-core` (config, paths, logging, error model),
  `watchdog-protocol`, `watchdog-adb`/`watchdog-device`/`watchdog-media`/`watchdog-analyzers`/
  `watchdog-scheduler`/`watchdog-api`/`watchdog-mcp`/`watchdog-ui-server` skeletons,
  `watchdog` host binary with clap CLI (`serve`, `device`, `capture`, `analyze`, `schedule`,
  `media`, `mcp`, `config`, `doctor`, `version`).
- Wire protocol v1 spec (`docs/protocol.md`) with codec and golden vectors shared with the
  future Kotlin app (`protocol/vectors/`).
- Layered configuration: defaults < `watchdog.toml` < `WATCHDOG_*` env < CLI flags, with
  per-key source tracking and LAN-exposure detection (`watchdog config show|path|validate`).
- Per-OS data directory layout with write probes (`watchdog doctor`); ADB key pair stored
  under the data dir, never `~/.android`.
- Logging to stderr + rotating daily file (JSON option) with streaming redaction of
  registered secrets (bearer tokens, pairing codes) — unit-tested, including secrets split
  across writes.
- Stable error taxonomy shared by CLI/API/MCP (`ErrorCode`), documented in `docs/api.md`.
- CI on Windows/macOS/Linux: fmt, clippy `-D warnings`, cargo test, cargo-deny, docs link
  check, fixture verification.
- Committed deterministic test fixtures (clean/black/frozen/spinner, silence, solid images)
  with a verify-only CI job.
- Documentation skeleton for all planned docs (see `docs/PROGRESS.md` for task status).

### Known limitations

- No device features yet (native ADB lands in M2); `watchdog device` commands report the
  milestone that implements them.
- No ffmpeg bundle yet (M7); `watchdog doctor` reports ffmpeg as not embedded.
- No Android app yet (M3).
