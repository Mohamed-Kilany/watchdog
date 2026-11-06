//! Regenerate the golden vectors under `protocol/vectors/`.
//!
//! Usage: `cargo run -p watchdog-protocol --example generate-vectors`
//!
//! Every value below is fixed; running this twice is byte-identical. CI
//! checks the vectors are in sync by rebuilding and diffing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use watchdog_protocol::*;

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("protocol")
        .join("vectors")
}

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write vector");
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
}

fn fake_jpeg() -> Bytes {
    // Opaque to the codec; a structurally complete stub of a JPEG.
    Bytes::from_static(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x02\x00\x00\x01\x00\x01\x00\x00\xde\xad\xbe\xefPAYLOAD\xff\xd9")
}

fn state_entries() -> BTreeMap<String, ControlEntry> {
    let mut iso = BTreeMap::new();
    iso.insert(
        "iso".to_string(),
        ControlEntry {
            requested: Some(serde_json::json!(400)),
            actual: Some(serde_json::json!(398)),
            locked: Some(true),
            supported: true,
            source: ControlSource::Host,
        },
    );
    iso.insert(
        "exposure_time_ns".to_string(),
        ControlEntry {
            requested: Some(serde_json::json!(33_333_333)),
            actual: Some(serde_json::json!(33_333_333)),
            locked: Some(true),
            supported: true,
            source: ControlSource::Host,
        },
    );
    iso.insert(
        "recording".to_string(),
        ControlEntry {
            requested: None,
            actual: Some(serde_json::json!(false)),
            locked: None,
            supported: true,
            source: ControlSource::Device,
        },
    );
    iso
}

fn main() {
    let dir = vectors_dir();
    fs::create_dir_all(dir.join("invalid")).expect("create vectors dir");

    let mut manifest_vectors = Vec::new();
    let mut emit = |name: &str, ty: &str, summary: &str, msg: Message| {
        let frame = msg.encode_frame().expect("encode");
        write(&dir, name, &frame);
        manifest_vectors.push(serde_json::json!({ "file": name, "type": ty, "summary": summary }));
    };

    emit(
        "0001-host-hello.bin",
        "HOST_HELLO",
        "Host handshake request with min/max protocol",
        Message::HostHello(HostHello {
            protocol_min: 1,
            protocol_max: 1,
            host: HostInfo {
                name: "watchdog".to_string(),
                version: "0.1.0".to_string(),
            },
            session_token: "00112233445566778899aabbccddeeff".to_string(),
        }),
    );

    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "0".to_string(),
        serde_json::json!({
            "hardware_level": "FULL",
            "ae_mode": {"supported": true, "modes": [0, 1, 2]},
            "iso": {"supported": true, "min": 50, "max": 12800, "step": 1},
            "zoom_ratio": {"supported": false, "reason": "requires API 30"}
        }),
    );
    emit(
        "0002-hello-ack.bin",
        "HELLO_ACK",
        "Handshake reply: app/device identity, cameras, capabilities",
        Message::HelloAck(HelloAck {
            protocol_version: 1,
            app: AppInfo {
                version_code: 1,
                version_name: "0.1.0".to_string(),
                package: "app.watchdog.remote".to_string(),
            },
            device: DeviceInfo {
                manufacturer: "Google".to_string(),
                model: "Pixel 7".to_string(),
                device: "panther".to_string(),
                sdk: 34,
                release: "14".to_string(),
                abi: "arm64-v8a".to_string(),
                battery_percent: Some(87),
                storage_free_mb: Some(104_857_600),
                temperature_c: Some(29.5),
            },
            cameras: vec![CameraDescriptor {
                id: "0".to_string(),
                facing: "back".to_string(),
                hardware_level: "FULL".to_string(),
                logical_multi_camera: true,
                physical_ids: vec!["3".to_string(), "4".to_string()],
            }],
            capabilities,
        }),
    );

    emit(
        "0003-frame.bin",
        "FRAME",
        "Viewfinder JPEG frame with sequence and sensor timestamp",
        Message::Frame(FramePacket {
            seq: 42,
            sensor_timestamp_ns: 1_234_567_890_123,
            width: 1280,
            height: 720,
            rotation: Rotation::R90,
            flags: 0b10, // is_snapshot
            jpeg: fake_jpeg(),
        }),
    );

    emit(
        "0004-state-full.bin",
        "STATE",
        "Full control state snapshot",
        Message::State(StateMessage {
            seq: 1543,
            timestamp_ms: 1_730_900_000_123,
            camera_id: "0".to_string(),
            state: state_entries(),
            dropped_frames: 3,
            storage_free_mb: Some(104_857_600),
            delta: false,
        }),
    );

    let mut delta_state = state_entries();
    delta_state.retain(|k, _| k == "iso");
    emit(
        "0005-state-delta.bin",
        "STATE",
        "Delta state (header flag 0x01), ISO only",
        Message::State(StateMessage {
            seq: 1544,
            timestamp_ms: 1_730_900_000_223,
            camera_id: "0".to_string(),
            state: delta_state,
            dropped_frames: 3,
            storage_free_mb: None,
            delta: true,
        }),
    );

    emit(
        "0006-command-set-controls.bin",
        "COMMAND",
        "set_controls batch with one control",
        Message::Command(CommandMessage {
            id: "c-000001".to_string(),
            command: "set_controls".to_string(),
            params: serde_json::json!({"controls": [{"id": "iso", "value": 400}]}),
        }),
    );

    emit(
        "0007-command-result.bin",
        "COMMAND_RESULT",
        "Per-control acknowledgement with applied value",
        Message::CommandResult(CommandResultMessage {
            id: "c-000001".to_string(),
            timestamp_ms: 1_730_900_000_456,
            results: vec![ControlResult {
                control: Some("iso".to_string()),
                ok: true,
                applied: Some(serde_json::json!(398)),
                error_code: None,
                message: None,
            }],
            error: None,
        }),
    );

    emit(
        "0008-event-camera-disconnected.bin",
        "EVENT",
        "Camera was disconnected (USB unplugged / taken by another app)",
        Message::Event(EventMessage {
            kind: "camera_disconnected".to_string(),
            timestamp_ms: 1_730_900_001_000,
            data: Some(serde_json::json!({"camera_id": "0"})),
        }),
    );

    emit(
        "0009-media-manifest.bin",
        "MEDIA_MANIFEST",
        "Recorded video ready to pull; hash + size for verification",
        Message::MediaManifest(MediaManifest {
            item: ManifestItem {
                id: "m-01JQZ9K0R0W4YV3B8N7E2M6XTD".to_string(),
                kind: "video".to_string(),
                created_ms: 1_730_900_000_000,
                duration_ms: Some(10_042),
                width: 1920,
                height: 1080,
                bytes: 15_204_301,
                sha256: "9f1cb5a3f4d8e7c6b5a4938271605f4e3d2c1b0a99887766554433221100ffeed"
                    .to_string(),
                device_path: "files/media/m-01JQZ9K0R0W4YV3B8N7E2M6XTD.mp4".to_string(),
                mime: "video/mp4".to_string(),
                control_snapshot: Some({
                    let mut m = BTreeMap::new();
                    m.insert("iso".to_string(), serde_json::json!(400));
                    m
                }),
                device: Some({
                    let mut m = BTreeMap::new();
                    m.insert("model".to_string(), serde_json::json!("Pixel 7"));
                    m.insert("sdk".to_string(), serde_json::json!(34));
                    m
                }),
                host_job_id: None,
            },
        }),
    );

    emit(
        "0010-log-chunk.bin",
        "LOG_CHUNK",
        "Device log ring-buffer chunk (INFO level, UTF-8)",
        Message::LogChunk(LogChunk {
            timestamp_ms: 1_730_900_002_000,
            level: LogLevel::Info,
            text: "watchdog: camera opened id=0\nwatchdog: stream started 1280x720@30".to_string(),
        }),
    );

    emit(
        "0011-error-version-mismatch.bin",
        "ERROR",
        "Fatal handshake error: no common protocol version",
        Message::Error(ErrorMessage {
            code: "PROTOCOL_VERSION_MISMATCH".to_string(),
            message: "no common protocol version (host max 1, app min 2)".to_string(),
            hint: Some("upgrade watchdog on the host".to_string()),
        }),
    );

    emit(
        "0012-ping.bin",
        "PING",
        "Liveness/latency probe from host",
        Message::Ping(PingPong {
            ts_ms: 1_730_900_003_000,
            nonce: Some("a1b2c3d4e5f6a7b8".to_string()),
            app_ts_ns: None,
        }),
    );

    emit(
        "0013-pong.bin",
        "PONG",
        "Liveness/latency reply with app boottime",
        Message::Pong(PingPong {
            ts_ms: 1_730_900_003_012,
            nonce: Some("a1b2c3d4e5f6a7b8".to_string()),
            app_ts_ns: Some(87_654_321_098),
        }),
    );

    emit(
        "0014-bye.bin",
        "BYE",
        "Orderly session close (host shutdown)",
        Message::Bye(ByeMessage {
            reason: "host_shutdown".to_string(),
            detail: None,
        }),
    );

    // --- invalid vectors: derived from the valid ones unless noted ---
    let hello = fs::read(dir.join("0001-host-hello.bin")).expect("vector exists");
    let frame = fs::read(dir.join("0003-frame.bin")).expect("vector exists");

    let mut bad_magic = hello.clone();
    bad_magic[0] = b'X';
    write(&dir, "invalid/bad-magic.bin", &bad_magic);

    write(&dir, "invalid/truncated-header.bin", &hello[..10]);
    write(
        &dir,
        "invalid/truncated-payload.bin",
        &hello[..hello.len() - 8],
    );

    let mut bad_crc = hello.clone();
    let last = bad_crc.len() - 1;
    bad_crc[last] ^= 0x01;
    write(&dir, "invalid/crc-mismatch.bin", &bad_crc);

    // Header claims payload_len = MAX+1; decoder must reject before allocating.
    let mut oversized = hello[..HEADER_LEN].to_vec();
    oversized[8..12].copy_from_slice(&(MAX_PAYLOAD_LEN + 1).to_le_bytes());
    write(&dir, "invalid/oversized-length.bin", &oversized);

    let mut bad_version = hello.clone();
    bad_version[4] = 0x7f;
    write(&dir, "invalid/unsupported-version.bin", &bad_version);

    let mut reserved_nonzero = hello.clone();
    reserved_nonzero[7] = 1;
    write(&dir, "invalid/reserved-nonzero.bin", &reserved_nonzero);

    let mut unknown_type = hello.clone();
    unknown_type[5] = 0x55;
    // Fix crc so only the type byte is invalid.
    let payload_len = u32::from_le_bytes(unknown_type[8..12].try_into().expect("len")) as usize;
    let payload_start = HEADER_LEN;
    let crc = {
        let payload = &unknown_type[payload_start..payload_start + payload_len];
        crc32(payload)
    };
    unknown_type[12..16].copy_from_slice(&crc.to_le_bytes());
    write(&dir, "invalid/unknown-type.bin", &unknown_type);

    // Frame payload with rotation 45 — reserved bytes stay zero, CRC is valid,
    // but the binary payload itself is invalid and must be rejected at decode time.
    let mut bad_frame = frame;
    bad_frame[HEADER_LEN + 24] = 45;
    bad_frame[HEADER_LEN + 25] = 0;
    let payload_len = u32::from_le_bytes(bad_frame[8..12].try_into().expect("len")) as usize;
    let crc = crc32(&bad_frame[HEADER_LEN..HEADER_LEN + payload_len]);
    bad_frame[12..16].copy_from_slice(&crc.to_le_bytes());
    write(&dir, "invalid/frame-bad-rotation.bin", &bad_frame);

    // Trailing-byte truncation is not an error: a stream decoder must wait for more bytes.
    let invalid = vec![
        ("invalid/bad-magic.bin", "BadMagic"),
        ("invalid/truncated-header.bin", "NeedMore"),
        ("invalid/truncated-payload.bin", "NeedMore"),
        ("invalid/crc-mismatch.bin", "CrcMismatch"),
        ("invalid/oversized-length.bin", "PayloadTooLarge"),
        ("invalid/unsupported-version.bin", "UnsupportedVersion"),
        ("invalid/reserved-nonzero.bin", "ReservedNonZero"),
        ("invalid/unknown-type.bin", "UnknownType"),
        ("invalid/frame-bad-rotation.bin", "MessageDecode"),
    ];

    let manifest = serde_json::json!({
        "version": 1,
        "note": "Golden vectors for the watchdog wire protocol. See docs/protocol.md §7. Regenerate with `cargo run -p watchdog-protocol --example generate-vectors`.",
        "vectors": manifest_vectors,
        "invalid": invalid.iter().map(|(file, expect)| serde_json::json!({ "file": file, "expect": expect })).collect::<Vec<_>>(),
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");
    println!("wrote {}", dir.join("manifest.json").display());
}
