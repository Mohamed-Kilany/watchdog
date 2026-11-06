# Release process

> Status: skeleton (M1). Automated by `.github/workflows/release.yml` (added in M10-T07);
> this page documents the manual-checklist parts. All releases are built by CI — never by
> hand locally (TASK.md section 6).

## Build matrix

Binaries: Windows x64/arm64, macOS x64/arm64, Linux x64/arm64.
APK: `app.watchdog.remote` (release build, CI-managed signing key — secret
`ANDROID_KEYSTORE_*`). Host embeds the APK and the web UI (`rust-embed`).
ffmpeg: per-OS/arch, LGPL build produced by the documented recipe (see
[licenses.md](licenses.md)), checksummed and embedded.

Artifacts per release tag `vX.Y.Z`: 6 archives (binary + licenses + docs), APK,
checksums (`SHA256SUMS`), SBOM (SPDX), attestation notes.

## Cutting a release

1. Ensure `master` is green (all CI jobs, all platforms).
2. Update `CHANGELOG.md` and `docs/verification/<date>-release.md` (final checklist on the
   **tagged artifacts**, per section 7.4 of TASK.md).
3. Tag: `git tag -s vX.Y.Z -m "watchdog vX.Y.Z" && git push origin vX.Y.Z`.
4. `release.yml` builds; verify checksums; GitHub Release is drafted automatically; review
   and publish manually.
5. Download the published artifacts, run the milestone device checklist, fill the
   verification record, and attach it (or commit it) with the release link.

## Signature/checksum verification (users)

Every release ships `SHA256SUMS`; verify with `sha256sum -c` (Linux/macOS) or
`Get-FileHash` (Windows). The APK verifies with `apksigner`/`keytool`; the host binary is
not code-signed yet — Windows SmartScreen and macOS Gatekeeper notes are documented in the
release notes (see section "OS trust notes" of each release).

## Protocol / schema compatibility matrix

Release notes include the supported protocol version and the report schema version
(`watchdog.report.v1`), and the compatibility policy (see [protocol.md](protocol.md) §9).
Breaking API changes ⇒ `/api/v2` with migration notes here.
