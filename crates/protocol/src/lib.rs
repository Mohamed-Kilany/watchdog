//! Wire protocol between the watchdog host and the watchdog Android app.
//!
//! Normative spec: `docs/protocol.md`. Golden vectors: `protocol/vectors/`.
//! The Kotlin app implements the same spec against the same vectors.

#![warn(missing_docs)]

mod decoder;
mod message;
mod packet;

pub use decoder::{Decoder, FeedResult};
pub use message::{
    AppInfo, ByeMessage, CameraDescriptor, Capability, CommandMessage, CommandResultMessage,
    ControlEntry, ControlResult, ControlSource, DeviceInfo, ErrorMessage, EventMessage,
    FramePacket, HelloAck, HostHello, HostInfo, JsonMap, LogChunk, LogLevel, ManifestItem,
    MediaManifest, Message, PingPong, Rotation, StateMessage, STATE_DELTA_FLAG,
};
pub use packet::{MessageType, Packet, HEADER_LEN, MAX_PAYLOAD_LEN, PROTOCOL_VERSION};

/// CRC-32 (IEEE) of `payload`, matching the spec's checksum rules.
#[must_use]
pub fn crc32(payload: &[u8]) -> u32 {
    crc32fast::hash(payload)
}

/// Errors produced by decoding frames or messages.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Frame magic did not match `WDG1`.
    #[error("bad magic (expected WDG1)")]
    BadMagic,
    /// `payload_len` exceeded the 64 MiB protocol limit.
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(u32),
    /// CRC-32 of the payload did not match the header.
    #[error("crc mismatch: header={expected:#010x} computed={actual:#010x}")]
    CrcMismatch {
        /// CRC from the frame header.
        expected: u32,
        /// CRC computed over the received payload.
        actual: u32,
    },
    /// Frame's protocol version is not supported by this build.
    #[error("unsupported protocol version {0} (this build speaks {PROTOCOL_VERSION})")]
    UnsupportedVersion(u8),
    /// Reserved header byte was non-zero.
    #[error("reserved header byte must be 0, got {0}")]
    ReservedNonZero(u8),
    /// JSON payload failed to deserialize.
    #[error("invalid JSON payload for {message_type}: {source}")]
    Json {
        /// Name of the message type being decoded.
        message_type: &'static str,
        /// Serde error with position information.
        source: serde_json::Error,
    },
    /// Binary payload failed to deserialize (wrong length, bad enum, …).
    #[error("invalid binary payload for {message_type}: {reason}")]
    Binary {
        /// Name of the message type being decoded.
        message_type: &'static str,
        /// What was wrong with the payload.
        reason: String,
    },
    /// Unknown message type byte.
    #[error("unknown message type 0x{0:02x}")]
    UnknownType(u8),
}
