//! In-process port forwarding: local TCP listener ↔ device service via ADB.
//!
//! Mirrors `adb forward`: each accepted local connection opens a new stream to
//! the same device service and proxies bytes in both directions with bounded
//! buffers. Listeners are tied to the owning [`Connection`]'s lifetime; dropping
//! the handle stops accepting (in-flight streams finish or die with the device).

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::connection::Connection;
use crate::error::{AdbError, Result};
use crate::transport::Transport;

const PROXY_BUF: usize = 64 * 1024;

/// Handle for a running forward; aborts on drop/call.
#[derive(Debug)]
pub struct ForwardHandle {
    local_addr: SocketAddr,
    /// Device service being forwarded (e.g. `localabstract:watchdog`).
    pub service: String,
    join: JoinHandle<()>,
}

impl ForwardHandle {
    /// Stop accepting new connections.
    pub fn stop(self) {
        self.join.abort();
    }

    /// Actual bound address (useful with port 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

/// Bind `listen` and forward every accepted connection to `service` on the device.
pub async fn start_forward<T: Transport + 'static>(
    connection: Arc<Connection<T>>,
    listen: SocketAddr,
    service: String,
) -> Result<ForwardHandle> {
    let listener = TcpListener::bind(listen).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            AdbError::Transport(format!("port {listen} is already in use ({e})"))
        } else {
            AdbError::Io(e)
        }
    })?;
    let local_addr = listener.local_addr()?;
    let service_clone = service.clone();
    debug!(%local_addr, service = %service, "forwarding listening");

    let join = tokio::spawn(async move {
        loop {
            let (socket, peer) = match listener.accept().await {
                Ok(pair) => pair,
                Err(err) => {
                    warn!("forward accept failed on {local_addr}: {err}");
                    continue;
                }
            };
            debug!(%peer, service = %service_clone, "forward: new connection");
            let conn = Arc::clone(&connection);
            let service = service_clone.clone();
            tokio::spawn(async move {
                if let Err(err) = proxy_one(conn, socket, service).await {
                    debug!(%peer, "forward proxy ended: {err}");
                }
            });
        }
    });

    Ok(ForwardHandle {
        local_addr,
        service,
        join,
    })
}

async fn proxy_one<T: Transport + 'static>(
    connection: Arc<Connection<T>>,
    socket: TcpStream,
    service: String,
) -> Result<()> {
    let stream = connection.open_stream(&service).await?;
    let (inbound, writer) = stream.into_parts();
    let (mut tcp_read, mut tcp_write) = socket.into_split();

    // tcp → adb
    let adb_write = async move {
        let writer = writer;
        let mut buf = vec![0u8; PROXY_BUF];
        loop {
            match tcp_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => writer.write(Bytes::copy_from_slice(&buf[..n])).await?,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(AdbError::Io(err)),
            }
        }
        writer.close().await
    };

    // adb → tcp
    let tcp_write_task = async move {
        let mut inbound = inbound;
        while let Some(chunk) = inbound.recv().await {
            tcp_write.write_all(&chunk).await?;
        }
        Ok::<(), AdbError>(())
    };

    // First leg to finish ends the proxy; the other is dropped.
    tokio::select! {
        res = adb_write => res,
        res = tcp_write_task => res,
    }
}
