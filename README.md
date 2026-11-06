# watchdog

Turn an Android phone into a **remotely controlled, observable camera** pointed at a screen
(TV, set-top box, another phone) so OTT/streaming playback can be watched, captured and
analyzed from outside the device under test.

`watchdog` has two parts:

- **Host** (Rust, single binary for Windows/macOS/Linux × x64/arm64): serves the web UI,
  an MCP server and an integration API. Talks to the device through its own native Rust
  implementation of the ADB protocol — no system `adb`, no Android SDK, no ffmpeg on PATH.
- **Remote** (Kotlin/Camera2 app, `app.watchdog.remote`): full manual/automatic camera
  controls, live viewfinder streamed to the host, local recording pushed to the host.

> **Status: early development — M1 (Foundations) in progress.**
> The feature surface described in [TASK.md](TASK.md) is the committed scope; see
> [docs/PROGRESS.md](docs/PROGRESS.md) for what exists today.

## How it works

```
┌────────────┐  zero-touch provision (install, grant, launch, forward)  ┌─────────────────┐
│  watchdog  │ ───────────────────────────────────────────────────────▶ │  watchdog app    │
│  (host)    │ ◀── JPEG frames (adb-forwarded socket) ─────────────────  │  (Android)       │
│            │ ◀── stills/video via adb sync, hash-verified ────────────  │  Camera2         │
└────────────┘                                                            └─────────────────┘
     │ 7700 web UI · 7701 MCP · 7702 integration API (all loopback-only by default)
     ▼
  analyzers (blackdetect, freezedetect, silence, OTT rollup, …) + scheduler + media library
```

## 5-minute quick start

> Works once M1–M3 land; tracked in [docs/PROGRESS.md](docs/PROGRESS.md).

1. Download the release binary for your OS from GitHub Releases.
2. Run `watchdog doctor` — verifies paths, ports, and notes OS-specific USB setup.
3. Connect a phone with USB debugging enabled (`watchdog device list`).
4. `watchdog device provision --serial <serial>` — installs the app, grants permissions,
   starts it, opens the forwarded socket. One tap on the phone (the OS debug-authorization
   prompt) is the only interaction.
5. Open <http://127.0.0.1:7700> — live view with full camera controls, capture, analysis.

For LAN / wireless debugging (Android 11+ pairing), see [docs/device-setup.md](docs/device-setup.md).

## Documentation

| Doc | Contents |
|---|---|
| [docs/user-guide.md](docs/user-guide.md) | UI walkthrough, scheduler, analysis, CLI reference |
| [docs/configuration.md](docs/configuration.md) | every `watchdog.toml` key, env var and flag |
| [docs/device-setup.md](docs/device-setup.md) | USB/wireless setup, pairing, per-OS notes, OEM quirks |
| [docs/camera-controls.md](docs/camera-controls.md) | every control, auto/manual semantics, device differences |
| [docs/analysis.md](docs/analysis.md) | analyzers, parameters, OTT report schema |
| [docs/api.md](docs/api.md) | integration API (`/api/v1`), OpenAPI, error model |
| [docs/mcp.md](docs/mcp.md) | MCP tools and client configuration |
| [docs/impresario-integration.md](docs/impresario-integration.md) | impresario ↔ watchdog contract and examples |
| [docs/protocol.md](docs/protocol.md) | host↔app wire protocol (normative) |
| [docs/architecture.md](docs/architecture.md) | architecture and data flow |
| [docs/development.md](docs/development.md) | build, test, debug, adding analyzers/controls |
| [docs/release.md](docs/release.md) | release process, artifact verification |
| [docs/security.md](docs/security.md) | threat model, token auth, LAN exposure |
| [docs/licenses.md](docs/licenses.md) | third-party licenses (ffmpeg, …) |
| [docs/device-setup.md](docs/device-setup.md) | setup + compatibility table |
| [docs/verification/](docs/verification/) | physical-device verification records |
| [CHANGELOG.md](CHANGELOG.md) | release notes |
| [TASK.md](TASK.md) | product specification (normative scope) |

## Building from source

Prerequisites: Rust stable (pinned by [rust-toolchain.toml](rust-toolchain.toml)).
No system adb/ffmpeg/Android SDK required. CI builds and tests Windows, macOS and Linux
(see [.github/workflows/ci.yml](.github/workflows/ci.yml)).

```bash
cargo build --workspace
cargo test --workspace
cargo run -p watchdog -- --help
```

## License

MIT OR Apache-2.0 (host and app source). Bundled third-party components keep their own
licenses — see [docs/licenses.md](docs/licenses.md).
