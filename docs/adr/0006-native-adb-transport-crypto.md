# ADR-0006: Native ADB — transport, crypto and protocol strategy

- Status: Accepted
- Date: 2026-11-06

## Context

TASK.md M2 requires a fully native Rust ADB: USB + TCP transports, auth, wireless-debugging
pairing (SPAKE2 + TLS), shell v2, sync, port forwarding, package install — **no system
adb, ever**. AOSP module `packages/modules/adb` was studied to extract the exact wire
formats (OVERVIEW.TXT, protocol.txt, adb.h, types.h, client/auth.cpp,
crypto/android_pubkey.cpp, pairing_connection.cpp, pairing_auth.cpp, aes_128_gcm.cpp,
tls/tls_connection.cpp).

## Decision

1. **USB access**: `nusb` (pure-Rust USB I/O; no libusb dependency, so no system USB
   library is required on any OS). Linux needs udev rules (shipped via
   `watchdog device udev-rules`); Windows uses WinUSB (device-specific driver binding via
   Zadig, documented); macOS needs no extra setup.
2. **Wire protocol**: exact AOSP `amessage` framing (24-byte header; `A_*` command ids;
   `magic = command ^ 0xffffffff`; version negotiation CNXN `0x01000001`; checksum 0 for
   version ≥ `0x01000001`). Payload limits: 4 KiB before CNXN, 1 MiB after.
3. **AUTH**: RSA-2048 key pair generated on first run, stored in
   `<data_dir>/keys/adbkey` (never `~/.android`). Sign with **SHA-1 + PKCS#1 v1.5** (as
   AOSP does, `NID_sha1`). Public key exported in Android's custom binary format
   (`modulus_size_words=64`, `n0inv = (-1/N[0]) mod 2³²`, LE modulus, `rr = 2^4096 mod N`,
   LE), base64-encoded + ` " user@host"` suffix — exactly what `adb_auth_get_userkey()`
   produces and what devices expect in `AUTH_RSAPUBLICKEY` / PeerInfo.
4. **Wireless pairing**: pure-Rust BoringSSL-compatible SPAKE2 over Ed25519 (the
   BoringSSL variant: M/N constants `5ada7e4b…`/`10e3df0a…`, SHA-512 transcript with
   8-byte LE length prefixes, password = 6-digit code bytes + 64 TLS exporter bytes
   (label `"adb-label\0"`), output 64 bytes). Key schedule reuses BoringSSL's exactly
   (no password-scalar hack needed: adding multiples of the group order is a no-op mod
   the order; `curve25519-dalek` reduces mod the order). Pairing packets:
   `u8 version=1 | u8 type (0=SPAKE2_MSG, 1=PEER_INFO) | u32 LE payload_len`; PeerInfo is
   8192 bytes (`ADB_RSA_PUB_KEY=0` host→device; `ADB_DEVICE_GUID=1` device→host),
   encrypted with AES-128-GCM (HKDF-SHA256 salt=zero(ikm → key info
   `"adb pairing_auth aes-128-gcm key"`), nonce = 8-byte LE counter + 4 zero bytes,
   tag appended, no AAD). TLS is 1.3-only with mutual certs and a verify-anything
   callback (matching AOSP); cert is a self-signed X509 (C=US, O=Android, CN=Adb,
   CA:TRUE, keyUsage keyCertSign+cRLSign+digitalSignature, 10-year) generated from the
   ADB RSA key. Rustls provides the TLS stack; a custom `ServerCertVerifier` accepts any
   chain; the RFC 8446 exporter yields the 64 bytes appended to the pairing code.
5. **STLS connect**: after pairing, `watchdog device connect` performs the normal ADB
   STLS upgrade (`A_STLS` with `A_STLS_VERSION=1`) on the wireless connect port; the
   normal CNXN/AUTH flow then runs over TLS 1.3 (again verify-anything on our side).
6. **Mock**: `adb-mock` test crate implements TCP transport with canned CNXN/AUTH/OPEN/
   shell/sync/install behaviors for CI; the real device path is verified manually per
   `docs/verification/` (never by CI).
7. **TCP**: tokio, with exponential-backoff reconnect; LAN mDNS via `mdns-sd` crate for
   `_adb-tls-connect._tcp` / `_adb-tls-pairing._tcp`.

## Consequences

- No external adb binary, no Android SDK, no libusb, no system OpenSSL — fully
  self-contained.
- `curve25519-dalek` + `rustls` are relied on for crypto; their security is the project's
  dependency posture (reviewed via cargo-deny).
- If BoringSSL ever changes its SPAKE2 derivation, pairing breaks; this is pinned by the
  golden interop test vectors we add from AOSP (see `crates/adb/tests/spake2_vectors`).
- Verification on real hardware is still pending and tracked in docs/PROGRESS.md; until
  then pairing is EXPERIMENTAL and documented as such.
