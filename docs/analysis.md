# Analysis

> Status: skeleton (M1). Every analyzer lands during M7 with its own parameter
> documentation and golden-file tests. This page defines the framework contracts and the
> OTT rollup model; parameters are filled in as analyzers ship.

## Framework contract

```rust
trait Analyzer {
    fn id(&self) -> &'static str;             // stable, kebab-case
    fn description(&self) -> &str;             // human + LLM-consumable
    fn inputs(&self) -> AnalyzerInputs;        // video | image | audio | sequence
    fn params_schema(&self) -> JsonSchema;     // validated before run
    fn run(&self, input: AnalyzerInput, params: Params, ctx: &RunContext) -> Report;
}
```

- Reports are **versioned JSON** (`"schema": "watchdog.report.v1"`).
- Findings carry: `start_ms`/`end_ms` or `frame_index`, `severity` (info|warn|critical),
  `rule` reference, `message`, optional `evidence` (thumbnail path/media id).
- Cancellation, progress events and partial results are first-class (long 1080p files).
- Evidence frames are stored as JPEG thumbs under the report id in the media store.

## Bundled ffmpeg analyzers (M7)

| Analyzer id | ffmpeg basis | Parameters (planned) |
|---|---|---|
| `blackdetect` | `blackdetect` + all-one-colour (luma histogram) | `pix_th`, `pic_th`, `min_duration_ms`, `all_colors` (black/white/green/blue) |
| `freezedetect` | `freezedetect` + native frame-hash cross-check | `noise_tolerance_db`, `min_duration_ms`, `region_mask` |
| `audio_silence` | `silencedetect` | `noise_db`, `min_duration_ms` |
| `loudness` | `ebur128` | `target_lufs`, `true_peak_dbtp` |
| `scene_changes` | `scdet` / `select=gt(scene,…)` | `threshold` |
| `signalstats` | `signalstats` | region, per-channel luma/chroma/sat min/max/avg |
| `duplicate_frames` | `mpdecimate`-style counting | `dup_threshold` |
| `effective_fps` | pts deltas + variance | — |
| `stream_info` | ffprobe (codec, res, fps, duration, audio, rotation) | — |
| `psnr` / `ssim` / `vmaf` | `psnr`, `ssim`, `vmaf` (only if the bundled build supports it) | reference path, alignment |

## Native image analyzers (Rust, no ffmpeg)

`image_diff` (pixel + SSIM, region masks, tolerance), `black_white_solid` frame detection,
`stuck_sequence` detection across a burst, `roi_compare`, `template_match` (spinner/logo/
error screens, multi-scale, confidence), `sharpness` (variance of Laplacian),
`brightness_histogram` (clipping %, exposure).

## OTT rollup report

One synthesized verdict from the analyzers above, with user-defined rules:

| Rule (example) | Default | Meaning |
|---|---|---|
| `max_buffering_events` | `0` | spinner-correlated freeze+audio-gap count |
| `max_buffering_total_ms` | `0` | summed buffering duration |
| `max_black_ms` | `500` | longest black screen tolerated |
| `max_black_count` | `0` | number of black screens tolerated |
| `max_freeze_ms` | `200` | longest stuck frame tolerated |
| `max_startup_ms` | `5000` | trigger → first stable non-black frame (needs trigger ts) |
| `min_effective_fps` | `23.0` | effective fps must stay above |
| `max_dropped_frame_pct` | `1.0` | duplicate/dropped ratio |

Report includes: per-rule pass/fail, overall `verdict: pass|fail`, plain-language summary,
timeline of events, thumbnails of offending frames. Sample JSON and HTML renderings land in
this file during M7.

### How buffering is derived

A **buffering event** = spinner template match ∪ (freeze period overlapping audio gap /
loudness dip) correlated within a sliding window; events are merged with gap ≤ 1 s. Count,
total duration, longest duration, and first start are reported. False positives (spinner on
static menus) are mitigated by requiring motion entropy drop or audio/silence correlation;
tunable via `buffering.min_confidence` and documented when tuning matters.

### Start-up time

From a caller-supplied trigger timestamp (impresario passes it in `POST /analyses` as
`trigger_ms`) to the first frame that is (a) not near-black, (b) no active spinner match,
(c) stable for `startup.stable_frames` consecutive frames. The report measures and
propagates this even if the operator didn't mean it as a "startup" test.

## Fixture reference

See [../tests/fixtures/README.md](../tests/fixtures/README.md) for the committed media
used by these analyzers.
