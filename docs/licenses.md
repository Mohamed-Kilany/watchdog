# Licenses

watchdog host and app source: **MIT OR Apache-2.0**.

Third-party components are redistributed under their own licenses. This file is updated
in the same PR that adds or changes a dependency; `cargo deny check` enforces the allow
list in `deny.toml` in CI.

## Host (Rust crates)

All crates are permissive (MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode-3.0) unless
noted; transitive closures are checked by cargo-deny including license *exceptions*
(e.g. `Apache-2.0 WITH LLVM-exception`).

| Component | License | Notes |
|---|---|---|
| tokio, serde, clap, thiserror, tracing, bytes, crc32fast, directories, toml, rust-embed (M6) | MIT/Apache-2.0 | — |
| rustls (M2: pairing/STLS) | Apache-2.0/ISC/MIT | per its NOTICE |
| rusb / nusb (M2: USB) | MIT/Apache-2.0 | to be named when chosen (M2-ADR) |

## ffmpeg (bundled, M7)

- The host **embeds** per-OS/arch ffmpeg+ffprobe binaries; nothing downloads at user runtime.
- Builds are LGPL-configured (`--enable-version3 --disable-gpl` or distro LGPL builds when
  their configuration is compatible); **no** GPL-only components (libx264/libx265 are
  excluded or dynamically linked only where legally compatible — final linkage documented
  here when the M7-T02 recipe lands).
- Rebuild recipe + exact configure flags live in `docs/ffmpeg-build.md` (M7).
- Corresponding source offer: the recipe references the exact upstream tarballs and the
  LGPL texts are shipped alongside (`licenses/ffmpeg/` in the release archives).
- Trademark: ffmpeg is a trademark of Fabrice Bellard; we distribute unmodified builds or
  builds with documented modifications.

## Android app

- Kotlin stdlib (Apache-2.0), AndroidX (Apache-2.0), Camera2 (platform),
  kotlinx.coroutines (Apache-2.0). R8/ProGuard keep rules are in `android/app/proguard-rules.pro`.
- The release APK is built by CI and signed with the CI-managed key.

## Test fixtures

Generated media under `tests/fixtures/` is synthetic (ffmpeg lavfi / Pillow) and carries
no third-party rights; it is MIT/Apache-2.0 like the rest of the repo.

## Fonts and UI assets

The web UI uses system font stacks and inline SVG only — **no bundled fonts, no CDN**.

## Notices

All redistributed notices are included in each release archive under `licenses/` and in
the binary's `--version`-adjacent `licenses` command output (M10-T07).
