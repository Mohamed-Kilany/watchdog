//! Embedded web UI server.
//!
//! M3-T15 ships the minimal test page: a live MJPEG view of the phone camera
//! plus the rolling stream stats from the device frame hub. The full UI
//! replaces it in M6, reusing the same port and bearer-token model.

#![warn(missing_docs)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::Result;
use watchdog_device::hub::FrameHub;

/// Embedded test page served at `GET /`.
const TEST_PAGE: &str = include_str!("test_page.html");
/// Longest request head accepted before returning `400 Bad Request`.
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// How long a client has to send its request head.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Idle check interval while a viewer waits for frames.
const STREAM_TICK: Duration = Duration::from_secs(1);
/// Multipart boundary used by the MJPEG stream.
const MJPEG_BOUNDARY: &str = "watchdogframe";
/// Terminator of an HTTP request head.
const HEAD_END: &[u8] = b"\r\n\r\n";

/// Options for [`serve_test_page`].
#[derive(Debug, Clone)]
pub struct TestPageOptions {
    /// Address the UI port binds (for example `127.0.0.1:7700`).
    pub bind: SocketAddr,
    /// Bearer token required for every request when set. Clients pass it as
    /// `?token=...` or `Authorization: Bearer ...`.
    pub token: Option<String>,
}

/// Serve the test page, the MJPEG stream and the stats API until the process
/// exits.
///
/// Each client is handled in its own task; a slow or stalled browser only
/// affects its own connection, never the frame hub.
pub async fn serve_test_page(options: TestPageOptions, hub: Arc<FrameHub>) -> Result<()> {
    let listener = TcpListener::bind(options.bind).await.map_err(|error| {
        WatchdogError::new(
            ErrorCode::PortInUse,
            format!("cannot bind the web UI at {}: {error}", options.bind),
        )
        .with_hint("choose another port with --ui-port or stop the process using it")
    })?;
    tracing::info!(address = %options.bind, "test page listening");
    let token = Arc::new(options.token);
    loop {
        let (stream, peer) = listener.accept().await?;
        let hub = Arc::clone(&hub);
        let token = Arc::clone(&token);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, &hub, token.as_deref()).await {
                tracing::debug!(%peer, %error, "UI connection ended");
            }
        });
    }
}

/// One parsed HTTP/1.1 request head.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.as_str() == name)
            .map(|(_, value)| value.as_str())
    }

    fn path(&self) -> &str {
        self.target
            .split_once('?')
            .map_or(self.target.as_str(), |(path, _)| path)
    }

    fn query(&self, key: &str) -> Option<String> {
        let (_, query) = self.target.split_once('?')?;
        query.split('&').find_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name == key).then(|| percent_decode(value))
        })
    }
}

/// Decode `%XX` escapes in a query value (the test page uses
/// `encodeURIComponent`, so arbitrary bearer tokens survive the round trip).
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high << 4 | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    hub: &FrameHub,
    token: Option<&str>,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await? else {
        return write_response(
            &mut stream,
            "400 Bad Request",
            &[("Content-Type", "text/plain; charset=utf-8")],
            b"bad request\n",
        )
        .await;
    };
    if !authorized(&request, token) {
        return write_response(
            &mut stream,
            "401 Unauthorized",
            &[
                ("Content-Type", "text/plain; charset=utf-8"),
                ("WWW-Authenticate", "Bearer"),
            ],
            b"unauthorized\n",
        )
        .await;
    }
    if request.method != "GET" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            &[
                ("Allow", "GET"),
                ("Content-Type", "text/plain; charset=utf-8"),
            ],
            b"method not allowed\n",
        )
        .await;
    }
    match request.path() {
        "/" => {
            write_response(
                &mut stream,
                "200 OK",
                &[("Content-Type", "text/html; charset=utf-8")],
                TEST_PAGE.as_bytes(),
            )
            .await
        }
        "/api/stats" => {
            let body = serde_json::to_vec(&hub.stats()).map_err(std::io::Error::other)?;
            write_response(
                &mut stream,
                "200 OK",
                &[("Content-Type", "application/json")],
                &body,
            )
            .await
        }
        "/stream.mjpg" => stream_mjpeg(stream, hub).await,
        "/healthz" => {
            write_response(
                &mut stream,
                "200 OK",
                &[("Content-Type", "text/plain; charset=utf-8")],
                b"ok\n",
            )
            .await
        }
        _ => {
            write_response(
                &mut stream,
                "404 Not Found",
                &[("Content-Type", "text/plain; charset=utf-8")],
                b"not found\n",
            )
            .await
        }
    }
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = match tokio::time::timeout(REQUEST_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(read) => read?,
            Err(_) => return Ok(None),
        };
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer
            .windows(HEAD_END.len())
            .any(|window| window == HEAD_END)
        {
            break;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Ok(None);
        }
    }
    let head = String::from_utf8_lossy(&buffer);
    Ok(parse_request(&head))
}

fn parse_request(head: &str) -> Option<Request> {
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let target = request_line.next()?.to_string();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    Some(Request {
        method,
        target,
        headers,
    })
}

fn authorized(request: &Request, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    if let Some(value) = request.header("authorization") {
        let bearer = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "));
        if bearer.is_some_and(|given| constant_time_eq(given.trim(), expected)) {
            return true;
        }
    }
    request
        .query("token")
        .is_some_and(|given| constant_time_eq(&given, expected))
}

/// Compare two secrets without an early exit on the first differing byte.
fn constant_time_eq(given: &str, expected: &str) -> bool {
    let given = given.as_bytes();
    let expected = expected.as_bytes();
    if given.len() != expected.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in given.iter().zip(expected) {
        difference |= left ^ right;
    }
    difference == 0
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> std::io::Result<()> {
    let length = body.len();
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {length}\r\nCache-Control: no-store\r\nConnection: close\r\n"
    );
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

async fn stream_mjpeg(mut stream: TcpStream, hub: &FrameHub) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary={MJPEG_BOUNDARY}\r\n\
         Cache-Control: no-store\r\nPragma: no-cache\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes()).await?;
    let mut frames = hub.subscribe();
    loop {
        match tokio::time::timeout(STREAM_TICK, frames.recv()).await {
            Ok(Ok(frame)) => {
                let length = frame.jpeg.len();
                let part = format!(
                    "\r\n--{MJPEG_BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {length}\r\n\r\n"
                );
                if stream.write_all(part.as_bytes()).await.is_err() {
                    break;
                }
                if stream.write_all(&frame.jpeg).await.is_err() {
                    break;
                }
            }
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => break,
            Err(_) => {
                if hub.stats().closed {
                    break;
                }
            }
        }
    }
    let _ = stream.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(target: &str) -> Request {
        parse_request(&format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n"))
            .expect("valid request")
    }

    #[test]
    fn parses_method_target_and_headers() {
        let parsed = request("/api/stats?token=abc");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.path(), "/api/stats");
        assert_eq!(parsed.query("token"), Some("abc".to_string()));
        assert_eq!(parsed.header("host"), Some("localhost"));
    }

    #[test]
    fn decodes_percent_escapes_in_query_values() {
        assert_eq!(
            request("/?token=a%2Bb%20c").query("token"),
            Some("a+b c".to_string())
        );
        assert_eq!(
            request("/?token=100%").query("token"),
            Some("100%".to_string())
        );
    }

    #[test]
    fn rejects_a_request_without_a_request_line() {
        assert!(parse_request("").is_none());
        assert!(parse_request("\r\n").is_none());
    }

    #[test]
    fn accepts_query_and_bearer_tokens() {
        let expected = Some("0123456789abcdef0123456789abcdef".to_string());
        assert!(authorized(
            &request("/?token=0123456789abcdef0123456789abcdef"),
            expected.as_deref()
        ));
        let bearer = parse_request(
            "GET / HTTP/1.1\r\nAuthorization: Bearer 0123456789abcdef0123456789abcdef\r\n\r\n",
        )
        .expect("valid request");
        assert!(authorized(&bearer, expected.as_deref()));
    }

    #[test]
    fn rejects_wrong_or_missing_tokens() {
        let expected = Some("0123456789abcdef0123456789abcdef".to_string());
        assert!(!authorized(&request("/"), expected.as_deref()));
        assert!(!authorized(&request("/?token=wrong"), expected.as_deref()));
        assert!(!authorized(
            &request("/?token=0123456789abcdef0123456789abcde"),
            expected.as_deref()
        ));
    }

    #[test]
    fn no_token_configured_allows_everything() {
        assert!(authorized(&request("/"), None));
    }

    #[test]
    fn constant_time_compare_matches_only_equal_values() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
    }

    #[test]
    fn test_page_uses_the_stream_and_stats_endpoints() {
        assert!(TEST_PAGE.contains("/stream.mjpg"));
        assert!(TEST_PAGE.contains("/api/stats"));
        assert!(TEST_PAGE.contains("token"));
    }
}
