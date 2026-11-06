# ADR-0003: Test fixtures — committed small media, ffmpeg only in CI

- Status: Accepted
- Date: 2026-11-06

## Context

M1-T10 requires deterministic test media (clean video, black/frozen/spinner segments,
silence, solid-colour/gradient images). TASK.md fixes that fixture generation uses ffmpeg
**in CI only**, that small fixtures are committed, and that fixtures must be reproducible
or have a documented tolerance.

## Decision

- All fixtures live in `tests/fixtures/` and are **committed** (all ≤ ~210 KB, 320×240 or
  8 kHz mono, 4 s each). No fixture requires ffmpeg to be present to run the test suite.
- `scripts/generate-fixtures.py` (Python + ffmpeg + Pillow) regenerates them; images use
  lavfi/`geq` gradient computation and Pillow for the spinner (fixed phase sequence).
- `--verify` checks *structure and content* (codec/resolution/duration and detector finds),
  never SHA-256: H.264 payloads differ across encoder builds.
- `tests/fixtures/SHA256SUMS` records hashes from ffmpeg 7.x for convenience; it is
  informational, not a gate.
- The `fixtures` job in `ci.yml` installs ffmpeg/Pillow and runs `--verify` on every push.
- Larger/soak fixtures, when needed later (M7), are generated on the fly in CI, not committed.

## Consequences

- Tests are hermetic; CI still proves the generator works and fixtures are intact.
- Bit-for-bit reproducibility is explicitly *not* guaranteed across ffmpeg versions; the
  tolerance policy is documented in `tests/fixtures/README.md`.
- Fixture regeneration is a manual, documented step with PR review of the diff.
