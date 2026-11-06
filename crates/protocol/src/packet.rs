//! Frame header and [`Packet`] — the transport-agnostic unit of the protocol.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::crc32;
use crate::DecodeError;

/// Protocol version implemented by this build.
pub const PROTOCOL_VERSION: u8 = 1;
/// Size of the fixed frame header in bytes.
pub const HEADER_LEN: usize = 16;
/// Maximum accepted `payload_len` (64 MiB). Larger frames are rejected.
pub const MAX_PAYLOAD_LEN: u32 = 64 * 1024 * 1024;

const MAGIC: [u8; 4] = *b"WDG1";

/// Message type byte values (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    /// Host → app: handshake request.
    HostHello = 0x01,
    /// App → host: handshake reply.
    HelloAck = 0x02,
    /// App → host: viewfinder JPEG frame.
    Frame = 0x10,
    /// App → host: control/camera state (full or delta).
    State = 0x20,
    /// Host → app: command.
    Command = 0x21,
    /// App → host: per-command acknowledgements.
    CommandResult = 0x22,
    /// App → host: asynchronous event.
    Event = 0x30,
    /// App → host: a media item is ready to pull.
    MediaManifest = 0x40,
    /// App → host: unredacted device log ring-buffer chunk.
    LogChunk = 0x50,
    /// App → host: fatal/transient error report.
    Error = 0x60,
    /// Host → app: liveness/latency probe.
    Ping = 0x70,
    /// App → host: liveness/latency reply.
    Pong = 0x71,
    /// Either direction: orderly close.
    Bye = 0x7f,
}

impl MessageType {
    /// Numeric value on the wire.
    #[must_use]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Stable name used in logs and manifests.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::HostHello => "HOST_HELLO",
            Self::HelloAck => "HELLO_ACK",
            Self::Frame => "FRAME",
            Self::State => "STATE",
            Self::Command => "COMMAND",
            Self::CommandResult => "COMMAND_RESULT",
            Self::Event => "EVENT",
            Self::MediaManifest => "MEDIA_MANIFEST",
            Self::LogChunk => "LOG_CHUNK",
            Self::Error => "ERROR",
            Self::Ping => "PING",
            Self::Pong => "PONG",
            Self::Bye => "BYE",
        }
    }

    /// Map a raw type byte to a known type, or `None`.
    #[must_use]
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::HostHello,
            0x02 => Self::HelloAck,
            0x10 => Self::Frame,
            0x20 => Self::State,
            0x21 => Self::Command,
            0x22 => Self::CommandResult,
            0x30 => Self::Event,
            0x40 => Self::MediaManifest,
            0x50 => Self::LogChunk,
            0x60 => Self::Error,
            0x70 => Self::Ping,
            0x71 => Self::Pong,
            0x7f => Self::Bye,
            _ => return None,
        })
    }
}

/// A decoded frame: type, flags and payload (header consumed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// The message type byte.
    pub msg_type: MessageType,
    /// Per-message flags (e.g. STATE delta bit 0x01).
    pub flags: u8,
    /// Payload bytes (CRC-verified).
    pub payload: Bytes,
}

impl Packet {
    /// Build a packet (validates the size limit).
    pub fn new(msg_type: MessageType, flags: u8, payload: impl Into<Bytes>) -> Result<Self, DecodeError> {
        let payload = payload.into();
        if payload.len() as u64 > u64::from(MAX_PAYLOAD_LEN) {
            return Err(DecodeError::PayloadTooLarge(u32::try_from(payload.len()).unwrap_or(u32::MAX)));
        }
        Ok(Self { msg_type, flags, payload })
    }

    /// Serialize to a complete frame (16-byte header + payload).
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_LEN + self.payload.len());
        buf.put_slice(&MAGIC);
        buf.put_u8(PROTOCOL_VERSION);
        buf.put_u8(self.msg_type.as_u8());
        buf.put_u8(self.flags);
        buf.put_u8(0); // reserved
        buf.put_u32_le(u32::try_from(self.payload.len()).expect("validated at construction"));
        buf.put_u32_le(crc32(&self.payload));
        buf.put_slice(&self.payload);
        buf.freeze()
    }

    /// Parse one complete frame from `bytes` (exactly one frame expected).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let header = parse_header(bytes)?;
        let payload_end = HEADER_LEN + header.payload_len as usize;
        if bytes.len() != payload_end {
            return Err(DecodeError::Binary {
                message_type: "FRAME",
                reason: format!("expected {} bytes, got {}", payload_end, bytes.len()),
            });
        }
        let payload = Bytes::copy_from_slice(&bytes[HEADER_LEN..payload_end]);
        let actual = crc32(&payload);
        if actual != header.crc32 {
            return Err(DecodeError::CrcMismatch { expected: header.crc32, actual });
        }
        Ok(Self { msg_type: header.msg_type, flags: header.flags, payload })
    }
}

/// Result of looking at a byte prefix for framing purposes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    pub(crate) msg_type: MessageType,
    pub(crate) flags: u8,
    pub(crate) payload_len: u32,
    pub(crate) crc32: u32,
}

/// Parse and validate the fixed 16-byte header.
pub(crate) fn parse_header(bytes: &[u8]) -> Result<Header, DecodeError> {
    if bytes.len() < HEADER_LEN {
        return Err(DecodeError::Binary {
            message_type: "FRAME",
            reason: format!("truncated header: {} bytes", bytes.len()),
        });
    }
    let mut cur: &[u8] = &bytes[..HEADER_LEN];
    let mut magic = [0u8; 4];
    cur.copy_to_slice(&mut magic);
    if magic != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = cur.get_u8();
    if version != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let type_byte = cur.get_u8();
    let flags = cur.get_u8();
    let reserved = cur.get_u8();
    if reserved != 0 {
        return Err(DecodeError::ReservedNonZero(reserved));
    }
    let payload_len = cur.get_u32_le();
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLarge(payload_len));
    }
    let crc = cur.get_u32_le();
    let msg_type = MessageType::from_u8(type_byte).ok_or(DecodeError::UnknownType(type_byte))?;
    Ok(Header { msg_type, flags, payload_len, crc32: crc })
}
