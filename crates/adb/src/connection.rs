//! Connection session: CNXN negotiation, AUTH handshake, and stream routing.
//!
//! After the sequential handshake, a reader task routes packets into per-stream
//! channels and a writer task serializes all outbound packets. Each stream gets
//! a 1-outstanding-WRTE flow-control window (refilled on OKAY), per the ADB spec.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::task::JoinHandle;

use crate::auth::AdbKeyPair;
use crate::error::{AdbError, Result};
use crate::packet::{AuthType, Command, Packet};
use crate::transport::Transport;

/// Default timeout for the whole handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Bounded buffer for inbound WRTE payloads per stream (byte count, not items).
pub const STREAM_BUFFER: usize = 256;
/// Default timeout waiting for an OKAY after OPEN.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of the initial handshake.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Peer banner (e.g. `device::1a2b3c4d:Pixel 7`).
    pub banner: String,
    /// Max payload the peer accepts (their `maxdata`).
    pub maxdata: u32,
}

/// A duplex stream to a device service.
#[derive(Debug)]
pub struct Stream {
    local_id: u32,
    remote_id: u32,
    /// Inbound payloads pushed by the reader task.
    pub inbound: mpsc::Receiver<Bytes>,
    write_permit: Arc<Semaphore>,
    outbound: mpsc::Sender<Outbound>,
}

impl Stream {
    /// Our local id.
    #[must_use]
    pub fn local_id(&self) -> u32 {
        self.local_id
    }

    /// Peer-assigned id.
    #[must_use]
    pub fn remote_id(&self) -> u32 {
        self.remote_id
    }

    /// Send a payload; waits for the per-stream write window (1 in flight).
    pub async fn write(&self, payload: Bytes) -> Result<()> {
        let permit = Arc::clone(&self.write_permit);
        permit.acquire_owned().await.map_err(|_| AdbError::StreamClosed)?;
        self.outbound
            .send(Outbound::Write { local_id: self.local_id, payload })
            .await
            .map_err(|_| AdbError::StreamClosed)
    }

    /// Close our side of the stream (sends CLSE).
    pub async fn close(self) -> Result<()> {
        let _ = self.outbound.send(Outbound::Close { local_id: self.local_id }).await;
        Ok(())
    }
}

/// Messages to the writer task.
#[derive(Debug)]
pub(crate) enum Outbound {
    /// Write payload bytes to an open stream.
    Write { local_id: u32, payload: Bytes },
    /// Close our side of a stream.
    Close { local_id: u32 },
}

/// Interior shared state (std mutex; never held across await).
#[derive(Default)]
pub(crate) struct Shared {
    next_local_id: u32,
    pending_open: HashMap<u32, oneshot::Sender<Result<u32>>>,
    stream_tx: HashMap<u32, mpsc::Sender<Bytes>>,
    streams: HashMap<u32, u32>, // local_id -> remote_id
    write_permits: HashMap<u32, Arc<Semaphore>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            next_local_id: 0x4000,
            pending_open: HashMap::new(),
            stream_tx: HashMap::new(),
            streams: HashMap::new(),
            write_permits: HashMap::new(),
        }
    }

    fn alloc_local_id(&mut self) -> u32 {
        let id = self.next_local_id;
        self.next_local_id = self.next_local_id.wrapping_add(1).max(1);
        id
    }
}

/// An established connection (post-handshake).
#[derive(Debug)]
pub struct Connection<T: Transport> {
    /// Negotiated session info.
    pub info: SessionInfo,
    transport: Arc<tokio::sync::Mutex<T>>,
    outbound: mpsc::Sender<Outbound>,
    shared: Arc<std::sync::Mutex<Shared>>,
    reader: Option<JoinHandle<Result<()>>>,
    writer: Option<JoinHandle<Result<()>>>,
}

impl<T: Transport + 'static> Connection<T> {
    /// Negotiate CNXN and complete AUTH on a fresh transport.
    pub async fn connect(mut transport: T, banner: &str, keys: &AdbKeyPair) -> Result<Self> {
        let info = handshake(&mut transport, banner, keys).await?;
        let transport = Arc::new(tokio::sync::Mutex::new(transport));
        let shared = Arc::new(std::sync::Mutex::new(Shared::new()));
        let (outbound_tx, outbound_rx) = mpsc::channel::<Outbound>(64);

        let writer = tokio::spawn(writer_task(
            Arc::clone(&transport),
            Arc::clone(&shared),
            outbound_rx,
        ));
        let reader = tokio::spawn(reader_task(
            Arc::clone(&transport),
            Arc::clone(&shared),
        ));

        Ok(Self {
            info,
            transport,
            outbound: outbound_tx,
            shared,
            reader: Some(reader),
            writer: Some(writer),
        })
    }

    /// Open a stream to `service` (e.g. `shell:id`, `tcp:8080`, `localabstract:watchdog`).
    pub async fn open_stream(&self, service: &str) -> Result<Stream> {
        // Register everything first so an instant OKAY/CLSE can be routed.
        let (reply_tx, reply_rx) = oneshot::channel();
        let (inbound_tx, inbound_rx) = mpsc::channel(STREAM_BUFFER);
        let permit = Arc::new(Semaphore::new(1));
        let local_id = {
            let mut guard = self.shared.lock().expect("shared state poisoned");
            let id = guard.alloc_local_id();
            guard.pending_open.insert(id, reply_tx);
            guard.stream_tx.insert(id, inbound_tx);
            guard.streams.insert(id, 0);
            guard.write_permits.insert(id, Arc::clone(&permit));
            id
        };

        let packet = Packet::new(Command::Open, local_id, 0, service.as_bytes().to_vec());
        {
            let mut t = self.transport.lock().await;
            t.write_all(&packet.encode()).await?;
        }

        let remote_id = tokio::time::timeout(OPEN_TIMEOUT, reply_rx)
            .await
            .map_err(|_| AdbError::Timeout("OPEN timed out waiting for OKAY".into()))?
            .map_err(|_| AdbError::StreamClosed)??;

        Ok(Stream {
            local_id,
            remote_id,
            inbound: inbound_rx,
            write_permit: permit,
            outbound: self.outbound.clone(),
        })
    }

    /// Abort both tasks (used on device disconnect).
    pub async fn close(mut self) {
        if let Some(task) = self.reader.take() {
            task.abort();
        }
        if let Some(task) = self.writer.take() {
            task.abort();
        }
    }
}

/// Complete the CNXN/AUTH handshake on a fresh transport.
pub async fn handshake<T: Transport>(transport: &mut T, banner: &str, keys: &AdbKeyPair) -> Result<SessionInfo> {
    send_cnxn(transport, banner).await?;

    let mut signature_sent = false;
    let mut public_key_sent = false;
    let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;

    loop {
        let remaining = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .ok_or_else(|| AdbError::Timeout("handshake timed out waiting for CNXN".into()))?;
        let packet = read_packet(transport, remaining).await?;
        match packet.command {
            Command::Cnxn => {
                let version = packet.arg0;
                if version < crate::packet::A_VERSION_MIN {
                    return Err(AdbError::VersionMismatch(format!("peer CNXN version {version:#x}")));
                }
                let peer_banner = parse_banner(&packet.payload)?;
                return Ok(SessionInfo { banner: peer_banner, maxdata: packet.arg1 });
            }
            Command::Auth => match packet.arg0 {
                x if x == AuthType::Token as u32 => {
                    if packet.payload.len() != 20 {
                        return Err(AdbError::Protocol(format!(
                            "AUTH token must be 20 bytes, got {}",
                            packet.payload.len()
                        )));
                    }
                    if !signature_sent {
                        let signature = keys.sign_token(&packet.payload)?;
                        let reply = Packet::new(Command::Auth, AuthType::Signature as u32, 0, Bytes::from(signature));
                        transport.write_all(&reply.encode()).await?;
                        signature_sent = true;
                    } else if !public_key_sent {
                        let reply = Packet::new(
                            Command::Auth,
                            AuthType::RsaPublicKey as u32,
                            0,
                            Bytes::from(keys.public_key_payload()),
                        );
                        transport.write_all(&reply.encode()).await?;
                        public_key_sent = true;
                    } else {
                        return Err(AdbError::Unauthorized);
                    }
                }
                other => {
                    return Err(AdbError::Protocol(format!(
                        "unexpected AUTH arg0={other} during handshake"
                    )));
                }
            },
            Command::Stls => {
                return Err(AdbError::Protocol(
                    "unexpected STLS (STLS must be negotiated before CNXN)".into(),
                ));
            }
            other => {
                return Err(AdbError::Protocol(format!(
                    "unexpected {} during handshake",
                    other.name()
                )));
            }
        }
    }
}

async fn send_cnxn<T: Transport>(transport: &mut T, banner: &str) -> Result<()> {
    let mut payload = banner.as_bytes().to_vec();
    payload.push(0);
    let packet = Packet::new(Command::Cnxn, crate::packet::A_VERSION, crate::packet::DEFAULT_MAXDATA, payload);
    transport.write_all(&packet.encode()).await
}

async fn read_packet<T: Transport>(transport: &mut T, timeout: Duration) -> Result<Packet> {
    use crate::packet::HEADER_LEN;

    let mut header = [0u8; HEADER_LEN];
    tokio::time::timeout(timeout, transport.read_exact(&mut header))
        .await
        .map_err(|_| AdbError::Timeout("timed out waiting for packet header".into()))??;
    let command_id = u32::from_le_bytes(header[0..4].try_into().expect("slice"));
    let arg0 = u32::from_le_bytes(header[4..8].try_into().expect("slice"));
    let arg1 = u32::from_le_bytes(header[8..12].try_into().expect("slice"));
    let len = u32::from_le_bytes(header[12..16].try_into().expect("slice")) as usize;
    let magic = u32::from_le_bytes(header[20..24].try_into().expect("slice"));
    let command = crate::packet::Command::from_u32(command_id)
        .ok_or_else(|| AdbError::Protocol(format!("unknown command 0x{command_id:08x}")))?;
    if magic != command.magic() {
        return Err(AdbError::BadMagic {
            command: command_id,
            magic,
        });
    }
    if len > crate::packet::MAX_PAYLOAD {
        return Err(AdbError::PayloadTooLarge(len));
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        transport.read_exact(&mut payload).await?;
    }
    Ok(Packet::new(command, arg0, arg1, Bytes::from(payload)))
}

fn parse_banner(payload: &[u8]) -> Result<String> {
    let nul = payload.iter().position(|&b| b == 0).unwrap_or(payload.len());
    std::str::from_utf8(&payload[..nul])
        .map(str::to_string)
        .map_err(|e| AdbError::Protocol(format!("banner is not UTF-8: {e}")))
}

/// Writer task: serialize all outbound packets.
async fn writer_task<T: Transport>(
    transport: Arc<tokio::sync::Mutex<T>>,
    shared: Arc<std::sync::Mutex<Shared>>,
    mut outbound: mpsc::Receiver<Outbound>,
) -> Result<()> {
    while let Some(msg) = outbound.recv().await {
        match msg {
            Outbound::Write { local_id, payload } => {
                let remote_id = {
                    let guard = shared.lock().expect("shared state poisoned");
                    guard.streams.get(&local_id).copied().unwrap_or(0)
                };
                if remote_id == 0 {
                    continue; // stream not established (or closed); drop
                }
                let packet = Packet::new(Command::Wrte, local_id, remote_id, payload);
                transport.lock().await.write_all(&packet.encode()).await?;
            }
            Outbound::Close { local_id } => {
                let remote_id = {
                    let guard = shared.lock().expect("shared state poisoned");
                    guard.streams.get(&local_id).copied().unwrap_or(0)
                };
                let packet = Packet::new(Command::Clse, local_id, remote_id, &[][..]);
                transport.lock().await.write_all(&packet.encode()).await?;
            }
        }
    }
    Ok(())
}

/// Reader task: route inbound packets to stream channels and OKAY/CLSE bookkeeping.
async fn reader_task<T: Transport>(
    transport: Arc<tokio::sync::Mutex<T>>,
    shared: Arc<std::sync::Mutex<Shared>>,
) -> Result<()> {
    loop {
        let packet = {
            let mut t = transport.lock().await;
            read_packet(&mut *t, Duration::from_secs(3600)).await?
        };
        match packet.command {
            Command::Okay => {
                let local_id = packet.arg1;
                let remote_id = packet.arg0;
                let mut guard = shared.lock().expect("shared state poisoned");
                guard.streams.insert(local_id, remote_id);
                if let Some(pending) = guard.pending_open.remove(&local_id) {
                    let _ = pending.send(Ok(remote_id));
                } else if let Some(permit) = guard.write_permits.get(&local_id) {
                    permit.add_permits(1);
                }
            }
            Command::Wrte => {
                let local_id = packet.arg1;
                let remote_id = packet.arg0;
                let sender = {
                    let guard = shared.lock().expect("shared state poisoned");
                    guard.stream_tx.get(&local_id).cloned()
                };
                if let Some(sender) = sender {
                    if sender.send(packet.payload).await.is_ok() {
                        let ok = Packet::new(Command::Okay, local_id, remote_id, &[][..]);
                        transport.lock().await.write_all(&ok.encode()).await?;
                    }
                }
            }
            Command::Clse => {
                let local_id = packet.arg1;
                let mut guard = shared.lock().expect("shared state poisoned");
                guard.stream_tx.remove(&local_id);
                guard.streams.remove(&local_id);
                guard.write_permits.remove(&local_id);
                if let Some(pending) = guard.pending_open.remove(&local_id) {
                    let _ = pending.send(Err(AdbError::StreamClosed));
                }
            }
            Command::Open => {
                // Device-initiated services (reverse ports) are unsupported in v1.
                let reply = Packet::new(Command::Clse, 0, packet.arg0, &[][..]);
                transport.lock().await.write_all(&reply.encode()).await?;
            }
            other => {
                tracing::warn!(packet = %other.name(), "ignoring unexpected packet from device");
            }
        }
    }
}
