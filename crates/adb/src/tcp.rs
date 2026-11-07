//! TCP transport for ADB over a LAN listener (`adb tcpip` / wireless debugging).
//!
//! Plain-TCP devices (pre-STLS) speak the raw wire protocol. STLS-upgraded
//! connections (wireless debugging connect port) are handled by a TLS wrapper
//! in [`crate::stls`]; both share this trait.

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::TcpStream;

use crate::error::{AdbError, Result};
use crate::transport::Transport;

/// Default TCP connect timeout.
pub const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A TCP-backed [`Transport`].
#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
    peer: SocketAddr,
}

impl TcpTransport {
    /// Connect to `addr` with [`TCP_CONNECT_TIMEOUT`].
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let stream = tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| AdbError::Timeout(format!("TCP connect to {addr} timed out")))?
            .map_err(|e| AdbError::Transport(format!("TCP connect to {addr} failed: {e}")))?;
        stream.set_nodelay(true).ok();
        Ok(Self { stream, peer: addr })
    }

    /// Wrap an already-connected stream (used by discovery/tests).
    #[must_use]
    pub fn from_stream(stream: TcpStream, peer: SocketAddr) -> Self {
        Self { stream, peer }
    }

    /// Peer address.
    #[must_use]
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }
}

#[async_trait]
impl Transport for TcpTransport {
    fn name(&self) -> String {
        format!("tcp:{}", self.peer)
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        use tokio::io::AsyncReadExt;
        self.stream
            .read_exact(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("tcp read: {e}")))
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.stream
            .write_all(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("tcp write: {e}")))
    }

    async fn close(&mut self) -> Result<()> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}
