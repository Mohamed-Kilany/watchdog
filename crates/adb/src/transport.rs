//! Transport abstraction — every higher layer depends only on this trait.

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::Result;

/// Read half of a byte-stream transport.
///
/// Read and write halves are independent so the session reader can block on
/// inbound packets while the writer sends outbound ones (an install command
/// that produces no output until it receives stdin deadlocks otherwise).
#[async_trait]
pub trait TransportReader: Send {
    /// Read exactly `buf.len()` bytes; fewer bytes ⇒ `Err`.
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// Write half of a byte-stream transport.
#[async_trait]
pub trait TransportWriter: Send {
    /// Write all bytes.
    async fn write_all(&mut self, buf: &[u8]) -> Result<()>;
}

/// Byte-stream transport for an ADB session (USB, TCP/TLS, or mock).
///
/// Implementations must provide ordered, reliable delivery of arbitrary byte
/// ranges and must never reorder. Reconnection logic lives above this trait
/// (in the device manager); transports either work or fail.
#[async_trait]
pub trait Transport: TransportReader + TransportWriter + Send + 'static {
    /// Human-readable name for diagnostics (e.g. `usb:1a2b3c4d` or `tcp:10.0.0.5:5555`).
    fn name(&self) -> String;

    /// Concrete read half produced by [`Transport::split`].
    type Reader: TransportReader + 'static;
    /// Concrete write half produced by [`Transport::split`].
    type Writer: TransportWriter + 'static;

    /// Split into independent halves for the connection's reader/writer tasks.
    fn split(self) -> (Self::Reader, Self::Writer);

    /// Close the transport and release OS handles.
    async fn close(&mut self) -> Result<()>;
}

#[async_trait]
impl<T> TransportReader for tokio::io::ReadHalf<T>
where
    T: tokio::io::AsyncRead + Send,
{
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        tokio::io::AsyncReadExt::read_exact(self, buf)
            .await
            .map(|_| ())
            .map_err(|e| crate::error::AdbError::Transport(format!("read: {e}")))
    }
}

#[async_trait]
impl<T> TransportWriter for tokio::io::WriteHalf<T>
where
    T: tokio::io::AsyncWrite + Send,
{
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        tokio::io::AsyncWriteExt::write_all(self, buf)
            .await
            .map_err(|e| crate::error::AdbError::Transport(format!("write: {e}")))
    }
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
pub async fn read_timed<T: TransportReader + ?Sized>(
    transport: &mut T,
    len: usize,
    timeout: Duration,
) -> Result<Bytes> {
    let mut buf = vec![0u8; len];
    tokio::time::timeout(timeout, transport.read_exact(&mut buf))
        .await
        .map_err(|_| crate::error::AdbError::Timeout(format!("read of {len} bytes timed out")))??;
    Ok(Bytes::from(buf))
}
