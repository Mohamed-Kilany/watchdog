//! Packet codec tests — including garbage-input fuzzing (no panics).

use bytes::Bytes;
use watchdog_adb::error::AdbError;
use watchdog_adb::packet::{Command, IncrementalDecoder, Packet, HEADER_LEN, MAX_PAYLOAD};

fn sample_cnxn() -> Packet {
    Packet::new(
        Command::Cnxn,
        0x0100_0001,
        1024 * 1024,
        b"host::test demo\0".as_slice(),
    )
}

#[test]
fn encode_matches_known_layout() {
    let bytes = sample_cnxn().encode();
    assert_eq!(bytes.len(), HEADER_LEN + "host::test demo\0".len());
    let mut expected = Vec::new();
    expected.extend_from_slice(&0x4e58_4e43u32.to_le_bytes()); // A_CNXN
    expected.extend_from_slice(&0x0100_0001u32.to_le_bytes());
    expected.extend_from_slice(&(1024u32 * 1024).to_le_bytes());
    expected.extend_from_slice(&("host::test demo\0".len() as u32).to_le_bytes());
    expected.extend_from_slice(&0u32.to_le_bytes()); // checksum unused
    expected.extend_from_slice(&(0x4e58_4e43u32 ^ 0xffff_ffff).to_le_bytes());
    expected.extend_from_slice(b"host::test demo\0");
    assert_eq!(bytes.as_ref(), expected.as_slice());
}

#[test]
fn decode_roundtrip() {
    let mut decoder = IncrementalDecoder::new();
    let packet = sample_cnxn();
    let bytes = packet.encode();
    let decoded = decoder.feed(&bytes, 4 * 1024).expect("decode");
    assert_eq!(decoded, vec![packet]);
}

#[test]
fn split_feed_byte_by_byte() {
    let packet = Packet::new(Command::Wrte, 7, 3, Bytes::from_static(b"abc"));
    let bytes = packet.encode();
    let mut decoder = IncrementalDecoder::new();
    let mut packets = Vec::new();
    for byte in bytes {
        packets.extend(decoder.feed(&[byte], MAX_PAYLOAD).expect("decode"));
    }
    assert_eq!(packets, vec![packet]);
}

#[test]
fn concatenated_packets_are_split() {
    let mut stream = sample_cnxn().encode().to_vec();
    stream.extend_from_slice(&Packet::new(Command::Okay, 1, 2, &[][..]).encode());
    let mut decoder = IncrementalDecoder::new();
    let packets = decoder.feed(&stream, MAX_PAYLOAD).expect("decode");
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].command, Command::Cnxn);
    assert_eq!(packets[1].command, Command::Okay);
}

#[test]
fn magic_mismatch_is_rejected() {
    let mut bytes = sample_cnxn().encode().to_vec();
    bytes[20] ^= 0x40;
    let mut decoder = IncrementalDecoder::new();
    let err = decoder.feed(&bytes, MAX_PAYLOAD).unwrap_err();
    assert!(matches!(err, AdbError::BadMagic { .. }), "got {err:?}");
}

#[test]
fn unknown_command_is_rejected() {
    let mut bytes = sample_cnxn().encode().to_vec();
    bytes[0..4].copy_from_slice(&0xdead_beefu32.to_le_bytes());
    // Recompute magic so only the command is bogus.
    bytes[20..24].copy_from_slice(&(0xdead_beefu32 ^ 0xffff_ffff).to_le_bytes());
    let mut decoder = IncrementalDecoder::new();
    let err = decoder.feed(&bytes, MAX_PAYLOAD).unwrap_err();
    assert!(matches!(err, AdbError::Protocol(_)), "got {err:?}");
}

#[test]
fn oversized_payload_declaration_is_rejected() {
    let mut decoder = IncrementalDecoder::new();
    let mut bytes = sample_cnxn().encode().to_vec();
    bytes[12..16].copy_from_slice(&((64u32 * 1024 * 1024) + 1).to_le_bytes());
    let err = decoder.feed(&bytes, MAX_PAYLOAD).unwrap_err();
    assert!(matches!(err, AdbError::PayloadTooLarge(_)), "got {err:?}");
}

#[test]
fn pre_cnxn_limit_is_enforced() {
    // A packet legitimately under the global cap but above 4 KiB must fail
    // before CNXN is negotiated.
    let payload = vec![0u8; 5000];
    let packet = Packet::new(Command::Wrte, 1, 2, Bytes::from(payload));
    let mut decoder = IncrementalDecoder::new();
    let err = decoder.feed(&packet.encode(), 4 * 1024).unwrap_err();
    assert!(
        matches!(err, AdbError::PayloadTooLarge(5000)),
        "got {err:?}"
    );
}

#[test]
fn garbage_never_panics() {
    // xorshift64 — deterministic pseudo-random inputs.
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..20_000u32 {
        let len = (rand() % 600) as usize;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push(rand() as u8);
        }
        let mut decoder = IncrementalDecoder::new();
        let _ = decoder.feed(&buf, MAX_PAYLOAD);
        if case % 5 == 0 {
            // Feed a valid prefix then garbage — still no panic.
            let valid = sample_cnxn().encode();
            let _ = decoder.feed(&valid[..7], MAX_PAYLOAD);
            let _ = decoder.feed(&buf, MAX_PAYLOAD);
        }
    }
}

#[test]
fn truncated_stream_waits_for_more_bytes() {
    let bytes = sample_cnxn().encode();
    let mut decoder = IncrementalDecoder::new();
    // Only the header, no payload — must return an empty vec, not an error.
    let packets = decoder
        .feed(&bytes[..HEADER_LEN], MAX_PAYLOAD)
        .expect("header only");
    assert!(packets.is_empty());
    // Payload arrives in a later chunk.
    let packets = decoder
        .feed(&bytes[HEADER_LEN..], MAX_PAYLOAD)
        .expect("payload");
    assert_eq!(packets.len(), 1);
}

#[test]
fn command_values_match_aosp() {
    // From packages/modules/adb protocol.txt — do not change without an ADR.
    assert_eq!(Command::Sync.as_u32(), 0x434e_5953);
    assert_eq!(Command::Cnxn.as_u32(), 0x4e58_4e43);
    assert_eq!(Command::Auth.as_u32(), 0x4854_5541);
    assert_eq!(Command::Open.as_u32(), 0x4e45_504f);
    assert_eq!(Command::Okay.as_u32(), 0x5941_4b4f);
    assert_eq!(Command::Wrte.as_u32(), 0x4554_5257);
    assert_eq!(Command::Clse.as_u32(), 0x4553_4c43);
    assert_eq!(Command::Stls.as_u32(), 0x534c_5453);
    for cmd in [
        Command::Sync,
        Command::Cnxn,
        Command::Auth,
        Command::Open,
        Command::Okay,
        Command::Wrte,
        Command::Clse,
        Command::Stls,
    ] {
        assert_eq!(cmd.magic(), cmd.as_u32() ^ 0xffff_ffff);
    }
}
