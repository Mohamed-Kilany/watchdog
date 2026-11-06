//! Golden-vector conformance tests (spec §7).
//!
//! The Kotlin app must pass the same vectors (see `android/`, M3-T06).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use watchdog_protocol::{Decoder, FeedResult, Message, MessageType};

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("protocol").join("vectors")
}

fn manifest() -> Value {
    let raw = fs::read_to_string(vectors_dir().join("manifest.json")).expect("manifest.json must exist");
    serde_json::from_str(&raw).expect("manifest.json must be valid JSON")
}

#[test]
fn valid_vectors_decode_and_reencode_byte_exact() {
    let manifest = manifest();
    let vectors = manifest["vectors"].as_array().expect("vectors array");
    assert!(vectors.len() >= 14, "expected at least 14 golden vectors");

    for entry in vectors {
        let file = entry["file"].as_str().expect("file");
        let expected_type = entry["type"].as_str().expect("type");
        let bytes = fs::read(vectors_dir().join(file)).unwrap_or_else(|e| panic!("read {file}: {e}"));

        // Decode in one shot.
        let mut decoder = Decoder::new();
        let packets = match decoder.feed(&bytes) {
            Ok(FeedResult::Packets(packets)) => packets,
            Ok(FeedResult::NeedMore) => panic!("{file}: decoder wants more bytes for a complete vector"),
            Err(err) => panic!("{file}: decode failed: {err}"),
        };
        assert_eq!(packets.len(), 1, "{file}: expected exactly one packet");
        let packet = packets.into_iter().next().expect("one packet");
        assert_eq!(packet.msg_type.name(), expected_type, "{file}: message type mismatch");

        let message = Message::from_packet(packet).unwrap_or_else(|e| panic!("{file}: typed decode failed: {e}"));
        assert_eq!(message.msg_type().name(), expected_type);

        // Re-encode must be byte-exact for the committed vectors.
        let reencoded = message.encode_frame().unwrap_or_else(|e| panic!("{file}: re-encode failed: {e}"));
        assert_eq!(reencoded.as_ref(), bytes.as_slice(), "{file}: re-encode mismatch");
    }
}

#[test]
fn valid_vectors_survive_byte_at_a_time_feeding() {
    let manifest = manifest();
    for entry in manifest["vectors"].as_array().expect("vectors array") {
        let file = entry["file"].as_str().expect("file");
        let bytes = fs::read(vectors_dir().join(file)).expect("read vector");
        let mut decoder = Decoder::new();
        let mut packets = Vec::new();
        for byte in &bytes {
            match decoder.feed(std::slice::from_ref(byte)) {
                Ok(FeedResult::Packets(found)) => packets.extend(found),
                Ok(FeedResult::NeedMore) => {}
                Err(err) => panic!("{file}: decode failed at byte level: {err}"),
            }
        }
        assert_eq!(packets.len(), 1, "{file}: byte-at-a-time feed produced wrong packet count");
    }
}

#[test]
fn invalid_vectors_fail_without_panicking() {
    let manifest = manifest();
    for entry in manifest["invalid"].as_array().expect("invalid array") {
        let file = entry["file"].as_str().expect("file");
        let expect = entry["expect"].as_str().expect("expect");
        let bytes = fs::read(vectors_dir().join(file)).expect("read invalid vector");

        let mut decoder = Decoder::new();
        match expect {
            "NeedMore" => match decoder.feed(&bytes) {
                Ok(FeedResult::NeedMore) => {}
                Ok(FeedResult::Packets(_)) => panic!("{file}: expected NeedMore, got packets"),
                Err(err) => panic!("{file}: expected NeedMore, got error: {err}"),
            },
            "MessageDecode" => {
                // Frame is well-formed (header CRC valid) but the typed payload is bad.
                let packet = match decoder.feed(&bytes) {
                    Ok(FeedResult::Packets(packets)) => packets.into_iter().next().expect("one packet"),
                    other => panic!("{file}: expected a packet, got {other:?}"),
                };
                assert!(Message::from_packet(packet).is_err(), "{file}: expected typed decode failure");
            }
            _ => {
                let result = decoder.feed(&bytes);
                let err = match result {
                    Err(err) => err,
                    Ok(ok) => panic!("{file}: expected {expect}, got {ok:?}"),
                };
                let kind = format!("{err:?}");
                assert!(kind.starts_with(expect), "{file}: expected {expect}, got {kind}");
            }
        }
    }
}

#[test]
fn concatenated_frames_are_split_correctly() {
    let one = fs::read(vectors_dir().join("0012-ping.bin")).expect("ping vector");
    let two = fs::read(vectors_dir().join("0013-pong.bin")).expect("pong vector");
    let mut stream = one.clone();
    stream.extend_from_slice(&two);

    let mut decoder = Decoder::new();
    let packets = match decoder.feed(&stream) {
        Ok(FeedResult::Packets(packets)) => packets,
        other => panic!("expected packets, got {other:?}"),
    };
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].msg_type, MessageType::Ping);
    assert_eq!(packets[1].msg_type, MessageType::Pong);
}

/// Deterministic xorshift64 PRNG — no external deps for a robustness sweep.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn garbage_inputs_never_panic() {
    let mut rng = XorShift64(0x9e3779b97f4a7c15);
    for case in 0..20_000u32 {
        let len = (rng.next() % 512) as usize;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push(rng.next() as u8);
        }
        let mut decoder = Decoder::new();
        let _ = decoder.feed(&buf); // must not panic; Ok or Err both fine
        // Also try feeding into a decoder that already saw valid bytes.
        if case % 7 == 0 {
            let valid = fs::read(vectors_dir().join("0001-host-hello.bin")).expect("vector");
            let mut decoder = Decoder::new();
            let _ = decoder.feed(&valid[..7]);
            let _ = decoder.feed(&buf);
        }
    }
}
