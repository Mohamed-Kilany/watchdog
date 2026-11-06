//! Transport abstraction — every higher layer depends only on this trait.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::Result;

/// Byte-stream transport for an ADB session (USB, TCP/TLS, or mock).
///
/// Implementations must provide ordered, reliable delivery of arbitrary byte
/// ranges and must never reorder. Reconnection logic lives above this trait
/// (in the device manager); transports either work or fail.
#[async_trait]
pub trait Transport: Send {
    /// Human-readable name for diagnostics (e.g. `usb:1a2b3c4d` or `tcp:10.0.0.5:5555`).
    fn name(&self) -> String;

    /// Read exactly `buf.len()` bytes; fewer bytes ⇒ `Err`.
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;

    /// Write all bytes.
    async fn write_all(&mut self, buf: &[u8]) -> Result<()>;

    /// Close the transport and release OS handles.
    async fn close(&mut self) -> Result<()>;
}

/// A transport paired with the maximum payload negotiated at CNXN.
#[derive(Debug, Clone, Copy)]
pub struct NegotiatedLimits {
    /// Max bytes the peer asked us to accept in `WRTE` packets.
    pub max_payload: usize,
}

/// Marker for transports that support half-close semantics.
#[async_trait]
pub trait ShutdownTransport: Transport {
    /// Half-close the write side.
    async fn shutdown(&mut self) -> Result<()>;
}

/// Read a `Bytes` buffer of exactly `len` bytes with a timeout.
pub async fn read_timed(
    transport: &mut dyn Transport,
    len: usize,
    timeout: Duration,
) -> Result<Bytes> {
    let mut buf = vec![0u8; len];
    tokio::time::timeout(timeout, transport.read_exact(&mut buf))
        .await
        .map_err(|_| crate::error::AdbError::Timeout(format!("read of {len} bytes timed out")))??;
    Ok(Bytes::from(buf))
}
