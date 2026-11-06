# Development guide

## Prerequisites

- Rust **stable pinned** by `rust-toolchain.toml` (rustup auto-installs on first use).
- Python 3.11+ and ffmpeg 7.x — only for the **fixture generator** (CI provides both).
- No system adb, ffmpeg or Android SDK are required for any build or test.
- GitHub Actions builds everything; local builds are for iteration only.

## Build, test, lint

```bash
cargo build --workspace
cargo test --workspace           # includes protocol golden-vector tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check                 # licenses + advisories (after cargo install cargo-deny)
python scripts/check-docs-links.py
python scripts/generate-fixtures.py --verify
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on
Ubuntu/Windows/macOS plus cargo-deny, the docs link check and the fixture verification
job. All must be green before a milestone merges to `master`.

## Layout

See [architecture.md](architecture.md#crate-map-workspace). The wire protocol spec is
[protocol.md](protocol.md); golden vectors live in `protocol/vectors/` and **must be the
same on both sides** — regenerate with:

```bash
cargo run -p watchdog-protocol --example generate-vectors
```

then commit the vectors; CI fails if the committed vectors drift from the generator.

## Adding an analyzer (M7)

1. Implement `Analyzer` (`watchdog-analyzers`): `id`, `description`, `inputs`,
   `params_schema`, `run`.
2. Declare JSON Schema for parameters; the framework validates and rejects bad input.
3. Emit findings with timestamps/frame indexes + severity + evidence thumbnail reference.
4. Add golden-file tests under `tests/` with fixtures from `tests/fixtures/`.
5. Document parameters + false-positive notes in [analysis.md](analysis.md).
6. Register in the analyzer registry; expose in UI/MCP/API in the same PR.

## Adding a camera control (M4)

Every control task has the same Definition of Done: capability detection from
`CameraCharacteristics`, request-key mapping, result readback, validation/clamping,
protocol messages, host UI widget, on-device display, API/MCP exposure, unit test with a
fake camera, docs entry in [camera-controls.md](camera-controls.md), and a real-device
check that the **reported** value changes.

## Fixtures

See [../tests/fixtures/README.md](../tests/fixtures/README.md) — regenerate with
`python scripts/generate-fixtures.py` (ffmpeg + Pillow), verify with `--verify`.

## Documentation & docs link check

All Markdown links to repo files are validated in CI by `scripts/check-docs-links.py`
(no network access needed). Code blocks are stripped before scanning, so links inside
examples don't need to resolve.

## Verification records

Physical-device verification follows [verification/TEMPLATE.md](verification/TEMPLATE.md);
records live in `docs/verification/<date>-<milestone>.md` and are committed with the
milestone they prove. See section 7 of `TASK.md` for the mandatory checklists.
