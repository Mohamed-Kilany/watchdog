//! Session handshake tests against the scripted mock device.

use bytes::Bytes;
use watchdog_adb::connection::Connection;
use watchdog_adb::error::AdbError;
use watchdog_adb::packet::{AuthType, Command, IncrementalDecoder, Packet};
use watchdog_adb::AdbKeyPair;
use watchdog_adb_mock::MockTransport;

const HOST_BANNER: &str = "host::watchdog-test";

fn test_keys() -> AdbKeyPair {
    let dir = tempfile::tempdir().expect("tempdir");
    // Leak the tempdir to keep the paths stable for the test duration; keys
    // are only written, never re-read in these tests.
    let path = dir.keep();
    AdbKeyPair::load_or_create(&path).expect("load or create adb key")
}

fn device_cnxn_packet() -> Packet {
    Packet::new(
        Command::Cnxn,
        0x0100_0001,
        1024 * 1024,
        b"device::mock:001122334455:mock device\0".as_slice(),
    )
}

fn parse_written(bytes: &[u8]) -> Vec<Packet> {
    IncrementalDecoder::new()
        .feed(bytes, watchdog_adb::packet::MAX_PAYLOAD)
        .expect("host output is well-formed")
}

#[tokio::test]
async fn cnxn_only_adbd_insecure() {
    let keys = test_keys();
    let transport = MockTransport::new("test").script_packet(&device_cnxn_packet());
    let connection = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect("connect");
    assert_eq!(connection.info.maxdata, 1024 * 1024);
    assert!(connection.info.banner.contains("mock device"));
}

#[tokio::test]
async fn auth_token_signature_accepted() {
    let keys = test_keys();
    let token = [0x42u8; 20];
    let transport = MockTransport::new("test")
        .script_packet(&device_cnxn_packet())
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ))
        .script_packet(&device_cnxn_packet());

    // Note: the mock serves scripted bytes in order; the host answers the
    // token with a signature, then reads the final CNXN.
    let connection = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect("connect");
    assert_eq!(connection.info.maxdata, 1024 * 1024);
}

#[tokio::test]
async fn signature_rejected_falls_back_to_public_key() {
    let keys = test_keys();
    let token = [0x7bu8; 20];
    let transport = MockTransport::new("test")
        .script_packet(&device_cnxn_packet())
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ))
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ))
        .script_packet(&device_cnxn_packet());

    let tap = transport.tap();
    let connection = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect("connect");
    assert_eq!(connection.info.maxdata, 1024 * 1024);
    let packets = parse_written(&tap.written());
    let auth_packets: Vec<&Packet> = packets
        .iter()
        .filter(|p| p.command == Command::Auth)
        .collect();
    assert_eq!(
        auth_packets.len(),
        2,
        "expected Signature then RsaPublicKey"
    );
    assert_eq!(auth_packets[0].arg0, AuthType::Signature as u32);
    assert_eq!(auth_packets[0].payload.len(), 256);
    assert_eq!(auth_packets[1].arg0, AuthType::RsaPublicKey as u32);
    assert_eq!(*auth_packets[1].payload.last().unwrap(), 0);
}

#[tokio::test]
async fn unauthorized_after_public_key_sent() {
    let keys = test_keys();
    let token = [0x11u8; 20];
    let transport = MockTransport::new("test")
        .script_packet(&device_cnxn_packet())
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ))
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ))
        .script_packet(&Packet::new(
            Command::Auth,
            AuthType::Token as u32,
            0,
            Bytes::from_static(&token),
        ));

    let err = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect_err("device never accepts");
    assert!(matches!(err, AdbError::Unauthorized), "got {err:?}");
}

#[tokio::test]
async fn peer_version_too_old_is_rejected() {
    let keys = test_keys();
    let transport = MockTransport::new("test").script_packet(&Packet::new(
        Command::Cnxn,
        0x0000_0001,
        4096,
        b"device::old\0".as_slice(),
    ));
    let err = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect_err("old adbd must be refused");
    assert!(matches!(err, AdbError::VersionMismatch(_)), "got {err:?}");
}

#[tokio::test]
async fn open_stream_ok() {
    let keys = test_keys();
    let transport = MockTransport::new("test")
        .script_packet(&device_cnxn_packet())
        .script_packet(&Packet::new(Command::Okay, 0xdead_beef, 0x4000, &[][..]));
    let connection = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect("connect");
    let stream = connection
        .open_stream("shell:id")
        .await
        .expect("open stream");
    assert_eq!(stream.remote_id(), 0xdead_beef);
    assert_eq!(stream.local_id(), 0x4000);
}

#[tokio::test]
async fn open_stream_denied() {
    let keys = test_keys();
    let transport = MockTransport::new("test")
        .script_packet(&device_cnxn_packet())
        .script_packet(&Packet::new(Command::Clse, 0, 0x4000, &[][..]));
    let connection = Connection::connect(transport, HOST_BANNER, &keys)
        .await
        .expect("connect");
    let err = connection
        .open_stream("shell:id")
        .await
        .expect_err("device refused the stream");
    assert!(matches!(err, AdbError::StreamClosed), "got {err:?}");
}
