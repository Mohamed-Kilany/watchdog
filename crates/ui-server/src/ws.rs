//! Minimal RFC 6455 WebSocket server for the UI real-time channel (M6-T02).
//!
//! The UI connects to `/ws` for merged state snapshots and app events. Commands
//! go through REST (`POST /api/command`) as the task requires, but the channel
//! also accepts `{"type":"command",...}` messages so tests and future clients
//! can use one connection.
//!
//! Only the pieces a browser needs are implemented: the server handshake,
//! masked client frames, text/binary/continuation frames, ping/pong and close.
//! Extensions and compression are rejected, and message size is bounded.

use std::io;

use base64::Engine as _;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;

use watchdog_device::hub::{FrameHub, HubUpdate};
use watchdog_protocol::{CommandMessage, PROTOCOL_VERSION};

use crate::{result_envelope, state_envelope, waiting_state_envelope};

/// Fixed GUID from RFC 6455 section 1.3.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest message accepted from a client.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Largest chunk read from the socket at once.
const READ_CHUNK: usize = 8192;

const OP_CONTINUATION: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// Complete the server handshake and run the channel until the client leaves.
pub(crate) async fn upgrade(mut stream: TcpStream, key: &str, hub: &FrameHub) -> io::Result<()> {
    let accept = accept_key(key);
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await?;
    stream.flush().await?;
    serve(stream, hub).await
}

/// Compute `Sec-WebSocket-Accept` for a client key.
fn accept_key(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.trim().as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

async fn serve(stream: TcpStream, hub: &FrameHub) -> io::Result<()> {
    let (read_half, mut writer) = stream.into_split();
    let mut reader = WsReader::new(read_half, MAX_MESSAGE_BYTES);
    let mut updates = hub.subscribe_updates();

    write_json(
        &mut writer,
        &serde_json::json!({ "type": "hello", "protocol": PROTOCOL_VERSION }),
    )
    .await?;
    write_snapshot(&mut writer, hub).await?;
    // A client that reconnects after the app session ended would otherwise wait
    // forever for a `Closed` update it missed; tell it immediately.
    if hub.stats().closed {
        write_closed(&mut writer).await?;
        return Ok(());
    }

    let mut fragments: Vec<u8> = Vec::new();
    let mut fragment_opcode = 0u8;

    loop {
        tokio::select! {
            // Read first: a completed `read` must never be dropped in favour of
            // an update, or the bytes would be lost. When the read is pending,
            // cancelling it is safe because `WsReader` keeps its buffer.
            biased;
            frame = reader.next_frame() => {
                let Some(frame) = frame? else { break; };
                match frame.opcode {
                    OP_TEXT | OP_BINARY => {
                        if frame.fin {
                            if frame.opcode == OP_TEXT {
                                handle_client_text(&mut writer, hub, &frame.payload).await?;
                            }
                        } else {
                            fragments.clear();
                            fragments.extend_from_slice(&frame.payload);
                            fragment_opcode = frame.opcode;
                        }
                    }
                    OP_CONTINUATION => {
                        if fragment_opcode == 0 {
                            return Err(invalid_data("unexpected websocket continuation frame"));
                        }
                        fragments.extend_from_slice(&frame.payload);
                        if fragments.len() > MAX_MESSAGE_BYTES {
                            return Err(invalid_data("websocket message exceeds the size limit"));
                        }
                        if frame.fin {
                            if fragment_opcode == OP_TEXT {
                                handle_client_text(&mut writer, hub, &fragments).await?;
                            }
                            fragments.clear();
                            fragment_opcode = 0;
                        }
                    }
                    OP_PING => write_frame(&mut writer, OP_PONG, &frame.payload).await?,
                    OP_PONG => {}
                    OP_CLOSE => {
                        let _ = write_frame(&mut writer, OP_CLOSE, &frame.payload).await;
                        break;
                    }
                    _ => return Err(invalid_data("unsupported websocket opcode")),
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(HubUpdate::State(state)) => {
                        write_json(&mut writer, &state_envelope(&state)).await?;
                    }
                    Ok(HubUpdate::Event(event)) => {
                        let mut value = serde_json::to_value(&event).map_err(io::Error::other)?;
                        if let serde_json::Value::Object(ref mut map) = value {
                            map.insert(
                                "type".to_string(),
                                serde_json::Value::String("event".to_string()),
                            );
                        }
                        write_json(&mut writer, &value).await?;
                    }
                    Ok(HubUpdate::Closed) => {
                        write_closed(&mut writer).await?;
                        break;
                    }
                    // A slow client missed updates; a fresh snapshot converges
                    // it without replaying every intermediate delta.
                    Err(RecvError::Lagged(_)) => write_snapshot(&mut writer, hub).await?,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

async fn write_snapshot<W>(writer: &mut W, hub: &FrameHub) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let value = match hub.state() {
        Some(state) => state_envelope(&state),
        None => waiting_state_envelope(),
    };
    write_json(writer, &value).await
}

/// Tell the client that the app session ended and close the socket.
async fn write_closed<W>(writer: &mut W) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_json(
        writer,
        &serde_json::json!({
            "type": "closed",
            "reason": "the app session closed",
        }),
    )
    .await?;
    let _ = write_frame(writer, OP_CLOSE, &[]).await;
    Ok(())
}

async fn handle_client_text<W>(writer: &mut W, hub: &FrameHub, payload: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message: ClientMessage = match serde_json::from_slice(payload) {
        Ok(message) => message,
        Err(error) => {
            return write_error(
                writer,
                "INVALID_REQUEST",
                &format!("invalid message: {error}"),
            )
            .await
        }
    };
    match message {
        ClientMessage::Command {
            id,
            command,
            params,
        } => {
            if id.is_empty() {
                return write_error(writer, "INVALID_REQUEST", "command id must not be empty")
                    .await;
            }
            match hub
                .command(CommandMessage {
                    id,
                    command,
                    params,
                })
                .await
            {
                Ok(result) => write_json(writer, &result_envelope(&result)).await,
                Err(error) => write_error(writer, error.code.as_str(), &error.message).await,
            }
        }
        ClientMessage::GetState => write_snapshot(writer, hub).await,
        ClientMessage::Ping => write_json(writer, &serde_json::json!({ "type": "pong" })).await,
    }
}

/// Messages a client may send on `/ws`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    /// Forward a command to the app.
    Command {
        /// Client-chosen correlation id.
        id: String,
        /// Command name (`set_controls`, `capture_still`, ...).
        command: String,
        /// Command parameters.
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Ask for a fresh snapshot.
    GetState,
    /// Application-level liveness check.
    Ping,
}

async fn write_error<W>(writer: &mut W, code: &str, message: &str) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    write_json(
        writer,
        &serde_json::json!({ "type": "error", "code": code, "message": message }),
    )
    .await
}

async fn write_json<W>(writer: &mut W, value: &serde_json::Value) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let text = serde_json::to_string(value).map_err(io::Error::other)?;
    write_frame(writer, OP_TEXT, text.as_bytes()).await
}

/// Write one unmasked, unfragmented server frame.
async fn write_frame<W>(writer: &mut W, opcode: u8, payload: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut header = Vec::with_capacity(10);
    header.push(0x80 | opcode);
    match payload.len() {
        length if length < 126 => header.push(length as u8),
        length if length <= usize::from(u16::MAX) => {
            header.push(126);
            header.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            header.push(127);
            header.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    writer.write_all(&header).await?;
    writer.write_all(payload).await?;
    writer.flush().await
}

/// One decoded client frame.
struct WsFrame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

/// Buffered, cancellation-safe frame reader.
///
/// `read_exact` is not cancellation safe, and the channel loop uses
/// [`tokio::select!`] to interleave reads with outgoing updates. Reading into a
/// persistent buffer with `read` (which is cancellation safe) keeps partial
/// frames intact when the update branch wins.
struct WsReader<R> {
    reader: R,
    buffer: Vec<u8>,
    max_payload: usize,
}

impl<R> WsReader<R>
where
    R: AsyncRead + Unpin,
{
    fn new(reader: R, max_payload: usize) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            max_payload,
        }
    }

    async fn next_frame(&mut self) -> io::Result<Option<WsFrame>> {
        loop {
            if let Some(frame) = self.try_parse()? {
                return Ok(Some(frame));
            }
            let mut chunk = [0u8; READ_CHUNK];
            let read = self.reader.read(&mut chunk).await?;
            if read == 0 {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "websocket frame truncated",
                    ))
                };
            }
            self.buffer.extend_from_slice(&chunk[..read]);
            if self.buffer.len() > self.max_payload + 14 {
                return Err(invalid_data("websocket frame exceeds the size limit"));
            }
        }
    }

    fn try_parse(&mut self) -> io::Result<Option<WsFrame>> {
        let Some(header) = self.buffer.get(..2) else {
            return Ok(None);
        };
        let fin = header[0] & 0x80 != 0;
        let rsv = header[0] & 0x70;
        let opcode = header[0] & 0x0f;
        let masked = header[1] & 0x80 != 0;
        let marker = header[1] & 0x7f;
        if rsv != 0 {
            return Err(invalid_data("websocket extensions are not supported"));
        }
        if !matches!(
            opcode,
            OP_CONTINUATION | OP_TEXT | OP_BINARY | OP_CLOSE | OP_PING | OP_PONG
        ) {
            return Err(invalid_data("unsupported websocket opcode"));
        }
        let (length, offset) = match marker {
            126 => {
                let Some(bytes) = self.buffer.get(2..4) else {
                    return Ok(None);
                };
                (usize::from(u16::from_be_bytes([bytes[0], bytes[1]])), 4)
            }
            127 => {
                let Some(bytes) = self.buffer.get(2..10) else {
                    return Ok(None);
                };
                let mut array = [0u8; 8];
                array.copy_from_slice(bytes);
                let length = usize::try_from(u64::from_be_bytes(array))
                    .map_err(|_| invalid_data("websocket frame too large"))?;
                (length, 10)
            }
            value => (usize::from(value), 2),
        };
        if length > self.max_payload {
            return Err(invalid_data("websocket frame exceeds the size limit"));
        }
        if opcode & 0x08 != 0 && (!fin || length > 125) {
            return Err(invalid_data("invalid websocket control frame"));
        }
        if !masked {
            return Err(invalid_data("client websocket frames must be masked"));
        }
        let Some(mask_bytes) = self.buffer.get(offset..offset + 4) else {
            return Ok(None);
        };
        let mut mask = [0u8; 4];
        mask.copy_from_slice(mask_bytes);
        let total = offset + 4 + length;
        if self.buffer.len() < total {
            return Ok(None);
        }
        let mut payload = self.buffer[offset + 4..total].to_vec();
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
        self.buffer.drain(..total);
        Ok(Some(WsFrame {
            fin,
            opcode,
            payload,
        }))
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    fn masked_frame(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [0x12, 0x34, 0x56, 0x78];
        let mut frame = Vec::new();
        frame.push(if fin { 0x80 | opcode } else { opcode });
        frame.push(0x80 | payload.len() as u8);
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        frame
    }

    #[test]
    fn accept_key_matches_the_rfc_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[tokio::test]
    async fn reads_a_masked_text_frame() {
        let (mut client, server) = tokio::io::duplex(1024);
        let frame = masked_frame(OP_TEXT, true, b"hello");
        client.write_all(&frame).await.expect("write");
        drop(client);
        let mut reader = WsReader::new(server, MAX_MESSAGE_BYTES);
        let frame = reader.next_frame().await.expect("read").expect("a frame");
        assert!(frame.fin);
        assert_eq!(frame.opcode, OP_TEXT);
        assert_eq!(frame.payload, b"hello");
        assert!(reader.next_frame().await.expect("eof").is_none());
    }

    #[tokio::test]
    async fn reads_a_frame_split_across_reads() {
        let (mut client, server) = tokio::io::duplex(1024);
        let frame = masked_frame(OP_TEXT, true, b"split payload");
        client.write_all(&frame[..3]).await.expect("first fragment");
        let mut reader = WsReader::new(server, MAX_MESSAGE_BYTES);
        // The first read cannot complete a frame; the second does.
        let read = tokio::time::timeout(std::time::Duration::from_millis(10), reader.next_frame());
        assert!(read.await.is_err(), "frame should not complete yet");
        client
            .write_all(&frame[3..])
            .await
            .expect("second fragment");
        let frame = reader.next_frame().await.expect("read").expect("a frame");
        assert_eq!(frame.payload, b"split payload");
    }

    #[tokio::test]
    async fn rejects_unmasked_client_frames() {
        let (mut client, server) = tokio::io::duplex(1024);
        client
            .write_all(&[0x81, 0x05, b'h', b'e', b'l', b'l', b'o'])
            .await
            .expect("write");
        let mut reader = WsReader::new(server, MAX_MESSAGE_BYTES);
        let error = reader.next_frame().await.expect_err("unmasked frame");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn writes_small_and_large_server_frames() {
        let (mut client, mut writer): (DuplexStream, DuplexStream) = tokio::io::duplex(4096);
        write_frame(&mut writer, OP_TEXT, b"small")
            .await
            .expect("small");
        write_frame(&mut writer, OP_TEXT, &[b'x'; 200])
            .await
            .expect("large");
        let mut header = [0u8; 2];
        client.read_exact(&mut header).await.expect("small header");
        assert_eq!(header, [0x81, 5]);
        let mut payload = [0u8; 5];
        client.read_exact(&mut payload).await.expect("small body");
        assert_eq!(&payload, b"small");
        client.read_exact(&mut header).await.expect("large header");
        assert_eq!(header, [0x81, 126]);
        let mut length = [0u8; 2];
        client.read_exact(&mut length).await.expect("large length");
        assert_eq!(u16::from_be_bytes(length), 200);
        let mut body = [0u8; 200];
        client.read_exact(&mut body).await.expect("large body");
        assert_eq!(body, [b'x'; 200]);
    }
}
