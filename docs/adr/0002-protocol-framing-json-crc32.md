# ADR-0002: Wire protocol — 16-byte header framing, JSON payloads, CRC-32 IEEE

- Status: Accepted
- Date: 2026-11-06

## Context

TASK.md §4 requires a single wire protocol (`docs/protocol.md`) used by the Rust host and
the Kotlin app, with golden vectors under `protocol/vectors/`. Decisions needed: framing,
checksum, payload encoding, and how JSON schemas evolve.

## Decision

1. **Framing**: fixed 16-byte header (`WDG1` magic, `protocol_version`, `type`, `flags`,
   reserved, `payload_len` u32 LE, CRC-32 of payload). Max payload 64 MiB. Decoders must
   never panic on garbage; any structural violation closes the stream.
2. **Checksum**: CRC-32/IEEE (as in Ethernet/PNG/zip), `java.util.zip.CRC32` on Android and
   `crc32fast` on host are bit-compatible.
3. **Payloads**: JSON for control/state/manifest/hello/error/ping (schemaless enough to
   evolve additively), fixed binary layout for `FRAME` and `LOG_CHUNK`. Binary header for
   frame metadata avoids JSON overhead for the 15–30 fps live path.
4. **Evolution**: v1 is frozen once shipped; unknown JSON keys must be tolerated; new
   fields/messages ⇒ `protocol_version` bump. Capability descriptor (reflection over
   Camera2 request/result keys) is passed in `HELLO_ACK`, not hand-maintained.
5. **Golden vectors** are canonical: both sides run the same `.bin` files; CI verifies the
   Rust side and diffs regeneration.

## Consequences

- JSON payloads are human-greppable in captures — good for debugging.
- CRC catches USB/TCP corruption that TCP checksums miss (esp. over TLS tunnels and when
  frames are forwarded by middleware).
- Rejected alternatives: protobuf/flatbuffers (adds codegen and version-skew risk for a
  13-message protocol; JSON is sufficient at this scale), length-prefix without CRC
  (insufficient integrity signal for media).
- The `rotation` field is `u16` degrees (0/90/180/270) because a `u8` cannot express 270°.
