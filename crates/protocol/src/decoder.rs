//! Incremental frame decoder for stream transports.

use bytes::BytesMut;

use crate::crc32;
use crate::packet::{parse_header, Packet, HEADER_LEN};
use crate::DecodeError;

/// Result of feeding bytes into the decoder.
#[derive(Debug)]
pub enum FeedResult {
    /// More bytes are needed to complete a frame.
    NeedMore,
    /// One or more complete frames were decoded.
    Packets(Vec<Packet>),
}

/// Streaming decoder: feed arbitrary chunks, get complete, CRC-verified frames.
///
/// A single call may yield zero or more packets. Any structural violation
/// (bad magic, oversized payload, CRC mismatch) is returned as `Err` and the
/// caller is expected to drop the connection — resynchronisation is not
/// attempted by design.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: BytesMut,
}

impl Decoder {
    /// New, empty decoder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Bytes currently buffered (unparsed). Exposed for diagnostics/tests.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// Append `input` and drain all complete frames.
    pub fn feed(&mut self, input: &[u8]) -> Result<FeedResult, DecodeError> {
        self.buf.extend_from_slice(input);
        let mut packets = Vec::new();
        loop {
            if self.buf.len() < HEADER_LEN {
                break;
            }
            let header = parse_header(&self.buf)?;
            let total = HEADER_LEN + header.payload_len as usize;
            if self.buf.len() < total {
                break; // wait for more bytes; never partially emit
            }
            let frame = self.buf.split_to(total).freeze();
            let payload = frame.slice(HEADER_LEN..);
            let actual = crc32(payload.as_ref());
            if actual != header.crc32 {
                return Err(DecodeError::CrcMismatch {
                    expected: header.crc32,
                    actual,
                });
            }
            packets.push(Packet {
                msg_type: header.msg_type,
                flags: header.flags,
                payload,
            });
        }
        if packets.is_empty() {
            Ok(FeedResult::NeedMore)
        } else {
            Ok(FeedResult::Packets(packets))
        }
    }
}
