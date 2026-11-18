//! Embedded web UI server.
//!
//! The static `ui/dist` build is embedded with [`rust_embed`] and served from
//! the host's own origin together with the live MJPEG stream, the stats API and
//! the health probe. Static responses carry a content-hash ETag, answer
//! `If-None-Match` with `304 Not Modified` and are gzip-compressed when the
//! client advertises support.
//!
//! M6-T02 adds the app shell's real-time channel: `/api/state` returns the
//! merged control snapshot, `/api/command` forwards one command to the app
//! through the frame hub and `/ws` streams state snapshots and events over a
//! minimal RFC 6455 WebSocket. All three reuse the same bearer-token check as
//! the static files. M6-T03 adds the `/api/devices*` management API backed by
//! [`watchdog_device::devices::DeviceService`]; the live routes stay up while no
//! device is active and follow session switches without closing the socket.
//! M6-T04 adds `/api/snapshot`, which returns the latest viewfinder frame as a
//! downloadable JPEG so the live page's snapshot button never needs a canvas
//! copy of the stream. M6-T05 adds `/api/presets` (list, load, save, delete)
//! backed by `watchdog_presets::PresetStore` for the controls page.

#![warn(missing_docs)]

mod ws;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::Result;
use watchdog_device::devices::{DeviceService, ProvisionJobState, ProvisionRequest, SessionInfo};
use watchdog_device::hub::{FrameHub, ReceivedFrame, StreamStats};
use watchdog_device::mock::MockData;
use watchdog_presets::{Preset, PresetStore};
use watchdog_protocol::{CommandMessage, CommandResultMessage, StateMessage};

/// Minimal live view served when the UI build was not embedded (Rust-only
/// builds such as clippy/test jobs).
const FALLBACK_PAGE: &str = include_str!("test_page.html");
/// Longest request head accepted before returning `400 Bad Request`.
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// Largest command body accepted by `/api/command`.
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// How long a client has to send its request head or body.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Idle check interval while a viewer waits for frames.
const STREAM_TICK: Duration = Duration::from_secs(1);
/// How long a snapshot request waits for the next frame.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);
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
/// affects its own connection, never the frame hub. The [`DeviceService`] owns
/// the optional app session, so the UI keeps working before any device is
/// activated and while sessions switch.
pub async fn serve_ui(options: UiOptions, service: Arc<DeviceService>) -> Result<()> {
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
        let service = Arc::clone(&service);
        let token = Arc::clone(&token);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, &service, token.as_deref()).await {
                tracing::debug!(%peer, %error, "UI connection ended");
            }
        });
    }
}

/// One parsed HTTP/1.1 request.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
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
    service: &Arc<DeviceService>,
    token: Option<&str>,
) -> std::io::Result<()> {
    let request = match read_request(&mut stream).await? {
        ReadRequest::Request(request) => request,
        ReadRequest::BadRequest => {
            return write_response(
                &mut stream,
                &Response::new(
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    b"bad request\n",
                )
                .header("Cache-Control", "no-store"),
            )
            .await
        }
        ReadRequest::TooLarge => {
            return write_response(
                &mut stream,
                &Response::new(
                    "413 Payload Too Large",
                    "text/plain; charset=utf-8",
                    b"request body too large\n",
                )
                .header("Cache-Control", "no-store"),
            )
            .await
        }
    };
    if !is_public_asset(&request) && !authorized(&request, token) {
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
    if request.path() == "/ws" {
        return websocket_handshake(stream, &request, service).await;
    }
    let response = match (request.method.as_str(), request.path()) {
        ("GET", "/api/state") => state_response(service),
        ("POST", "/api/command") => command_response(service, &request).await,
        ("GET", "/api/stats") => stats_response(service),
        ("GET", "/api/presets") => presets_response(service, &request),
        ("POST", "/api/presets") => save_preset_response(service, &request),
        ("DELETE", "/api/presets") => delete_preset_response(service, &request),
        ("GET", "/api/devices") => devices_response(service, false).await,
        ("POST", "/api/devices/discover") => devices_response(service, true).await,
        ("POST", "/api/devices/pair") => pair_response(service, &request).await,
        ("POST", "/api/devices/activate") => activate_response(service, &request).await,
        ("POST", "/api/devices/disconnect") => disconnect_response(service).await,
        ("POST", "/api/devices/restore") => restore_response(service, &request).await,
        ("POST", "/api/devices/provision") => provision_response(service, &request),
        ("GET", "/api/devices/status") => status_response(service, &request).await,
        ("GET", path) if path.starts_with("/api/devices/provision/") => {
            provision_job_response(service, path)
        }
        ("GET", "/stream.mjpg") => {
            let Some(hub) = service.active_hub() else {
                return write_response(&mut stream, &no_session_response()).await;
            };
            return stream_mjpeg(stream, &hub).await;
        }
        ("GET", "/api/snapshot") => {
            return write_response(&mut stream, &snapshot_response(service).await).await;
        }
        ("GET", "/healthz") => Response::new("200 OK", "text/plain; charset=utf-8", b"ok\n")
            .header("Cache-Control", "no-store"),
        (_, path) if service.is_mock() && is_mock_api_path(path) => {
            mock_api_response(&request, service)
        }
        (_, "/api/command") => method_not_allowed("POST"),
        (_, "/api/snapshot") => method_not_allowed("GET"),
        (_, "/api/presets") => method_not_allowed("GET, POST, DELETE"),
        (_, "/api/devices/pair")
        | (_, "/api/devices/activate")
        | (_, "/api/devices/disconnect")
        | (_, "/api/devices/restore")
        | (_, "/api/devices/provision") => method_not_allowed("POST"),
        (_, "/api/devices/status")
        | (_, "/api/devices")
        | (_, "/api/state")
        | (_, "/api/stats") => method_not_allowed("GET"),
        (_, path) if path.starts_with("/api/") => api_not_found(),
        ("GET", _) => static_response(&request, embedded_asset),
        _ => method_not_allowed("GET"),
    };
    write_response(&mut stream, &response).await
}

fn method_not_allowed(allow: &'static str) -> Response {
    Response::new(
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed\n",
    )
    .header("Cache-Control", "no-store")
    .header("Allow", allow)
}

/// `GET /api/state` — the merged snapshot the app last reported.
fn state_response(service: &DeviceService) -> Response {
    json_response("200 OK", &snapshot_envelope(service))
}

/// The snapshot envelope for the current session (or a waiting one).
fn snapshot_envelope(service: &DeviceService) -> serde_json::Value {
    let session = service.session_info();
    match service.active_hub().and_then(|hub| hub.state()) {
        Some(state) => state_envelope(&state, &session),
        None => waiting_state_envelope(&session),
    }
}

/// `GET /api/presets` — list summaries, or one preset with `?name=`.
fn presets_response(service: &DeviceService, request: &Request) -> Response {
    let store = PresetStore::new(&service.paths().presets);
    if let Some(name) = request.query("name") {
        return match store.load(&name) {
            Ok(preset) => json_value_response("200 OK", &serde_json::to_value(preset)),
            Err(error) => watchdog_response(&error),
        };
    }
    match store.list() {
        Ok(presets) => json_response("200 OK", &serde_json::json!({ "presets": presets })),
        Err(error) => watchdog_response(&error),
    }
}

#[derive(Debug, Deserialize)]
struct PresetSaveBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    controls: BTreeMap<String, serde_json::Value>,
}

/// `POST /api/presets` — create or overwrite a preset from a controls map.
fn save_preset_response(service: &DeviceService, request: &Request) -> Response {
    let body: PresetSaveBody = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid preset body: {error}"),
                None,
            )
        }
    };
    let store = PresetStore::new(&service.paths().presets);
    let saved = Preset::new(body.name, body.description, body.controls).and_then(|mut preset| {
        store.save(&mut preset)?;
        Ok(preset)
    });
    match saved {
        Ok(preset) => json_value_response("200 OK", &serde_json::to_value(preset)),
        Err(error) => watchdog_response(&error),
    }
}

/// `DELETE /api/presets?name=...` — remove a stored preset.
fn delete_preset_response(service: &DeviceService, request: &Request) -> Response {
    let Some(name) = request.query("name") else {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "missing preset name",
            Some("pass ?name=<preset>"),
        );
    };
    match PresetStore::new(&service.paths().presets).delete(&name) {
        Ok(()) => json_response("200 OK", &serde_json::json!({ "ok": true, "name": name })),
        Err(error) => watchdog_response(&error),
    }
}

/// `GET /api/snapshot` - the latest viewfinder frame as a JPEG download.
async fn snapshot_response(service: &DeviceService) -> Response {
    let Some(hub) = service.active_hub() else {
        return no_session_response();
    };
    let Some(frame) = next_frame(hub.subscribe(), SNAPSHOT_TIMEOUT).await else {
        return api_error(
            "503 Service Unavailable",
            ErrorCode::StreamClosed,
            "no viewfinder frame is available yet",
            Some("wait for the live view to show frames, then try again"),
        );
    };
    Response::new("200 OK", "image/jpeg", &frame.jpeg)
        .header("Cache-Control", "no-store")
        .header(
            "Content-Disposition",
            "attachment; filename=\"watchdog-snapshot.jpg\"",
        )
}

/// Wait for the next usable frame, skipping lagged markers.
async fn next_frame(
    mut frames: broadcast::Receiver<Arc<ReceivedFrame>>,
    timeout: Duration,
) -> Option<Arc<ReceivedFrame>> {
    tokio::time::timeout(timeout, async {
        loop {
            match frames.recv().await {
                Ok(frame) => return Some(frame),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten()
}

/// `GET /api/stats` - frame counters, zeroed while no session is open.
fn stats_response(service: &DeviceService) -> Response {
    let stats = service
        .active_hub()
        .map_or_else(StreamStats::default, |hub| hub.stats());
    match serde_json::to_value(stats) {
        Ok(mut value) => {
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert(
                    "session".to_string(),
                    serde_json::to_value(service.session_info()).unwrap_or_default(),
                );
            }
            json_response("200 OK", &value)
        }
        Err(error) => internal_error(&error),
    }
}

/// `POST /api/command` — forward one command to the app and return its result.
async fn command_response(service: &DeviceService, request: &Request) -> Response {
    let message: CommandMessage = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid command body: {error}"),
                None,
            )
        }
    };
    if message.id.is_empty() {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "command id must not be empty",
            None,
        );
    }
    let Some(hub) = service.active_hub() else {
        return no_session_response();
    };
    match hub.command(message).await {
        Ok(result) => json_response("200 OK", &result_envelope(&result)),
        Err(error) => watchdog_response(&error),
    }
}

fn no_session_response() -> Response {
    api_error(
        "503 Service Unavailable",
        ErrorCode::DeviceOffline,
        "no device session is active",
        Some("open the devices page and activate a device"),
    )
}

/// Paths served by the synthetic backend (M6-T09).
fn is_mock_api_path(path: &str) -> bool {
    path == "/api/media"
        || path.starts_with("/api/media/")
        || path == "/api/analyses"
        || path == "/api/settings"
        || path == "/api/logs"
        || path == "/api/diagnostics"
}

/// Temporary mock-only media, analysis, settings and diagnostics routes.
///
/// The real media store, analysis queue and settings/logs stores land with
/// M5/M7/M9; these routes exist so the end-to-end suite exercises the real UI,
/// host and WebSocket against a synthetic device.
fn mock_api_response(request: &Request, service: &DeviceService) -> Response {
    let Some(data) = service.mock_data() else {
        return api_error(
            "503 Service Unavailable",
            ErrorCode::DeviceOffline,
            "the mock device is not active",
            None,
        );
    };
    let path = request.path();
    match (request.method.as_str(), path) {
        ("GET", "/api/media") => {
            json_response("200 OK", &serde_json::json!({ "items": data.list_media() }))
        }
        ("POST", "/api/analyses") => analyses_response(request, &data),
        ("GET", "/api/settings") => json_response("200 OK", &data.settings()),
        ("PATCH", "/api/settings") => patch_settings_response(request, &data),
        ("GET", "/api/logs") => logs_response(request, &data),
        ("GET", "/api/diagnostics") => diagnostics_response(&data),
        (_, "/api/media") => method_not_allowed("GET"),
        (_, path) if path.starts_with("/api/media/") => media_item_response(request, &data),
        (_, "/api/analyses") => method_not_allowed("POST"),
        (_, "/api/settings") => method_not_allowed("GET, PATCH"),
        (_, "/api/logs") | (_, "/api/diagnostics") => method_not_allowed("GET"),
        _ => api_not_found(),
    }
}

/// `GET`/`DELETE /api/media/{id}` and `GET /api/media/{id}/file`.
fn media_item_response(request: &Request, data: &MockData) -> Response {
    let rest = request.path().trim_start_matches("/api/media/");
    let (raw_id, file) = match rest.strip_suffix("/file") {
        Some(id) => (id, true),
        None => (rest, false),
    };
    let id = percent_decode(raw_id);
    if id.is_empty() || id.contains('/') {
        return api_not_found();
    }
    if file {
        return match (request.method.as_str(), data.media_file(&id)) {
            ("GET", Some((bytes, content_type))) => {
                Response::new("200 OK", content_type, bytes).header("Cache-Control", "no-store")
            }
            ("GET", None) => media_not_found(&id),
            (_, Some(_)) => method_not_allowed("GET"),
            (_, None) => media_not_found(&id),
        };
    }
    match (request.method.as_str(), data.get_media(&id)) {
        ("GET", Some(item)) => json_response("200 OK", &serde_json::json!({ "item": item })),
        ("GET", None) => media_not_found(&id),
        ("DELETE", Some(_)) => {
            if data.delete_media(&id) {
                json_response("200 OK", &serde_json::json!({ "ok": true, "id": id }))
            } else {
                media_not_found(&id)
            }
        }
        ("DELETE", None) => media_not_found(&id),
        _ => method_not_allowed("GET, DELETE"),
    }
}

fn media_not_found(id: &str) -> Response {
    api_error(
        "404 Not Found",
        ErrorCode::NotFound,
        &format!("no media item {id:?}"),
        None,
    )
}

#[derive(Debug, Deserialize)]
struct AnalysisBody {
    #[serde(default)]
    media_ids: Vec<String>,
}

/// `POST /api/analyses` — queue a mock analysis.
fn analyses_response(request: &Request, data: &MockData) -> Response {
    let body: AnalysisBody = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid analysis body: {error}"),
                None,
            )
        }
    };
    match data.submit_analysis(&body.media_ids) {
        Ok(payload) => json_response("200 OK", &payload),
        Err(message) => api_error("400 Bad Request", ErrorCode::InvalidRequest, &message, None),
    }
}

#[derive(Debug, Deserialize)]
struct SettingsPatch {
    #[serde(default)]
    retention: Option<RetentionPatch>,
}

#[derive(Debug, Deserialize)]
struct RetentionPatch {
    #[serde(default)]
    max_age_days: Option<i64>,
    #[serde(default)]
    max_size_mb: Option<i64>,
}

/// `PATCH /api/settings` — update the mock retention limits.
fn patch_settings_response(request: &Request, data: &MockData) -> Response {
    let body: SettingsPatch = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid settings body: {error}"),
                None,
            )
        }
    };
    let Some(retention) = body.retention else {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "the retention object is required",
            Some("send {\"retention\":{\"max_age_days\":30,\"max_size_mb\":10240}}"),
        );
    };
    let updated = data.update_retention(
        retention.max_age_days.unwrap_or(30),
        retention.max_size_mb.unwrap_or(10_240),
    );
    json_response("200 OK", &serde_json::json!({ "retention": updated }))
}

/// `GET /api/logs?level=&limit=` — the mock log ring buffer.
fn logs_response(request: &Request, data: &MockData) -> Response {
    let level = request.query("level");
    let limit = request
        .query("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200);
    json_response(
        "200 OK",
        &serde_json::json!({ "lines": data.logs(level.as_deref(), limit) }),
    )
}

/// `GET /api/diagnostics` — a stored (uncompressed) ZIP with one JSON entry.
fn diagnostics_response(data: &MockData) -> Response {
    let json = data.diagnostics_json();
    let zip = zip_single_file("diagnostics.json", &json);
    Response::new("200 OK", "application/zip", zip)
        .header("Cache-Control", "no-store")
        .header(
            "Content-Disposition",
            format!(
                "attachment; filename=\"watchdog-diagnostics-{}.zip\"",
                now_ms()
            ),
        )
}

/// Build a valid ZIP archive with one stored (method 0) entry.
fn zip_single_file(name: &str, contents: &[u8]) -> Vec<u8> {
    let crc = crc32fast::hash(contents);
    let size = u32::try_from(contents.len()).unwrap_or(u32::MAX);
    let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
    let mut zip = Vec::with_capacity(contents.len() + 256);
    // Local file header.
    zip.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
    zip.extend_from_slice(&20_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(name.as_bytes());
    zip.extend_from_slice(contents);
    let central_offset = u32::try_from(zip.len()).unwrap_or(u32::MAX);
    // Central directory header.
    zip.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
    zip.extend_from_slice(&20_u16.to_le_bytes());
    zip.extend_from_slice(&20_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&name_len.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u32.to_le_bytes());
    zip.extend_from_slice(&0_u32.to_le_bytes());
    zip.extend_from_slice(name.as_bytes());
    let central_size = u32::try_from(zip.len()).unwrap_or(u32::MAX) - central_offset;
    // End of central directory.
    zip.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip.extend_from_slice(&1_u16.to_le_bytes());
    zip.extend_from_slice(&1_u16.to_le_bytes());
    zip.extend_from_slice(&central_size.to_le_bytes());
    zip.extend_from_slice(&central_offset.to_le_bytes());
    zip.extend_from_slice(&0_u16.to_le_bytes());
    zip
}

fn api_not_found() -> Response {
    api_error(
        "404 Not Found",
        ErrorCode::NotFound,
        "no such API route",
        Some("see docs/api.md for the supported routes"),
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

/// `GET /api/devices` and `POST /api/devices/discover`.
async fn devices_response(service: &DeviceService, discover: bool) -> Response {
    match service.list(discover).await {
        Ok(list) => json_value_response("200 OK", &serde_json::to_value(list)),
        Err(error) => watchdog_response(&error),
    }
}

#[derive(Debug, Deserialize)]
struct PairBody {
    target: String,
    code: String,
}

/// `POST /api/devices/pair` — wireless-debugging pairing with a 6-digit code.
async fn pair_response(service: &DeviceService, request: &Request) -> Response {
    let body: PairBody = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid pair body: {error}"),
                None,
            )
        }
    };
    match service.pair(&body.target, &body.code).await {
        Ok(guid) => json_response("200 OK", &serde_json::json!({ "ok": true, "guid": guid })),
        Err(error) => watchdog_response(&error),
    }
}

#[derive(Debug, Deserialize)]
struct SerialBody {
    serial: String,
}

/// `POST /api/devices/activate` — open the app session for one device.
async fn activate_response(service: &DeviceService, request: &Request) -> Response {
    let body: SerialBody = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid activate body: {error}"),
                None,
            )
        }
    };
    match service.activate(&body.serial).await {
        Ok(device) => {
            let session = SessionInfo {
                active: true,
                closed: false,
                serial: Some(device.serial.clone()),
                device: Some(device),
            };
            json_response(
                "200 OK",
                &serde_json::json!({ "ok": true, "session": session }),
            )
        }
        Err(error) => watchdog_response(&error),
    }
}

/// `POST /api/devices/disconnect` — close the active session.
async fn disconnect_response(service: &DeviceService) -> Response {
    let previous = service.deactivate().await;
    json_response(
        "200 OK",
        &serde_json::json!({
            "ok": true,
            "session": service.session_info(),
            "disconnected": previous.map(|device| device.serial),
        }),
    )
}

/// `POST /api/devices/restore` — revert provisioning's screen settings.
async fn restore_response(service: &DeviceService, request: &Request) -> Response {
    let body: SerialBody = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid restore body: {error}"),
                None,
            )
        }
    };
    match service.restore(&body.serial).await {
        Ok((serial, steps)) => json_response(
            "200 OK",
            &serde_json::json!({ "ok": true, "serial": serial, "steps": steps }),
        ),
        Err(error) => watchdog_response(&error),
    }
}

/// `POST /api/devices/provision` — start a background provisioning job.
fn provision_response(service: &Arc<DeviceService>, request: &Request) -> Response {
    let body: ProvisionRequest = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return api_error(
                "400 Bad Request",
                ErrorCode::InvalidRequest,
                &format!("invalid provision body: {error}"),
                None,
            )
        }
    };
    match service.start_provision(body) {
        Ok(id) => {
            let job: Option<ProvisionJobState> = service.provision_job(&id);
            json_response("202 Accepted", &serde_json::json!({ "id": id, "job": job }))
        }
        Err(error) => watchdog_response(&error),
    }
}

/// `GET /api/devices/provision/{id}` — poll one provisioning job.
fn provision_job_response(service: &DeviceService, path: &str) -> Response {
    let id = percent_decode(path.trim_start_matches("/api/devices/provision/"));
    match service.provision_job(&id) {
        Some(job) => json_value_response("200 OK", &serde_json::to_value(job)),
        None => api_error(
            "404 Not Found",
            ErrorCode::JobNotFound,
            &format!("no provisioning job {id:?}"),
            None,
        ),
    }
}

/// `GET /api/devices/status?serial=...` — app/permission/version status.
async fn status_response(service: &DeviceService, request: &Request) -> Response {
    let Some(serial) = request.query("serial") else {
        return api_error(
            "400 Bad Request",
            ErrorCode::InvalidRequest,
            "the serial query parameter is required",
            None,
        );
    };
    match service.status(&serial).await {
        Ok(report) => json_value_response("200 OK", &serde_json::to_value(report)),
        Err(error) => watchdog_response(&error),
    }
}

fn json_value_response(
    status: &'static str,
    value: &std::result::Result<serde_json::Value, serde_json::Error>,
) -> Response {
    match value {
        Ok(value) => json_response(status, value),
        Err(error) => internal_error(error),
    }
}

fn watchdog_response(error: &WatchdogError) -> Response {
    let status = match error.code {
        ErrorCode::InvalidRequest => "400 Bad Request",
        ErrorCode::NotFound | ErrorCode::DeviceNotFound | ErrorCode::JobNotFound => "404 Not Found",
        ErrorCode::DeviceUnauthorized => "403 Forbidden",
        ErrorCode::DeviceOffline | ErrorCode::StreamClosed => "503 Service Unavailable",
        ErrorCode::ConnectionLost => "504 Gateway Timeout",
        ErrorCode::Conflict => "409 Conflict",
        _ => "502 Bad Gateway",
    };
    api_error(status, error.code, &error.message, error.hint.as_deref())
}

/// Serialize the WS/REST state envelope (`{ "type": "state", ... }`).
pub(crate) fn state_envelope(state: &StateMessage, session: &SessionInfo) -> serde_json::Value {
    serde_json::json!({
        "type": "state",
        "seq": state.seq,
        "timestamp_ms": state.timestamp_ms,
        "camera_id": state.camera_id,
        "state": state.state,
        "dropped_frames": state.dropped_frames,
        "storage_free_mb": state.storage_free_mb,
        "session": session,
    })
}

/// Envelope sent before the app has produced its first full `STATE`.
pub(crate) fn waiting_state_envelope(session: &SessionInfo) -> serde_json::Value {
    serde_json::json!({
        "type": "state",
        "waiting": true,
        "seq": 0,
        "timestamp_ms": 0,
        "camera_id": "",
        "state": {},
        "session": session,
    })
}

/// Envelope sent whenever the active device session changes.
pub(crate) fn session_envelope(session: &SessionInfo) -> serde_json::Value {
    serde_json::json!({ "type": "session", "session": session })
}

/// Serialize the WS/REST command-result envelope.
pub(crate) fn result_envelope(result: &CommandResultMessage) -> serde_json::Value {
    serde_json::json!({
        "type": "result",
        "id": result.id,
        "timestamp_ms": result.timestamp_ms,
        "results": result.results,
        "error": result.error,
    })
}

fn json_response(status: &'static str, value: &serde_json::Value) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => {
            Response::new(status, "application/json", body).header("Cache-Control", "no-store")
        }
        Err(error) => internal_error(&error),
    }
}

fn api_error(status: &'static str, code: ErrorCode, message: &str, hint: Option<&str>) -> Response {
    let mut error = serde_json::json!({ "code": code.as_str(), "message": message });
    if let (Some(hint), serde_json::Value::Object(map)) = (hint, &mut error) {
        map.insert(
            "hint".to_string(),
            serde_json::Value::String(hint.to_string()),
        );
    }
    json_response(status, &serde_json::json!({ "error": error }))
}

fn internal_error(error: &impl std::fmt::Display) -> Response {
    let body = serde_json::json!({
        "error": { "code": ErrorCode::Internal.as_str(), "message": error.to_string() }
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    Response::new("500 Internal Server Error", "application/json", bytes)
        .header("Cache-Control", "no-store")
}

/// Validate the RFC 6455 upgrade and hand the stream to the WebSocket loop.
async fn websocket_handshake(
    mut stream: TcpStream,
    request: &Request,
    service: &DeviceService,
) -> std::io::Result<()> {
    if request.method != "GET" {
        return write_response(&mut stream, &method_not_allowed("GET")).await;
    }
    let upgrade = request
        .header("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection = request.header("connection").is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
    });
    let version = request.header("sec-websocket-version") == Some("13");
    let Some(key) = request
        .header("sec-websocket-key")
        .filter(|_| upgrade && connection && version)
    else {
        return write_response(
            &mut stream,
            &Response::new(
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"invalid websocket handshake\n",
            )
            .header("Cache-Control", "no-store"),
        )
        .await;
    };
    ws::upgrade(stream, key, service).await
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
        Some("vtt") => "text/vtt; charset=utf-8",
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

/// Outcome of reading one HTTP request from a connection.
enum ReadRequest {
    Request(Request),
    BadRequest,
    TooLarge,
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<ReadRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let read = match tokio::time::timeout(REQUEST_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(read) => read?,
            Err(_) => return Ok(ReadRequest::BadRequest),
        };
        if read == 0 {
            return Ok(ReadRequest::BadRequest);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer
            .windows(HEAD_END.len())
            .position(|window| window == HEAD_END)
        {
            break position;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Ok(ReadRequest::BadRequest);
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]);
    let Some(mut request) = parse_request(&head) else {
        return Ok(ReadRequest::BadRequest);
    };
    let content_length = match request.header("content-length") {
        Some(value) => match value.parse::<usize>() {
            Ok(length) => length,
            Err(_) => return Ok(ReadRequest::BadRequest),
        },
        None => 0,
    };
    if content_length > MAX_BODY_BYTES {
        return Ok(ReadRequest::TooLarge);
    }
    let mut body = buffer[head_end + HEAD_END.len()..].to_vec();
    while body.len() < content_length {
        let read = match tokio::time::timeout(REQUEST_TIMEOUT, stream.read(&mut chunk)).await {
            Ok(read) => read?,
            Err(_) => return Ok(ReadRequest::BadRequest),
        };
        if read == 0 {
            return Ok(ReadRequest::BadRequest);
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_BODY_BYTES {
            return Ok(ReadRequest::TooLarge);
        }
    }
    body.truncate(content_length);
    request.body = body;
    Ok(ReadRequest::Request(request))
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
        body: Vec::new(),
    })
}

/// Static UI files are served without a token.
///
/// A browser asks for the app shell and its assets before any JavaScript can
/// read a token, so gating them would leave the page blank instead of showing
/// the token prompt. They contain no user data; every dynamic route (the
/// stream, `/healthz`, the WebSocket and `/api/*`) still requires the token.
fn is_public_asset(request: &Request) -> bool {
    request.method == "GET"
        && !request.path().starts_with("/api/")
        && !matches!(request.path(), "/ws" | "/stream.mjpg" | "/healthz")
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

    fn frame(seq: u64) -> ReceivedFrame {
        ReceivedFrame {
            seq,
            sensor_timestamp_ns: seq * 1_000_000,
            width: 2,
            height: 2,
            rotation: watchdog_protocol::Rotation::R0,
            flags: 0,
            jpeg: bytes::Bytes::from_static(b"\xff\xd8jpeg\xff\xd9"),
            received_at: std::time::Instant::now(),
            latency_ms: None,
        }
    }

    #[tokio::test]
    async fn next_frame_skips_lagged_markers() {
        let (sender, receiver) = broadcast::channel(1);
        sender.send(Arc::new(frame(1))).expect("send first");
        sender.send(Arc::new(frame(2))).expect("send second");
        let received = next_frame(receiver, Duration::from_secs(1))
            .await
            .expect("a frame");
        assert_eq!(received.seq, 2);
    }

    #[tokio::test]
    async fn next_frame_times_out_without_frames() {
        let (_sender, receiver) = broadcast::channel(1);
        let received = next_frame(receiver, Duration::from_millis(10)).await;
        assert!(received.is_none());
    }

    #[tokio::test]
    async fn next_frame_reports_a_closed_stream() {
        let (sender, receiver) = broadcast::channel(1);
        drop(sender);
        let received = next_frame(receiver, Duration::from_secs(1)).await;
        assert!(received.is_none());
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
    fn static_ui_files_are_public() {
        for target in ["/", "/assets/app.js", "/assets/app.css", "/devices/123"] {
            assert!(is_public_asset(&request(target)), "{target}");
        }
    }

    #[test]
    fn dynamic_routes_still_require_a_token() {
        for target in [
            "/api/state",
            "/api/media/m-1/file",
            "/api/settings",
            "/api/diagnostics",
            "/stream.mjpg",
            "/healthz",
            "/ws",
        ] {
            assert!(!is_public_asset(&request(target)), "{target}");
        }
        let post = parse_request("POST / HTTP/1.1\r\n\r\n").expect("valid request");
        assert!(!is_public_asset(&post));
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
        assert_eq!(
            mime_for_path("captions.vtt"),
            "text/vtt; charset=utf-8"
        );
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

    #[test]
    fn preset_api_saves_lists_loads_and_deletes() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "watchdog-ui-presets-{}-{stamp}",
            std::process::id()
        ));
        let paths = watchdog_core::paths::DataPaths::resolve(&root);
        let service = DeviceService::new(paths).expect("device service");

        let payload = serde_json::json!({
            "name": "TV screen capture",
            "description": "Manual exposure",
            "controls": { "ae_mode": 0, "iso": 100, "zoom_ratio": 1.0 }
        })
        .to_string();
        let mut save_request = parse_request(&format!(
            "POST /api/presets HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            payload.len(),
            payload
        ))
        .expect("valid request");
        save_request.body = payload.into_bytes();
        let saved = save_preset_response(&service, &save_request);
        assert_eq!(
            saved.status,
            "200 OK",
            "{}",
            String::from_utf8_lossy(&saved.body)
        );
        let saved_body: serde_json::Value =
            serde_json::from_slice(&saved.body).expect("preset JSON");
        assert_eq!(saved_body["name"], "TV screen capture");
        assert_eq!(saved_body["controls"]["iso"], 100);

        let listed = presets_response(&service, &request("/api/presets"));
        assert_eq!(listed.status, "200 OK");
        let listed_body: serde_json::Value =
            serde_json::from_slice(&listed.body).expect("list JSON");
        assert_eq!(listed_body["presets"][0]["name"], "TV screen capture");
        assert_eq!(listed_body["presets"][0]["controls"], 3);

        let loaded = presets_response(&service, &request("/api/presets?name=tv-screen-capture"));
        assert_eq!(loaded.status, "200 OK");
        let loaded_body: serde_json::Value =
            serde_json::from_slice(&loaded.body).expect("preset JSON");
        assert_eq!(loaded_body["controls"]["ae_mode"], 0);

        let deleted =
            delete_preset_response(&service, &request("/api/presets?name=tv-screen-capture"));
        assert_eq!(deleted.status, "200 OK");
        let missing = presets_response(&service, &request("/api/presets?name=tv-screen-capture"));
        assert_eq!(missing.status, "404 Not Found");

        let _ = std::fs::remove_dir_all(&root);
    }
}
