# Test fixtures

Deterministic media used by analyzer and pipeline tests (see TASK.md M1-T10, M7).

| Path | Kind | Purpose |
|---|---|---|
| `images/gradient-320x240.png` | image | gradient ramp; image analyzers, histogram tests |
| `images/solid-black-320x240.png` | image | black frame detection |
| `images/solid-white-320x240.png` | image | white/solid-colour detection |
| `images/solid-green-320x240.png` | image | colour-cast / solid-colour detection |
| `video/clean-4s.mp4` | video | healthy reference stream (testsrc2 + 440 Hz tone) |
| `video/black-segment-4s.mp4` | video | black segment from t=2 s; `blackdetect` fixture |
| `video/frozen-segment-4s.mp4` | video | frozen tail from t=2 s; `freezedetect` fixture |
| `video/spinner-segment-4s.mp4` | video | rotating spinner; buffering-detector fixture |
| `audio/tone-then-silence-4s.wav` | audio | 1 s tone + 3 s silence; `silencedetect` fixture |

All fixtures are 320×240 (images/video) or 8 kHz mono (audio), 4 s long, and
each is below ~210 KB.

## Regenerating

```bash
python scripts/generate-fixtures.py          # rewrite all fixtures
python scripts/generate-fixtures.py --verify # check committed fixtures
```

Requires ffmpeg and Pillow; CI installs both. Changing fixtures rewrites
`tests/fixtures/SHA256SUMS`.

## Determinism policy

Sources (`lavfi` filters, Pillow drawing with fixed phase) are deterministic,
but H.264 payload bytes differ across encoder builds. Therefore:

- `--verify` checks **structure and content** (codec, resolution, duration,
  and that black/freeze/silence detectors find the expected segments); it
  never compares bytes.
- `SHA256SUMS` lists hashes from the reference toolchain (ffmpeg 7.x) for
  convenience; a hash mismatch across environments is not a failure by itself —
  run `--verify` instead.
