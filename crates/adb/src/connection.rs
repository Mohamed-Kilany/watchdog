//! Connection session: CNXN negotiation, AUTH handshake, and stream routing.
//!
//! After the sequential handshake the transport is split into independent read
//! and write halves: a reader task routes packets into per-stream channels and
//! a writer task serializes all outbound packets. This matters for commands
//! that consume stdin but produce no output (e.g. a streaming APK install) —
//! with a shared transport mutex the reader would hold the lock while blocked
//! and the writer could never send the input the device is waiting for.
//!
//! Each stream gets a 1-outstanding-WRTE flow-control window (refilled on
//! OKAY), per the ADB spec. Closing a stream closes its semaphore so writers
//! blocked on the window fail with [`AdbError::StreamClosed`] instead of
//! hanging forever.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::task::JoinHandle;

use crate::auth::AdbKeyPair;
use crate::error::{AdbError, Result};
use crate::packet::{AuthType, Command, Packet};
use crate::transport::{Transport, TransportReader, TransportWriter};

/// Default timeout for the whole handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Bounded buffer for inbound WRTE payloads per stream (item count).
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

    /// Split into the inbound receiver and the write half (consumes the stream).
    ///
    /// Used by port forwarding to run the two copy directions as separate tasks.
    pub fn into_parts(self) -> (mpsc::Receiver<Bytes>, StreamWriter) {
        let Self {
            local_id,
            remote_id: _,
            inbound,
            write_permit,
            outbound,
        } = self;
        (
            inbound,
            StreamWriter {
                local_id,
                write_permit,
                outbound,
            },
        )
    }

    /// Send a payload; waits for the per-stream write window (1 in flight).
    pub async fn write(&self, payload: Bytes) -> Result<()> {
        tracing::trace!(
            local_id = self.local_id,
            len = payload.len(),
            "stream write waiting for permit"
        );
        let permit = Arc::clone(&self.write_permit)
            .acquire_owned()
            .await
            .map_err(|_| AdbError::StreamClosed)?;
        tracing::trace!(
            local_id = self.local_id,
            len = payload.len(),
            "stream write permit acquired"
        );
        // Flow control: the permit is restored by the reader task when the
        // device acknowledges the WRTE with OKAY. Forget it here so dropping
        // this future does not return it early.
        permit.forget();
        self.outbound
            .send(Outbound::Write {
                local_id: self.local_id,
                payload,
            })
            .await
            .map_err(|_| AdbError::StreamClosed)
    }

    /// Close our side of the stream (sends CLSE).
    pub async fn close(self) -> Result<()> {
        let _ = self
            .outbound
            .send(Outbound::Close {
                local_id: self.local_id,
            })
            .await;
        Ok(())
    }
}

/// Write half of a stream (see [`Stream::into_parts`]).
#[derive(Debug)]
pub struct StreamWriter {
    local_id: u32,
    write_permit: Arc<Semaphore>,
    outbound: mpsc::Sender<Outbound>,
}

impl StreamWriter {
    /// Send a payload; waits for the per-stream write window (1 in flight).
    pub async fn write(&self, payload: Bytes) -> Result<()> {
        tracing::trace!(
            local_id = self.local_id,
            len = payload.len(),
            "stream write waiting for permit"
        );
        let permit = Arc::clone(&self.write_permit)
            .acquire_owned()
            .await
            .map_err(|_| AdbError::StreamClosed)?;
        tracing::trace!(
            local_id = self.local_id,
            len = payload.len(),
            "stream write permit acquired"
        );
        // Flow control: the permit is restored by the reader task when the
        // device acknowledges the WRTE with OKAY. Forget it here so dropping
        // this future does not return it early.
        permit.forget();
        self.outbound
            .send(Outbound::Write {
                local_id: self.local_id,
                payload,
            })
            .await
            .map_err(|_| AdbError::StreamClosed)
    }

    /// Close our side of the stream (sends CLSE).
    pub async fn close(self) -> Result<()> {
        let _ = self
            .outbound
            .send(Outbound::Close {
                local_id: self.local_id,
            })
            .await;
        Ok(())
    }
}

/// Messages to the writer task.
#[derive(Debug)]
pub(crate) enum Outbound {
    /// Open a stream to a service.
    Open { local_id: u32, service: Bytes },
    /// Write payload bytes to an open stream.
    Write { local_id: u32, payload: Bytes },
    /// Close our side of a stream.
    Close { local_id: u32 },
    /// Acknowledge an inbound WRTE.
    Okay { local_id: u32, remote_id: u32 },
    /// Close a stream without consulting the local map (device-initiated).
    Clse { local_id: u32, remote_id: u32 },
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
pub struct Connection<T: Transport + 'static> {
    /// Negotiated session info.
    pub info: SessionInfo,
    outbound: mpsc::Sender<Outbound>,
    shared: Arc<std::sync::Mutex<Shared>>,
    reader: Option<JoinHandle<Result<()>>>,
    writer: Option<JoinHandle<Result<()>>>,
    _transport: PhantomData<fn() -> T>,
}

impl<T: Transport + 'static> std::fmt::Debug for Connection<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl<T: Transport + 'static> Connection<T> {
    /// Negotiate CNXN and complete AUTH on a fresh transport.
    pub async fn connect(mut transport: T, banner: &str, keys: &AdbKeyPair) -> Result<Self> {
        let info = handshake(&mut transport, banner, keys).await?;
        Ok(Self::start(transport, info))
    }

    fn start(transport: T, info: SessionInfo) -> Self {
        let (reader_half, writer_half) = transport.split();
        let shared = Arc::new(std::sync::Mutex::new(Shared::new()));
        let (outbound_tx, outbound_rx) = mpsc::channel::<Outbound>(64);

        let writer = tokio::spawn(writer_task(
            writer_half,
            Arc::clone(&shared),
            info.maxdata,
            outbound_rx,
        ));
        let reader = tokio::spawn(reader_task(
            reader_half,
            Arc::clone(&shared),
            outbound_tx.clone(),
        ));

        Self {
            info,
            outbound: outbound_tx,
            shared,
            reader: Some(reader),
            writer: Some(writer),
            _transport: PhantomData,
        }
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

        if self
            .outbound
            .send(Outbound::Open {
                local_id,
                service: Bytes::copy_from_slice(service.as_bytes()),
            })
            .await
            .is_err()
        {
            unregister(&self.shared, local_id);
            return Err(AdbError::StreamClosed);
        }

        let remote_id = match tokio::time::timeout(OPEN_TIMEOUT, reply_rx).await {
            Err(_) => {
                unregister(&self.shared, local_id);
                return Err(AdbError::Timeout("OPEN timed out waiting for OKAY".into()));
            }
            Ok(Err(_)) => {
                unregister(&self.shared, local_id);
                return Err(AdbError::StreamClosed);
            }
            Ok(Ok(result)) => result?,
        };

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
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(task) = self.reader.take() {
            task.abort();
        }
        if let Some(task) = self.writer.take() {
            task.abort();
        }
        close_all(&self.shared);
    }
}

impl<T: Transport + 'static> Drop for Connection<T> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Remove a stream registration that never became usable.
fn unregister(shared: &std::sync::Mutex<Shared>, local_id: u32) {
    let mut guard = shared.lock().expect("shared state poisoned");
    guard.pending_open.remove(&local_id);
    guard.stream_tx.remove(&local_id);
    guard.streams.remove(&local_id);
    if let Some(permit) = guard.write_permits.remove(&local_id) {
        permit.close();
    }
}

/// Fail every waiter and wake blocked stream writes after the session ends.
fn close_all(shared: &std::sync::Mutex<Shared>) {
    let mut guard = shared.lock().expect("shared state poisoned");
    for (_, permit) in guard.write_permits.drain() {
        permit.close();
    }
    guard.stream_tx.clear();
    guard.streams.clear();
    for (_, pending) in guard.pending_open.drain() {
        let _ = pending.send(Err(AdbError::StreamClosed));
    }
}

/// Complete the CNXN/AUTH handshake on a fresh transport.
pub async fn handshake<T: Transport + 'static>(
    transport: &mut T,
    banner: &str,
    keys: &AdbKeyPair,
) -> Result<SessionInfo> {
    send_cnxn(transport, banner).await?;
    let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
    let mut state = HandshakeState::default();

    loop {
        let packet = read_packet(transport, remaining(deadline)?).await?;
        match process_handshake_packet(transport, packet, keys, &mut state).await? {
            HandshakeStep::Continue => {}
            HandshakeStep::Done(info) => return Ok(info),
            HandshakeStep::Stls => {
                return Err(AdbError::Protocol(
                    "unexpected STLS (TLS upgrade requires a LAN transport)".into(),
                ));
            }
        }
    }
}

/// Complete the CNXN handshake on a wireless-debugging connect port.
///
/// The client sends CNXN first; a TLS-only device answers `A_STLS`. We reply
/// with our own `A_STLS`, complete the TLS 1.3 handshake (our ADB certificate
/// is the client cert), and then keep reading: the device sends CNXN inside
/// TLS. Plain-TCP devices (e.g. an emulator) work unchanged.
pub async fn handshake_lan(
    transport: &mut crate::stls::LanTransport,
    banner: &str,
    keys: &AdbKeyPair,
    cert_der: &[u8],
    key_der: &[u8],
) -> Result<SessionInfo> {
    send_cnxn(transport, banner).await?;
    let deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
    let mut state = HandshakeState::default();

    loop {
        let packet = read_packet(transport, remaining(deadline)?).await?;
        if packet.command == Command::Stls {
            let reply = Packet::new(Command::Stls, crate::packet::A_STLS_VERSION, 0, &[][..]);
            transport.write_all(&reply.encode()).await?;
            transport.upgrade(cert_der, key_der, "adb").await?;
            continue;
        }
        match process_handshake_packet(transport, packet, keys, &mut state).await? {
            HandshakeStep::Continue => {}
            HandshakeStep::Done(info) => return Ok(info),
            HandshakeStep::Stls => unreachable!("STLS handled above"),
        }
    }
}

impl Connection<crate::stls::LanTransport> {
    /// Connect to a wireless-debugging connect port (STLS auto-upgrade).
    pub async fn connect_lan(
        transport: crate::tcp::TcpTransport,
        banner: &str,
        keys: &AdbKeyPair,
    ) -> Result<Self> {
        let cert_der = keys.certificate_der()?;
        let key_der = keys.private_key_der()?;
        let mut transport = crate::stls::LanTransport::plain(transport);
        let info = handshake_lan(&mut transport, banner, keys, &cert_der, &key_der).await?;
        Ok(Self::start(transport, info))
    }
}

/// AUTH bookkeeping shared by the plain and LAN handshakes.
#[derive(Default)]
struct HandshakeState {
    signature_sent: bool,
    public_key_sent: bool,
}

/// Outcome of one handshake packet.
enum HandshakeStep {
    /// Keep reading.
    Continue,
    /// Peer offered STLS.
    Stls,
    /// CNXN received.
    Done(SessionInfo),
}

fn remaining(deadline: tokio::time::Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(tokio::time::Instant::now())
        .ok_or_else(|| AdbError::Timeout("handshake timed out waiting for CNXN".into()))
}

async fn process_handshake_packet<T: Transport + 'static>(
    transport: &mut T,
    packet: Packet,
    keys: &AdbKeyPair,
    state: &mut HandshakeState,
) -> Result<HandshakeStep> {
    match packet.command {
        Command::Cnxn => {
            let version = packet.arg0;
            if version < crate::packet::A_VERSION_MIN {
                return Err(AdbError::VersionMismatch(format!(
                    "peer CNXN version {version:#x}"
                )));
            }
            let peer_banner = parse_banner(&packet.payload)?;
            Ok(HandshakeStep::Done(SessionInfo {
                banner: peer_banner,
                maxdata: packet.arg1,
            }))
        }
        Command::Auth => match packet.arg0 {
            x if x == AuthType::Token as u32 => {
                if packet.payload.len() != 20 {
                    return Err(AdbError::Protocol(format!(
                        "AUTH token must be 20 bytes, got {}",
                        packet.payload.len()
                    )));
                }
                if !state.signature_sent {
                    let signature = keys.sign_token(&packet.payload)?;
                    let reply = Packet::new(
                        Command::Auth,
                        AuthType::Signature as u32,
                        0,
                        Bytes::from(signature),
                    );
                    transport.write_all(&reply.encode()).await?;
                    state.signature_sent = true;
                } else if !state.public_key_sent {
                    let reply = Packet::new(
                        Command::Auth,
                        AuthType::RsaPublicKey as u32,
                        0,
                        Bytes::from(keys.public_key_payload()),
                    );
                    transport.write_all(&reply.encode()).await?;
                    state.public_key_sent = true;
                } else {
                    return Err(AdbError::Unauthorized);
                }
                Ok(HandshakeStep::Continue)
            }
            other => Err(AdbError::Protocol(format!(
                "unexpected AUTH arg0={other} during handshake"
            ))),
        },
        Command::Stls => Ok(HandshakeStep::Stls),
        other => Err(AdbError::Protocol(format!(
            "unexpected {} during handshake",
            other.name()
        ))),
    }
}

async fn send_cnxn<T: Transport + 'static>(transport: &mut T, banner: &str) -> Result<()> {
    let mut payload = banner.as_bytes().to_vec();
    payload.push(0);
    let packet = Packet::new(
        Command::Cnxn,
        crate::packet::A_VERSION,
        crate::packet::DEFAULT_MAXDATA,
        payload,
    );
    transport.write_all(&packet.encode()).await
}

async fn read_packet<R: TransportReader + ?Sized>(
    reader: &mut R,
    timeout: Duration,
) -> Result<Packet> {
    use crate::packet::HEADER_LEN;

    let mut header = [0u8; HEADER_LEN];
    tokio::time::timeout(timeout, reader.read_exact(&mut header))
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
        reader.read_exact(&mut payload).await?;
    }
    Ok(Packet::new(command, arg0, arg1, Bytes::from(payload)))
}

fn parse_banner(payload: &[u8]) -> Result<String> {
    let nul = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    std::str::from_utf8(&payload[..nul])
        .map(str::to_string)
        .map_err(|e| AdbError::Protocol(format!("banner is not UTF-8: {e}")))
}

/// Writer task: serialize all outbound packets.
async fn writer_task<W: TransportWriter + 'static>(
    mut writer: W,
    shared: Arc<std::sync::Mutex<Shared>>,
    maxdata: u32,
    mut outbound: mpsc::Receiver<Outbound>,
) -> Result<()> {
    let result = write_loop(&mut writer, &shared, maxdata, &mut outbound).await;
    close_all(&shared);
    result
}

async fn write_loop<W: TransportWriter + ?Sized>(
    writer: &mut W,
    shared: &std::sync::Mutex<Shared>,
    maxdata: u32,
    outbound: &mut mpsc::Receiver<Outbound>,
) -> Result<()> {
    while let Some(msg) = outbound.recv().await {
        let packet = match msg {
            Outbound::Open { local_id, service } => {
                tracing::debug!(local_id, service = %String::from_utf8_lossy(service.as_ref()), "ADB OPEN");
                Packet::new(Command::Open, local_id, 0, service)
            }
            Outbound::Write { local_id, payload } => {
                let remote_id = {
                    let guard = shared.lock().expect("shared state poisoned");
                    guard.streams.get(&local_id).copied().unwrap_or(0)
                };
                if remote_id == 0 {
                    tracing::debug!(local_id, "dropping WRTE for closed stream");
                    continue;
                }
                if maxdata > 0 && payload.len() > maxdata as usize {
                    tracing::warn!(
                        local_id,
                        len = payload.len(),
                        maxdata,
                        "WRTE exceeds the peer's advertised maxdata"
                    );
                }
                tracing::trace!(local_id, remote_id, len = payload.len(), "ADB WRTE");
                Packet::new(Command::Wrte, local_id, remote_id, payload)
            }
            Outbound::Close { local_id } => {
                let remote_id = {
                    let guard = shared.lock().expect("shared state poisoned");
                    guard.streams.get(&local_id).copied().unwrap_or(0)
                };
                if remote_id == 0 {
                    continue;
                }
                tracing::debug!(local_id, remote_id, "ADB CLSE");
                Packet::new(Command::Clse, local_id, remote_id, &[][..])
            }
            Outbound::Okay {
                local_id,
                remote_id,
            } => Packet::new(Command::Okay, local_id, remote_id, &[][..]),
            Outbound::Clse {
                local_id,
                remote_id,
            } => Packet::new(Command::Clse, local_id, remote_id, &[][..]),
        };
        writer.write_all(&packet.encode()).await?;
        tracing::trace!(
            command = %packet.command.name(),
            arg0 = packet.arg0,
            len = packet.payload.len(),
            "ADB TX flushed"
        );
    }
    Ok(())
}

/// Reader task: route inbound packets to stream channels and OKAY/CLSE bookkeeping.
async fn reader_task<R: TransportReader + 'static>(
    mut reader: R,
    shared: Arc<std::sync::Mutex<Shared>>,
    outbound: mpsc::Sender<Outbound>,
) -> Result<()> {
    let result = read_loop(&mut reader, &shared, &outbound).await;
    close_all(&shared);
    result
}

async fn read_loop<R: TransportReader + ?Sized>(
    reader: &mut R,
    shared: &std::sync::Mutex<Shared>,
    outbound: &mpsc::Sender<Outbound>,
) -> Result<()> {
    loop {
        let packet = read_packet(reader, Duration::from_secs(3600)).await?;
        tracing::trace!(
            command = %packet.command.name(),
            arg0 = packet.arg0,
            arg1 = packet.arg1,
            len = packet.payload.len(),
            "ADB RX"
        );
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
                    tracing::trace!(local_id, remote_id, "ADB OKAY permit returned");
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
                        outbound
                            .send(Outbound::Okay {
                                local_id,
                                remote_id,
                            })
                            .await
                            .map_err(|_| AdbError::StreamClosed)?;
                    } else {
                        unregister(shared, local_id);
                        let _ = outbound
                            .send(Outbound::Clse {
                                local_id,
                                remote_id,
                            })
                            .await;
                    }
                } else {
                    tracing::debug!(local_id, remote_id, "WRTE for unknown stream");
                    let _ = outbound
                        .send(Outbound::Clse {
                            local_id: 0,
                            remote_id,
                        })
                        .await;
                }
            }
            Command::Clse => {
                let local_id = packet.arg1;
                let mut guard = shared.lock().expect("shared state poisoned");
                guard.stream_tx.remove(&local_id);
                guard.streams.remove(&local_id);
                if let Some(permit) = guard.write_permits.remove(&local_id) {
                    permit.close();
                }
                if let Some(pending) = guard.pending_open.remove(&local_id) {
                    let _ = pending.send(Err(AdbError::StreamClosed));
                }
            }
            Command::Open => {
                // Device-initiated services (reverse ports) are unsupported in v1.
                let _ = outbound
                    .send(Outbound::Clse {
                        local_id: 0,
                        remote_id: packet.arg0,
                    })
                    .await;
            }
            other => {
                tracing::warn!(packet = %other.name(), "ignoring unexpected packet from device");
            }
        }
    }
}
