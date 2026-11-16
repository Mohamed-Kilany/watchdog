//! Embedded web UI server.
//!
//! The static `ui/dist` build is embedded with [`rust_embed`] and served from
//! the host's own origin together with the live MJPEG stream, the stats API and
//! the health probe. Static responses carry a content-hash ETag, answer
//! `If-None-Match` with `304 Not Modified` and are gzip-compressed when the
//! client advertises support.

#![warn(missing_docs)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use flate2::write::GzEncoder;
use flate2::Compression;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::Result;
use watchdog_device::hub::FrameHub;

/// Minimal live view served when the UI build was not embedded (Rust-only
/// builds such as clippy/test jobs).
const FALLBACK_PAGE: &str = include_str!("test_page.html");
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
/// Responses smaller than this are served uncompressed even when the client
/// accepts gzip; the overhead is not worth it.
const COMPRESS_MIN_BYTES: usize = 512;

/// Static UI build embedded into the host binary.
///
/// Release builds embed the files. Debug builds read them from `ui/dist` so a
/// UI rebuild is visible without rebuilding the host. `allow_missing` keeps
/// Rust-only builds working when the UI has not been built; those builds serve
/// [`FALLBACK_PAGE`] at `/`.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist/"]
#[allow_missing = true]
struct UiAssets;

/// Number of files in the embedded UI build.
///
/// Returns zero when the host was built without `ui/dist` (for example a
/// Rust-only test job) and the fallback page is served instead.
#[must_use]
pub fn embedded_asset_count() -> usize {
    UiAssets::iter().count()
}

/// Options for [`serve_ui`].
#[derive(Debug, Clone)]
pub struct UiOptions {
    /// Address the UI port binds (for example `127.0.0.1:7700`).
    pub bind: SocketAddr,
    /// Bearer token required for every request when set. Clients pass it as
    /// `?token=...` or `Authorization: Bearer ...`.
    pub token: Option<String>,
}

/// Serve the embedded UI, the MJPEG stream, the stats API and the health probe
/// until the process exits.
///
/// Each client is handled in its own task; a slow or stalled browser only
/// affects its own connection, never the frame hub.
pub async fn serve_ui(options: UiOptions, hub: Arc<FrameHub>) -> Result<()> {
    let listener = TcpListener::bind(options.bind).await.map_err(|error| {
        WatchdogError::new(
            ErrorCode::PortInUse,
            format!("cannot bind the web UI at {}: {error}", options.bind),
        )
        .with_hint("choose another port with --ui-port or stop the process using it")
    })?;
    tracing::info!(address = %options.bind, "web UI listening");
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
            if name == key {
                Some(percent_decode(value))
            } else {
                None
            }
        })
    }
}

/// Decode `%XX` escapes in a query value or path (browsers use
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

/// Encode bytes as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// One response ready to be written to a connection.
struct Response {
    status: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn new(status: &'static str, content_type: &str, body: impl AsRef<[u8]>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", content_type.to_string())],
            body: body.as_ref().to_vec(),
        }
    }

    fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// One static asset plus the validator used for conditional requests.
struct Asset {
    bytes: Cow<'static, [u8]>,
    etag: String,
}

/// Load an asset from the embedded UI build.
fn embedded_asset(path: &str) -> Option<Asset> {
    UiAssets::get(path).map(|file| Asset {
        bytes: file.data,
        etag: format!("W/\"{}\"", hex(&file.metadata.sha256_hash())),
    })
}

async fn handle_connection(
    mut stream: TcpStream,
    hub: &FrameHub,
    token: Option<&str>,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await? else {
        return write_response(
            &mut stream,
            &Response::new(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"bad request\n",
            )
            .header("Cache-Control", "no-store"),
        )
        .await;
    };
    if !authorized(&request, token) {
        return write_response(
            &mut stream,
            &Response::new(
                "401 Unauthorized",
                "text/plain; charset=utf-8",
                b"unauthorized\n",
            )
            .header("Cache-Control", "no-store")
            .header("WWW-Authenticate", "Bearer"),
        )
        .await;
    }
    if request.method != "GET" {
        return write_response(
            &mut stream,
            &Response::new(
                "405 Method Not Allowed",
                "text/plain; charset=utf-8",
                b"method not allowed\n",
            )
            .header("Cache-Control", "no-store")
            .header("Allow", "GET"),
        )
        .await;
    }
    let response = match request.path() {
        "/api/stats" => {
            let body = serde_json::to_vec(&hub.stats()).map_err(std::io::Error::other)?;
            Response::new("200 OK", "application/json", body).header("Cache-Control", "no-store")
        }
        "/stream.mjpg" => return stream_mjpeg(stream, hub).await,
        "/healthz" => Response::new("200 OK", "text/plain; charset=utf-8", b"ok\n")
            .header("Cache-Control", "no-store"),
        _ => static_response(&request, embedded_asset),
    };
    write_response(&mut stream, &response).await
}

/// Resolve a request against the embedded UI.
///
/// `/` maps to `index.html`, exact asset paths win, and extensionless paths
/// fall back to the app shell for client-side routing. When no UI build is
/// embedded, `/` and extensionless routes serve the fallback live view.
fn static_response<F>(request: &Request, lookup: F) -> Response
where
    F: Fn(&str) -> Option<Asset>,
{
    let decoded = percent_decode(request.path());
    let relative = decoded.trim_start_matches('/');
    if relative.contains("..") || relative.contains('\\') || relative.contains('\0') {
        return not_found();
    }
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    if let Some(asset) = lookup(relative) {
        return asset_response(request, relative, asset);
    }
    if relative == "index.html" || !relative.contains('.') {
        if let Some(index) = lookup("index.html") {
            return asset_response(request, "index.html", index);
        }
        return fallback_response();
    }
    not_found()
}

fn asset_response(request: &Request, path: &str, asset: Asset) -> Response {
    let content_type = mime_for_path(path);
    let compressible = is_compressible(content_type);
    let mut headers = vec![
        ("Content-Type", content_type.to_string()),
        ("ETag", asset.etag.clone()),
        ("Cache-Control", cache_control(path).to_string()),
    ];
    if compressible {
        headers.push(("Vary", "Accept-Encoding".to_string()));
    }
    if request
        .header("if-none-match")
        .is_some_and(|value| etag_matches(value, &asset.etag))
    {
        return Response {
            status: "304 Not Modified",
            headers,
            body: Vec::new(),
        };
    }
    let mut body = asset.bytes.to_vec();
    if compressible
        && asset.bytes.len() >= COMPRESS_MIN_BYTES
        && accepts_gzip(request.header("accept-encoding"))
    {
        if let Some(compressed) = gzip_cached(path, &asset.etag, asset.bytes.as_ref()) {
            headers.push(("Content-Encoding", "gzip".to_string()));
            body = compressed;
        }
    }
    Response {
        status: "200 OK",
        headers,
        body,
    }
}

fn fallback_response() -> Response {
    Response::new(
        "200 OK",
        "text/html; charset=utf-8",
        FALLBACK_PAGE.as_bytes(),
    )
    .header("Cache-Control", "no-store")
    .header("X-Watchdog-UI", "fallback")
}

fn not_found() -> Response {
    Response::new("404 Not Found", "text/plain; charset=utf-8", b"not found\n")
        .header("Cache-Control", "no-store")
}

/// Map a file extension to the response content type.
fn mime_for_path(path: &str) -> &'static str {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("webmanifest" | "manifest") => "application/manifest+json",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn is_compressible(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || matches!(
            content_type,
            "application/json" | "application/manifest+json" | "application/wasm" | "image/svg+xml"
        )
}

fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        // Vite fingerprints asset filenames, so a URL can never change content.
        "public, max-age=31536000, immutable"
    } else {
        // The app shell must be revalidated; the ETag makes that a cheap 304.
        "no-cache"
    }
}

/// Compare an `If-None-Match` value with an entity tag.
///
/// ETags are weak (`W/"..."`) because the same content hash validates both the
/// identity and gzip representations; weak comparison is what conditional
/// requests require.
fn etag_matches(header: &str, etag: &str) -> bool {
    let target = etag.strip_prefix("W/").unwrap_or(etag);
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == target
    })
}

/// Whether an `Accept-Encoding` value allows a gzip response.
fn accepts_gzip(header: Option<&str>) -> bool {
    let Some(header) = header else {
        return false;
    };
    let mut explicit = None;
    let mut wildcard = None;
    for part in header.split(',') {
        let mut pieces = part.split(';');
        let coding = pieces.next().unwrap_or_default().trim();
        let quality = pieces
            .find_map(|parameter| {
                let (name, value) = parameter.split_once('=')?;
                if name.trim().eq_ignore_ascii_case("q") {
                    value.trim().parse::<f32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(1.0);
        if coding.eq_ignore_ascii_case("gzip") {
            explicit = Some(quality);
        } else if coding == "*" {
            wildcard = Some(quality);
        }
    }
    explicit.or(wildcard).is_some_and(|quality| quality > 0.0)
}

/// Gzip responses cached by asset path and ETag.
static GZIP_CACHE: OnceLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();

fn gzip_cached(path: &str, etag: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let key = format!("{etag}:{path}");
    let cache = GZIP_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&key) {
            return Some(cached.as_ref().clone());
        }
    }
    let compressed = gzip(bytes)?;
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, Arc::new(compressed.clone()));
    }
    Some(compressed)
}

fn gzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).ok()?;
    encoder.finish().ok()
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

async fn write_response<W>(stream: &mut W, response: &Response) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut head = format!("HTTP/1.1 {}\r\nConnection: close\r\n", response.status);
    if response.status != "304 Not Modified" {
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&response.body).await?;
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
    use flate2::read::GzDecoder;
    use std::io::Read as _;

    const BIG_JS: [u8; 2048] = [b'x'; 2048];
    const INDEX: &[u8] = b"<!doctype html><title>watchdog</title>";

    fn request(target: &str) -> Request {
        parse_request(&format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n"))
            .expect("valid request")
    }

    fn request_with_headers(target: &str, headers: &[(&str, &str)]) -> Request {
        let mut head = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n");
        for (name, value) in headers {
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        parse_request(&head).expect("valid request")
    }

    fn asset(bytes: &[u8], etag: &str) -> Asset {
        Asset {
            bytes: Cow::Owned(bytes.to_vec()),
            etag: etag.to_string(),
        }
    }

    fn site(path: &str) -> Option<Asset> {
        match path {
            "index.html" => Some(asset(INDEX, "W/\"index\"")),
            "assets/app.js" => Some(asset(&BIG_JS, "W/\"app-js\"")),
            "assets/app.css" => Some(asset(b"body{color:red}", "W/\"app-css\"")),
            "icon.svg" => Some(asset(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
                "W/\"icon\"",
            )),
            _ => None,
        }
    }

    fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response
            .headers
            .iter()
            .find(|(header, _)| *header == name)
            .map(|(_, value)| value.as_str())
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
    fn serves_the_index_with_a_content_hash_etag() {
        let response = static_response(&request("/"), site);
        assert_eq!(response.status, "200 OK");
        assert_eq!(
            header(&response, "Content-Type"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(header(&response, "ETag"), Some("W/\"index\""));
        assert_eq!(header(&response, "Cache-Control"), Some("no-cache"));
        assert_eq!(header(&response, "Vary"), Some("Accept-Encoding"));
        assert_eq!(response.body, INDEX.to_vec());
    }

    #[test]
    fn serves_fingerprinted_assets_as_immutable() {
        let response = static_response(&request("/assets/app.js"), site);
        assert_eq!(response.status, "200 OK");
        assert_eq!(
            header(&response, "Content-Type"),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(
            header(&response, "Cache-Control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(header(&response, "Content-Encoding"), None);
        assert_eq!(response.body, BIG_JS.to_vec());
    }

    #[test]
    fn falls_back_to_the_app_shell_for_extensionless_routes() {
        let response = static_response(&request("/devices/123"), site);
        assert_eq!(response.status, "200 OK");
        assert_eq!(header(&response, "ETag"), Some("W/\"index\""));
        assert_eq!(response.body, INDEX.to_vec());
    }

    #[test]
    fn missing_assets_are_not_found() {
        let response = static_response(&request("/assets/missing.js"), site);
        assert_eq!(response.status, "404 Not Found");
        assert_eq!(header(&response, "Cache-Control"), Some("no-store"));
    }

    #[test]
    fn rejects_path_traversal() {
        for target in [
            "/../secret",
            "/%2e%2e/secret",
            "/..%2fsecret",
            "/assets/..%5csecret",
            "/%00",
        ] {
            assert_eq!(
                static_response(&request(target), site).status,
                "404 Not Found",
                "{target}"
            );
        }
    }

    #[test]
    fn serves_the_fallback_page_when_no_ui_is_embedded() {
        let response = static_response(&request("/"), |_| None);
        assert_eq!(response.status, "200 OK");
        assert_eq!(header(&response, "X-Watchdog-UI"), Some("fallback"));
        assert_eq!(header(&response, "Cache-Control"), Some("no-store"));
        assert!(response
            .body
            .windows(b"/stream.mjpg".len())
            .any(|window| window == b"/stream.mjpg"));
    }

    #[test]
    fn not_modified_requests_return_a_bodyless_304() {
        let response = static_response(
            &request_with_headers("/", &[("If-None-Match", "W/\"index\"")]),
            site,
        );
        assert_eq!(response.status, "304 Not Modified");
        assert!(response.body.is_empty());
        assert_eq!(header(&response, "ETag"), Some("W/\"index\""));
        assert_eq!(header(&response, "Vary"), Some("Accept-Encoding"));
    }

    #[test]
    fn not_modified_accepts_a_list_and_wildcard() {
        for value in ["\"other\", W/\"index\"", "*", "\"index\""] {
            let response = static_response(
                &request_with_headers("/", &[("If-None-Match", value)]),
                site,
            );
            assert_eq!(response.status, "304 Not Modified", "{value}");
        }
    }

    #[test]
    fn a_stale_etag_returns_the_full_asset() {
        let response = static_response(
            &request_with_headers("/", &[("If-None-Match", "W/\"stale\"")]),
            site,
        );
        assert_eq!(response.status, "200 OK");
        assert_eq!(response.body, INDEX.to_vec());
    }

    #[test]
    fn gzip_is_used_when_the_client_accepts_it() {
        let response = static_response(
            &request_with_headers("/assets/app.js", &[("Accept-Encoding", "br, gzip")]),
            site,
        );
        assert_eq!(response.status, "200 OK");
        assert_eq!(header(&response, "Content-Encoding"), Some("gzip"));
        assert_eq!(header(&response, "Vary"), Some("Accept-Encoding"));
        let mut decoder = GzDecoder::new(response.body.as_slice());
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).expect("valid gzip");
        assert_eq!(decoded, BIG_JS.to_vec());
    }

    #[test]
    fn small_or_uncompressible_assets_are_not_gzipped() {
        let css = static_response(
            &request_with_headers("/assets/app.css", &[("Accept-Encoding", "gzip")]),
            site,
        );
        assert_eq!(header(&css, "Content-Encoding"), None);
        assert_eq!(header(&css, "Vary"), Some("Accept-Encoding"));

        let icon = static_response(
            &request_with_headers("/icon.svg", &[("Accept-Encoding", "gzip")]),
            site,
        );
        assert_eq!(header(&icon, "Content-Encoding"), None);
        assert_eq!(header(&icon, "Vary"), Some("Accept-Encoding"));
    }

    #[test]
    fn gzip_is_not_used_without_an_accept_encoding_header() {
        let response = static_response(&request("/assets/app.js"), site);
        assert_eq!(header(&response, "Content-Encoding"), None);
        assert_eq!(header(&response, "Vary"), Some("Accept-Encoding"));
    }

    #[test]
    fn zero_quality_disables_gzip() {
        for value in ["gzip;q=0", "gzip;q=0.0, *;q=1", "identity"] {
            let response = static_response(
                &request_with_headers("/assets/app.js", &[("Accept-Encoding", value)]),
                site,
            );
            assert_eq!(header(&response, "Content-Encoding"), None, "{value}");
        }
    }

    #[test]
    fn parses_accept_encoding_quality_values() {
        assert!(accepts_gzip(Some("gzip")));
        assert!(accepts_gzip(Some("GZIP")));
        assert!(accepts_gzip(Some("br, gzip;q=0.5")));
        assert!(accepts_gzip(Some("*")));
        assert!(!accepts_gzip(Some("br")));
        assert!(!accepts_gzip(Some("gzip;q=0")));
        assert!(!accepts_gzip(Some("gzip;q=0, *;q=1")));
        assert!(!accepts_gzip(None));
    }

    #[test]
    fn compares_etags_weakly_and_understands_lists() {
        assert!(etag_matches("W/\"abc\"", "W/\"abc\""));
        assert!(etag_matches("\"abc\"", "W/\"abc\""));
        assert!(etag_matches("W/\"abc\"", "\"abc\""));
        assert!(etag_matches("\"other\", \"abc\"", "W/\"abc\""));
        assert!(etag_matches("*", "W/\"abc\""));
        assert!(!etag_matches("\"other\"", "W/\"abc\""));
    }

    #[test]
    fn maps_extensions_to_content_types() {
        assert_eq!(mime_for_path("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            mime_for_path("assets/app.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_for_path("assets/app.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for_path("data.json"), "application/json");
        assert_eq!(mime_for_path("icon.svg"), "image/svg+xml");
        assert_eq!(mime_for_path("icon.png"), "image/png");
        assert_eq!(mime_for_path("font.woff2"), "font/woff2");
        assert_eq!(mime_for_path("app.wasm"), "application/wasm");
        assert_eq!(mime_for_path("blob.bin"), "application/octet-stream");
    }

    #[test]
    fn hex_encodes_lowercase() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }

    #[tokio::test]
    async fn writes_content_length_for_a_body() {
        let response = Response::new("200 OK", "text/plain", b"hello");
        let mut out = Vec::new();
        write_response(&mut out, &response).await.expect("write");
        let head = String::from_utf8_lossy(&out);
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Length: 5\r\n"));
        assert!(head.ends_with("\r\n\r\nhello"));
    }

    #[tokio::test]
    async fn not_modified_has_no_content_length() {
        let response = static_response(
            &request_with_headers("/", &[("If-None-Match", "W/\"index\"")]),
            site,
        );
        let mut out = Vec::new();
        write_response(&mut out, &response).await.expect("write");
        let head = String::from_utf8_lossy(&out);
        assert!(head.starts_with("HTTP/1.1 304 Not Modified\r\n"));
        assert!(!head.to_ascii_lowercase().contains("content-length"));
        assert!(head.ends_with("\r\n\r\n"));
    }

    #[test]
    fn fallback_page_uses_the_stream_and_stats_endpoints() {
        assert!(FALLBACK_PAGE.contains("/stream.mjpg"));
        assert!(FALLBACK_PAGE.contains("/api/stats"));
        assert!(FALLBACK_PAGE.contains("token"));
    }
}
