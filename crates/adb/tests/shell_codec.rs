//! Shell v2 codec tests — pure functions, no device required.

use bytes::Bytes;
use watchdog_adb::shell::{decode_shell_packet, encode_shell_packet, ShellPacket};

fn roundtrip(id: u8, payload: &[u8]) {
    let encoded = encode_shell_packet(id, payload);
    let (packet, consumed) = decode_shell_packet(&encoded).expect("decode");
    assert_eq!(consumed, encoded.len());
    match (id, packet) {
        (1, ShellPacket::Stdout(p)) => assert_eq!(p.as_ref(), payload),
        (2, ShellPacket::Stderr(p)) => assert_eq!(p.as_ref(), payload),
        (3, ShellPacket::Exit(c)) => assert_eq!(c as u8, payload[0]),
        (other, ShellPacket::Other(oid, _)) => assert_eq!(other, oid),
        (4, ShellPacket::CloseStdin) => assert!(payload.is_empty()),
        _ => panic!("unexpected packet for id {id}"),
    }
}

#[test]
fn stdout_roundtrip() {
    roundtrip(1, b"hello\n");
}

#[test]
fn stderr_roundtrip() {
    roundtrip(2, b"boom");
}

#[test]
fn exit_one_byte() {
    roundtrip(3, &[0]);
    let (packet, _) = decode_shell_packet(&encode_shell_packet(3, &[42])).expect("decode");
    assert_eq!(packet, ShellPacket::Exit(42));
}

#[test]
fn exit_four_bytes_le() {
    let (packet, _) = decode_shell_packet(&encode_shell_packet(3, &[1, 0, 0, 0])).expect("decode");
    assert_eq!(packet, ShellPacket::Exit(1));
}

#[test]
fn partial_packet_returns_none() {
    let full = encode_shell_packet(1, b"hello world");
    assert!(decode_shell_packet(&full[..4]).is_none());
    assert!(decode_shell_packet(&full[..8]).is_none());
}

#[test]
fn multiple_packets_in_one_buffer() {
    let mut buf = encode_shell_packet(1, b"out");
    buf.extend_from_slice(&encode_shell_packet(3, &[7]));
    let (first, used) = decode_shell_packet(&buf).expect("first");
    assert_eq!(used, 8);
    let (second, used2) = decode_shell_packet(&buf[used..]).expect("second");
    assert_eq!(used2, buf.len() - used);
    assert!(matches!(first, ShellPacket::Stdout(_)));
    assert_eq!(second, ShellPacket::Exit(7));
}

#[test]
fn malformed_exit_lengths_do_not_panic() {
    for len in [0usize, 2, 3, 5] {
        let payload = vec![1u8; len];
        let (packet, _) = decode_shell_packet(&encode_shell_packet(3, &payload)).expect("decode");
        assert!(matches!(packet, ShellPacket::Exit(_)));
    }
}

#[test]
fn invalid_id_is_reported_not_fatal() {
    let packet = encode_shell_packet(254, b"x");
    let (decoded, _) = decode_shell_packet(&packet).expect("decode");
    assert_eq!(decoded, ShellPacket::Other(254, Bytes::from_static(b"x")));
}
