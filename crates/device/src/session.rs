//! Host side of one watchdog app session: `HOST_HELLO` → `HELLO_ACK` → frames.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::Result;
use watchdog_protocol::{
    Decoder, FeedResult, FramePacket, HelloAck, HostHello, HostInfo, Message, PROTOCOL_VERSION,
};

use crate::connection::DeviceConnection;

/// TCP connect timeout for one session attempt.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Delay between connection attempts while the app socket starts up.
const RETRY_DELAY: Duration = Duration::from_millis(250);
/// Read buffer for framed app messages.
const READ_BUFFER: usize = 64 * 1024;

/// Result of [`Session::poll`].
pub enum Polled {
    /// A typed message arrived (boxed: `Message` is much larger than the
    /// other variants).
    Message(Box<Message>),
    /// No message arrived before the timeout (the stream is still open).
    Timeout,
    /// The app closed the stream.
    Closed,
}

/// A framed, authenticated (after [`Session::handshake`]) app connection.
pub struct Session {
    stream: TcpStream,
    decoder: Decoder,
    pending: VecDeque<Message>,
}

impl Session {
    /// Connect to a forwarded app socket and send `HOST_HELLO`.
    pub async fn open(addr: SocketAddr, token: &str) -> Result<Self> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                WatchdogError::new(
                    ErrorCode::ConnectionLost,
                    format!("timed out connecting to the forwarded app socket at {addr}"),
                )
            })??;
        stream.set_nodelay(true)?;
        let mut session = Self {
            stream,
            decoder: Decoder::new(),
            pending: VecDeque::new(),
        };
        session
            .send(&Message::HostHello(HostHello {
                protocol_min: PROTOCOL_VERSION,
                protocol_max: PROTOCOL_VERSION,
                host: HostInfo {
                    name: "watchdog".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                session_token: token.to_string(),
            }))
            .await?;
        Ok(session)
    }

    /// Encode and send one message to the app.
    pub async fn send(&mut self, message: &Message) -> Result<()> {
        let frame = message
            .encode_frame()
            .map_err(|e| WatchdogError::new(ErrorCode::ProtocolError, e.to_string()))?;
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Wait for the app's `HELLO_ACK` (or a fatal `ERROR`/`BYE`).
    pub async fn handshake(&mut self, timeout: Duration) -> Result<HelloAck> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(WatchdogError::new(
                    ErrorCode::ConnectionLost,
                    "timed out waiting for HELLO_ACK from the app",
                ));
            }
            match self.next(remaining).await? {
                Some(Message::HelloAck(ack)) => {
                    if ack.protocol_version != PROTOCOL_VERSION {
                        return Err(WatchdogError::new(
                            ErrorCode::ProtocolVersionMismatch,
                            format!(
                                "app negotiated protocol {}, host speaks {}",
                                ack.protocol_version, PROTOCOL_VERSION
                            ),
                        )
                        .with_hint("update the watchdog app to match the host"));
                    }
                    return Ok(ack);
                }
                Some(Message::Error(error)) => {
                    return Err(app_error(&error.code, &error.message, error.hint));
                }
                Some(Message::Bye(bye)) => {
                    return Err(WatchdogError::new(
                        ErrorCode::StreamClosed,
                        format!("app closed the session during handshake: {}", bye.reason),
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(WatchdogError::new(
                        ErrorCode::StreamClosed,
                        "app closed the connection during handshake",
                    ));
                }
            }
        }
    }

    /// Read the next typed message; `None` means the app closed the stream.
    pub async fn next(&mut self, timeout: Duration) -> Result<Option<Message>> {
        match self.poll(timeout).await? {
            Polled::Message(message) => Ok(Some(*message)),
            Polled::Closed => Ok(None),
            Polled::Timeout => Err(WatchdogError::new(
                ErrorCode::ConnectionLost,
                "timed out waiting for a message from the app",
            )),
        }
    }

    /// Wait up to `timeout` for the next message, distinguishing an idle
    /// session from a closed one (used by `watchdog device logs`).
    pub async fn poll(&mut self, timeout: Duration) -> Result<Polled> {
        if let Some(message) = self.pending.pop_front() {
            return Ok(Polled::Message(Box::new(message)));
        }
        let mut buffer = [0u8; READ_BUFFER];
        loop {
            let read = match tokio::time::timeout(timeout, self.stream.read(&mut buffer)).await {
                Ok(read) => read?,
                Err(_) => return Ok(Polled::Timeout),
            };
            if read == 0 {
                return Ok(Polled::Closed);
            }
            match self
                .decoder
                .feed(&buffer[..read])
                .map_err(|e| WatchdogError::new(ErrorCode::ProtocolError, e.to_string()))?
            {
                FeedResult::NeedMore => {}
                FeedResult::Packets(packets) => {
                    for packet in packets {
                        self.pending
                            .push_back(Message::from_packet(packet).map_err(|e| {
                                WatchdogError::new(ErrorCode::ProtocolError, e.to_string())
                            })?);
                    }
                }
            }
            if let Some(message) = self.pending.pop_front() {
                return Ok(Polled::Message(Box::new(message)));
            }
        }
    }

    /// Wait until the app produces a JPEG frame.
    pub async fn wait_for_frame(&mut self, timeout: Duration) -> Result<FramePacket> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(WatchdogError::new(
                    ErrorCode::ConnectionLost,
                    "timed out waiting for the first camera frame",
                )
                .with_hint(
                    "the camera may be in use by another app, or the device screen is locked; \
                     unlock it and check `docs/device-setup.md`",
                ));
            }
            match self.next(remaining).await? {
                Some(Message::Frame(frame)) => return Ok(frame),
                Some(Message::Error(error)) => {
                    return Err(app_error(&error.code, &error.message, error.hint));
                }
                Some(Message::Bye(bye)) => {
                    return Err(WatchdogError::new(
                        ErrorCode::StreamClosed,
                        format!(
                            "app closed the session before sending a frame: {}",
                            bye.reason
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(WatchdogError::new(
                        ErrorCode::StreamClosed,
                        "app closed the session before sending a frame",
                    ));
                }
            }
        }
    }
}

/// A session plus the ADB port-forward that keeps it reachable.
pub struct LiveSession {
    session: Session,
    _forward: watchdog_adb::ForwardHandle,
}

impl LiveSession {
    /// Forward the app's abstract socket, then retry connect+handshake until the
    /// app starts listening or `timeout` elapses. Authentication failures are
    /// returned immediately (retrying cannot fix a wrong token).
    pub async fn open_authenticated(
        connection: &DeviceConnection,
        token: &str,
        timeout: Duration,
        handshake_timeout: Duration,
    ) -> Result<(Self, HelloAck)> {
        let forward = connection
            .forward(
                SocketAddr::from(([127, 0, 0, 1], 0)),
                format!("localabstract:watchdog-{token}"),
            )
            .await?;
        let addr = forward.local_addr();
        let deadline = Instant::now() + timeout;
        let mut last: Option<WatchdogError> = None;
        while Instant::now() < deadline {
            match Session::open(addr, token).await {
                Ok(mut session) => match session.handshake(handshake_timeout).await {
                    Ok(ack) => {
                        return Ok((
                            Self {
                                session,
                                _forward: forward,
                            },
                            ack,
                        ))
                    }
                    Err(error) if is_fatal(&error) => return Err(error),
                    Err(error) => last = Some(error),
                },
                Err(error) => last = Some(error),
            }
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }
        let mut error = last.unwrap_or_else(|| {
            WatchdogError::new(
                ErrorCode::ConnectionLost,
                format!("timed out waiting for the app socket at {addr}"),
            )
        });
        if error.hint.is_none() {
            error = error.with_hint(
                "check that provisioning launched the capture service and granted CAMERA; \
                 unlock the device and see docs/device-setup.md",
            );
        }
        Err(error)
    }

    /// Access the underlying framed session.
    pub fn session(&mut self) -> &mut Session {
        &mut self.session
    }
}

/// Map an app `ERROR` payload onto the shared host error taxonomy.
fn app_error(code: &str, message: &str, hint: Option<String>) -> WatchdogError {
    let error_code = match code {
        "UNAUTHORIZED" => ErrorCode::Unauthorized,
        "PROTOCOL_VERSION_MISMATCH" => ErrorCode::ProtocolVersionMismatch,
        _ => ErrorCode::ProtocolError,
    };
    let mut error = WatchdogError::new(
        error_code,
        format!("app rejected the session: {message} ({code})"),
    );
    if let Some(hint) = hint {
        error = error.with_hint(hint);
    }
    error
}

fn is_fatal(error: &WatchdogError) -> bool {
    matches!(
        error.code,
        ErrorCode::Unauthorized | ErrorCode::ProtocolVersionMismatch
    )
}
