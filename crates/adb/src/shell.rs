//! Shell v2 protocol — `shell,v2,raw:<command>` service.
//!
//! Packet framing (all integers little-endian): `u8 id | u32 len | len bytes`.
//! IDs per AOSP `shell_protocol.h`: Stdin=0, Stdout=1, Stderr=2, Exit=3,
//! CloseStdin=4, WindowSizeChange=5, Invalid=255.
//!
//! The legacy plain `shell:<command>` service (combined output, no exit code)
//! is available as [`run_shell_legacy`].

use bytes::Bytes;

use crate::connection::{Connection, Stream};
use crate::error::{AdbError, Result};
use crate::transport::Transport;

/// Shell packet ids (AOSP `kId*`).
pub mod ids {
    /// Standard input frame (host → device).
    pub const STDIN: u8 = 0;
    /// Standard output frame (device → host).
    pub const STDOUT: u8 = 1;
    /// Standard error frame (device → host).
    pub const STDERR: u8 = 2;
    /// Exit status frame (1 or 4 byte payload).
    pub const EXIT: u8 = 3;
    /// Signal EOF on stdin.
    pub const CLOSE_STDIN: u8 = 4;
    /// Terminal window size change (unused by watchdog).
    pub const WINDOW_SIZE_CHANGE: u8 = 5;
    /// Peer reported an invalid packet.
    pub const INVALID: u8 = 255;
}

/// Collected output of a shell v2 invocation.
#[derive(Debug, Clone, Default)]
pub struct ShellOutput {
    /// Raw stdout bytes.
    pub stdout: Vec<u8>,
    /// Raw stderr bytes.
    pub stderr: Vec<u8>,
    /// Exit code from the device (`None` if the stream ended without EXIT).
    pub exit_code: Option<i32>,
}

impl ShellOutput {
    /// Stdout lossy-decoded as UTF-8.
    #[must_use]
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Stderr lossy-decoded as UTF-8.
    #[must_use]
    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// One decoded shell packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellPacket {
    /// stdout payload.
    Stdout(Bytes),
    /// stderr payload.
    Stderr(Bytes),
    /// exit code.
    Exit(i32),
    /// peer closed stdout/stdin.
    CloseStdin,
    /// unknown/ignored packet (kept for logging).
    Other(u8, Bytes),
}

/// Decode one shell packet from the front of `buf`.
///
/// Returns `(packet, bytes_consumed)`, or `None` when `buf` holds only a
/// partial packet (caller should buffer more bytes).
#[must_use]
pub fn decode_shell_packet(buf: &[u8]) -> Option<(ShellPacket, usize)> {
    if buf.len() < 5 {
        return None;
    }
    let id = buf[0];
    let len = u32::from_le_bytes(buf[1..5].try_into().expect("slice")) as usize;
    if len > 1024 * 1024 {
        // Not a plausible shell packet; treat as fatal upstream.
        return Some((ShellPacket::Other(ids::INVALID, Bytes::new()), 5));
    }
    if buf.len() < 5 + len {
        return None;
    }
    let payload = Bytes::copy_from_slice(&buf[5..5 + len]);
    let packet = match id {
        ids::STDOUT => ShellPacket::Stdout(payload),
        ids::STDERR => ShellPacket::Stderr(payload),
        ids::EXIT => {
            // Accept 1- and 4-byte exits (AOSP) but never panic on malformed
            // lengths: zero-pad short payloads and keep the last 4 bytes of
            // longer ones.
            let mut code = [0u8; 4];
            let start = payload.len().saturating_sub(4);
            let tail = &payload[start..];
            code[..tail.len()].copy_from_slice(tail);
            ShellPacket::Exit(i32::from_le_bytes(code))
        }
        ids::CLOSE_STDIN => ShellPacket::CloseStdin,
        other => ShellPacket::Other(other, payload),
    };
    Some((packet, 5 + len))
}

/// Encode a shell packet (id + len + payload).
#[must_use]
pub fn encode_shell_packet(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(id);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Run `command` via `shell,v2,raw:` and collect output.
///
/// `stdin` is written in full then signalled EOF (CloseStdin); the device
/// responds with stdout/stderr frames and a final EXIT frame.
pub async fn run_shell<T: Transport + 'static>(
    connection: &Connection<T>,
    command: &str,
    stdin: Option<&[u8]>,
) -> Result<ShellOutput> {
    let stream = connection
        .open_stream(&format!("shell,v2,raw:{command}"))
        .await?;

    if let Some(input) = stdin {
        for chunk in input.chunks(16 * 1024) {
            stream
                .write(Bytes::from(encode_shell_packet(ids::STDIN, chunk)))
                .await?;
        }
        stream
            .write(Bytes::from(encode_shell_packet(ids::CLOSE_STDIN, &[])))
            .await?;
    } else {
        stream
            .write(Bytes::from(encode_shell_packet(ids::CLOSE_STDIN, &[])))
            .await?;
    }

    collect_shell(stream).await
}

/// Run `command` via the legacy `shell:` service (combined stdout+stderr, no exit code).
///
/// Provided for very old devices and emulator edge cases; prefer [`run_shell`].
pub async fn run_shell_legacy<T: Transport + 'static>(
    connection: &Connection<T>,
    command: &str,
) -> Result<ShellOutput> {
    let mut stream = connection.open_stream(&format!("shell:{command}")).await?;
    let mut output = ShellOutput::default();
    let mut buf = Vec::new();
    while let Ok(Some(chunk)) =
        tokio::time::timeout(std::time::Duration::from_secs(3600), stream.inbound.recv()).await
    {
        buf.extend_from_slice(&chunk);
    }
    output.stdout = buf; // legacy merges the streams
    Ok(output)
}

/// Read packets until EXIT or stream end.
async fn collect_shell(mut stream: Stream) -> Result<ShellOutput> {
    let mut output = ShellOutput::default();
    let mut pending: Vec<u8> = Vec::new();

    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(3600), stream.inbound.recv())
            .await
        {
            Ok(Some(chunk)) => {
                pending.extend_from_slice(&chunk);
                while let Some((packet, consumed)) = decode_shell_packet(&pending) {
                    pending.drain(..consumed);
                    match packet {
                        ShellPacket::Stdout(data) => output.stdout.extend_from_slice(&data),
                        ShellPacket::Stderr(data) => output.stderr.extend_from_slice(&data),
                        ShellPacket::Exit(code) => {
                            output.exit_code = Some(code);
                            return Ok(output);
                        }
                        ShellPacket::CloseStdin => {}
                        ShellPacket::Other(id, _) => {
                            tracing::debug!(id, "ignoring shell packet");
                        }
                    }
                }
            }
            Ok(None) => {
                return Ok(output); // stream ended without EXIT
            }
            Err(_) => {
                return Err(AdbError::Timeout(
                    "shell timed out waiting for output".into(),
                ));
            }
        }
    }
}
