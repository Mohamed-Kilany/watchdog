# Native ADB — implementation notes for `watchdog-adb`

Implements the Android Debug Bridge protocol in Rust with **no external adb**.
Normative references: AOSP `packages/modules/adb` (OVERVIEW.TXT, protocol.txt,
adb.h, types.h, client/auth.cpp, sync service docs). Citations are inline.

## Modules

| File | About |
|---|---|
| `packet.rs` | 24-byte `amessage` header, `A_*` commands, incremental decoder |
| `transport.rs` | `Transport` trait — all higher layers depend only on this |
| `connection.rs` | CNXN negotiation, AUTH handshake, stream multiplexer with per-stream OKAY flow control |
| `auth.rs` | RSA-2048 keypair (`<data_dir>/keys/adbkey`), Android binary public key, SHA-1/PKCS#1 v1.5 signing |
| `shell.rs` | `shell,v2,raw:` framing (u8 id + u32 LE length), exit codes, legacy shell fallback |
| `sync.rs` | `sync:` service — STAT/LIST/SEND/RECV, 64 KiB chunks, `.watchdog-part` pull semantics |
| `forward.rs` | In-process port forwarding (`adb forward` equivalent) |
| `install.rs` | `cmd package install` (fallback `pm install`) over shell v2 stdin; `INSTALL_FAILED_*` hints |
| `tcp.rs` | TCP transport (`adb tcpip`) |
| `stls.rs` | STLS upgrade (TLS 1.3, verify-any) for wireless debugging; `LanTransport` auto-upgrades after `A_STLS` |
| `pairing.rs` | BoringSSL-profile SPAKE2 over Ed25519 (M `5ada7e4b…`, N `10e3df0a…`), transcript, HKDF, AES-128-GCM PeerInfo (Android scheme) |
| `pairing_flow.rs` | Orchestration (TLS exporter → SPAKE2 → PeerInfo); AOSP-profile X509 via `generate_x509_der` |
| `usb.rs` | nusb bulk I/O, claim ADB interface (`ff/42/01`), enumerate, worker-thread multiplexing |
| `discovery.rs` | mDNS browse of `_adb-tls-connect._tcp` and `_adb-tls-pairing._tcp` |
| `manager.rs` | Device table, USB/TCP connect helpers, property refresh |

## Key store

`<data_dir>/keys/adbkey` (PKCS#8 PEM) + `adbkey.pub` (base64 + ` user@host`). Never
`~/.android`. See `docs/security.md` for backup/revoke guidance.

## Testing

- `tests/packet_codec.rs` — framing, garbage-input sweeps (no panics).
- `tests/session.rs` — handshake flows against the scripted mock transport.
- `tests/shell_codec.rs` — shell v2 codec roundtrips.
- `tests/no_system_adb.rs` — `no_system_adb_*` isolation tests (M2-T16).
- CI has an `isolation` job that binds a foreign server to 5037 and runs the
  `no_system_adb` tests without invoking system adb.

## On-device status

Handshake, shell, sync and install flows are implemented and exercised against
`adb-mock`; real-device proof lands in `docs/verification/` and is **pending** —
until then, device operations are experimental. See `docs/PROGRESS.md`.
