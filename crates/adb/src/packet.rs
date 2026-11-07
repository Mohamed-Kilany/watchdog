//! ADB wire protocol: the 24-byte packet header and command constants.
//!
//! Normative references (AOSP `packages/modules/adb`):
//! - `protocol.txt` — framing; - `adb.h` — constants; - `types.h` — header layout.
//!
//! Layout (all fields little-endian u32):
//!
//! ```text
//! offset  field
//! 0       command      (A_CNXN, A_OPEN, …; see [`Command`])
//! 4       arg0
//! 8       arg1
//! 12      data_length  (payload bytes following the header)
//! 16      data_checksum (0 for protocol version ≥ 0x01000001)
//! 20      magic = command ^ 0xffffffff
//! ```

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::{AdbError, Result};

/// Header size in bytes.
pub const HEADER_LEN: usize = 24;
/// Protocol version we negotiate (≥ 0x01000001 ⇒ checksum unused).
pub const A_VERSION: u32 = 0x0100_0001;
/// Oldest peer version we accept.
pub const A_VERSION_MIN: u32 = 0x0100_0000;
/// Version at which the payload checksum stopped being verified.
pub const A_VERSION_SKIP_CHECKSUM: u32 = 0x0100_0001;
/// Stream-based TLS upgrade offer (STLS), current version.
pub const A_STLS_VERSION: u32 = 0x0100_0000;
/// Maximum payload we advertise pre-negotiation (AOSP caps CONNECT/AUTH at this).
pub const MAX_PAYLOAD_V1: usize = 4 * 1024;
/// Maximum payload after negotiation.
pub const MAX_PAYLOAD: usize = 1024 * 1024;
/// Proposal in CNXN for maxdata (adbd honors 1 MiB on modern versions).
pub const DEFAULT_MAXDATA: u32 = MAX_PAYLOAD as u32;
/// Default local id base (0 is reserved).
const LOCAL_ID_START: u32 = 0x0000_4000;

/// Command identifiers (AOSP `A_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Command {
    /// Legacy sync beacon (unused on the wire today).
    Sync = 0x434e_5953,
    /// Connection negotiation.
    Cnxn = 0x4e58_4e43,
    /// Authentication exchange.
    Auth = 0x4854_5541,
    /// Open a stream to a service.
    Open = 0x4e45_504f,
    /// Stream ready acknowledgement.
    Okay = 0x5941_4b4f,
    /// Stream data.
    Wrte = 0x4554_5257,
    /// Close stream.
    Clse = 0x4553_4c43,
    /// TLS upgrade offer (then TLS 1.3 handshake resumes the session).
    Stls = 0x534c_5453,
}

impl Command {
    /// Raw 32-bit id.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self as u32
    }

    /// Magic companion value (`command ^ 0xffffffff`).
    #[must_use]
    pub fn magic(self) -> u32 {
        self.as_u32() ^ 0xffff_ffff
    }

    /// Decode a raw id; `None` for unknown commands.
    #[must_use]
    pub fn from_u32(value: u32) -> Option<Self> {
        Some(match value {
            0x434e_5953 => Self::Sync,
            0x4e58_4e43 => Self::Cnxn,
            0x4854_5541 => Self::Auth,
            0x4e45_504f => Self::Open,
            0x5941_4b4f => Self::Okay,
            0x4554_5257 => Self::Wrte,
            0x4553_4c43 => Self::Clse,
            0x534c_5453 => Self::Stls,
            _ => return None,
        })
    }

    /// Stable name for logs.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sync => "SYNC",
            Self::Cnxn => "CNXN",
            Self::Auth => "AUTH",
            Self::Open => "OPEN",
            Self::Okay => "OKAY",
            Self::Wrte => "WRTE",
            Self::Clse => "CLSE",
            Self::Stls => "STLS",
        }
    }
}

/// Authentication sub-type (`arg0` of `AUTH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum AuthType {
    /// Challenge token (20 random bytes).
    Token = 1,
    /// RSA signature over the token (SHA-1, PKCS#1 v1.5).
    Signature = 2,
    /// Base64 Android public key + " " + user@host + NUL.
    RsaPublicKey = 3,
}

/// A decoded ADB packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// Command.
    pub command: Command,
    /// First argument (meaning depends on command).
    pub arg0: u32,
    /// Second argument (meaning depends on command).
    pub arg1: u32,
    /// Payload bytes (may be empty).
    pub payload: Bytes,
}

impl Packet {
    /// Create a packet with id-independent fields.
    #[must_use]
    pub fn new(command: Command, arg0: u32, arg1: u32, payload: impl Into<Bytes>) -> Self {
        Self {
            command,
            arg0,
            arg1,
            payload: payload.into(),
        }
    }

    /// Serialize to header + payload.
    #[must_use]
    pub fn encode(&self) -> BytesMut {
        let payload = &self.payload;
        debug_assert!(payload.len() <= MAX_PAYLOAD);
        let mut out = BytesMut::with_capacity(HEADER_LEN + payload.len());
        out.put_u32_le(self.command.as_u32());
        out.put_u32_le(self.arg0);
        out.put_u32_le(self.arg1);
        out.put_u32_le(u32::try_from(payload.len()).expect("payload bounded"));
        out.put_u32_le(0); // checksum unused at our negotiated version
        out.put_u32_le(self.command.magic());
        out.put_slice(payload);
        out
    }
}

/// Parse the 24-byte header from the first 24 bytes of `buf`.
///
/// Returns `(packet-meta, payload_len)` on success.
fn parse_header(buf: &[u8]) -> Result<(Command, u32, u32, usize)> {
    debug_assert_eq!(buf.len(), HEADER_LEN);
    let mut cur = buf;
    let command_id = cur.get_u32_le();
    let arg0 = cur.get_u32_le();
    let arg1 = cur.get_u32_le();
    let data_length = cur.get_u32_le() as usize;
    let _checksum = cur.get_u32_le();
    let magic = cur.get_u32_le();
    let command = Command::from_u32(command_id)
        .ok_or_else(|| AdbError::Protocol(format!("unknown command 0x{command_id:08x}")))?;
    if magic != command.magic() {
        return Err(AdbError::BadMagic {
            command: command_id,
            magic,
        });
    }
    if data_length > MAX_PAYLOAD {
        return Err(AdbError::PayloadTooLarge(data_length));
    }
    Ok((command, arg0, arg1, data_length))
}

/// Errors from [`IncrementalDecoder`].
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Header or payload violates the wire format.
    #[error(transparent)]
    Adb(#[from] AdbError),
}

/// Incremental decoder for packet streams (any transport).
///
/// Feed arbitrary chunks; complete packets come out. Any structural violation
/// fails the connection (protocol resync is not attempted, by design).
#[derive(Default)]
pub struct IncrementalDecoder {
    buf: BytesMut,
}

impl IncrementalDecoder {
    /// Empty decoder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: BytesMut::with_capacity(HEADER_LEN + MAX_PAYLOAD),
        }
    }

    /// Buffered bytes not yet consumed (header + partial payload).
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// Append `input` and drain all complete packets.
    ///
    /// `max_payload` may be large-pre-CNXN (4 KiB) or 1 MiB after negotiation;
    /// packets declaring more are rejected.
    pub fn feed(&mut self, input: &[u8], max_payload: usize) -> Result<Vec<Packet>> {
        self.buf.extend_from_slice(input);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < HEADER_LEN {
                break;
            }
            let header = parse_header(&self.buf[..HEADER_LEN])?;
            let (command, arg0, arg1, data_length) = header;
            if data_length > max_payload {
                return Err(AdbError::PayloadTooLarge(data_length));
            }
            if self.buf.len() < HEADER_LEN + data_length {
                break; // wait for the rest of the payload
            }
            let _ = self.buf.split_to(HEADER_LEN);
            let payload = self.buf.split_to(data_length).freeze();
            out.push(Packet {
                command,
                arg0,
                arg1,
                payload,
            });
        }
        Ok(out)
    }
}

/// Monotonic local stream id allocator (starts at a non-zero base).
#[derive(Debug)]
pub struct LocalIdAllocator(u32);

impl Default for LocalIdAllocator {
    fn default() -> Self {
        Self(LOCAL_ID_START)
    }
}

impl LocalIdAllocator {
    /// Next free id.
    pub fn next_id(&mut self) -> u32 {
        let id = self.0;
        self.0 = self.0.wrapping_add(1).max(1); // never returns 0
        id
    }
}

/// Minimal logging helpers.
impl std::fmt::Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} arg0={} arg1={} len={}",
            self.command.name(),
            self.arg0,
            self.arg1,
            self.payload.len()
        )
    }
}
