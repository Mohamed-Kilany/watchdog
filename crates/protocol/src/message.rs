//! Typed message payloads (spec §5).

use std::collections::BTreeMap;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::packet::{MessageType, Packet};
use crate::DecodeError;

/// Serde map alias used for capability descriptors and control snapshots.
pub type JsonMap = BTreeMap<String, serde_json::Value>;

/// `HOST_HELLO` — handshake request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostHello {
    /// Lowest protocol version the host speaks.
    pub protocol_min: u8,
    /// Highest protocol version the host speaks.
    pub protocol_max: u8,
    /// Host identity.
    pub host: HostInfo,
    /// Random per-session token (hex); a new host session with a different
    /// token replaces the old one.
    pub session_token: String,
}

/// Host identity in `HOST_HELLO`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// Host program name (`watchdog`).
    pub name: String,
    /// Host version string.
    pub version: String,
}

/// App identity in `HELLO_ACK`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    /// `versionCode` from the APK.
    pub version_code: i64,
    /// `versionName` from the APK.
    pub version_name: String,
    /// Application id (`app.watchdog.remote`).
    #[serde(default = "default_package")]
    pub package: String,
}

fn default_package() -> String {
    "app.watchdog.remote".to_string()
}

/// Device information in `HELLO_ACK`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// `Build.MANUFACTURER`.
    pub manufacturer: String,
    /// `Build.MODEL`.
    pub model: String,
    /// `Build.DEVICE`.
    #[serde(default)]
    pub device: String,
    /// `Build.VERSION.SDK_INT`.
    pub sdk: i64,
    /// `Build.VERSION.RELEASE`.
    #[serde(default)]
    pub release: String,
    /// `Build.SUPPORTED_ABIS[0]`.
    #[serde(default)]
    pub abi: String,
    /// Current battery percentage, 0–100.
    #[serde(default)]
    pub battery_percent: Option<i64>,
    /// Free internal storage in MiB.
    #[serde(default)]
    pub storage_free_mb: Option<u64>,
    /// Battery temperature in °C.
    #[serde(default)]
    pub temperature_c: Option<f64>,
}

/// Camera entry in `HELLO_ACK`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraDescriptor {
    /// Camera id (string, as in `CameraCharacteristics`).
    pub id: String,
    /// `LENS_FACING_*` mapped to a lowercase string.
    pub facing: String,
    /// `INFO_SUPPORTED_HARDWARE_LEVEL_*` mapped to a name.
    pub hardware_level: String,
    /// True when the device exposes this as a logical multi-camera.
    #[serde(default)]
    pub logical_multi_camera: bool,
    /// Physical camera ids backing the logical camera.
    #[serde(default)]
    pub physical_ids: Vec<String>,
}

/// A single capability block (passthrough; schema lands with M4).
pub type Capability = JsonMap;

/// `HELLO_ACK` — handshake reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloAck {
    /// Negotiated protocol version.
    pub protocol_version: u8,
    /// App identity.
    pub app: AppInfo,
    /// Device identity and dynamic vitals.
    pub device: DeviceInfo,
    /// All cameras on the device.
    pub cameras: Vec<CameraDescriptor>,
    /// Per-camera capability descriptors keyed by camera id.
    #[serde(default)]
    pub capabilities: JsonMap,
}

/// Rotation degrees for `FRAME`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rotation {
    /// 0°.
    R0 = 0,
    /// 90° clockwise.
    R90 = 90,
    /// 180°.
    R180 = 180,
    /// 270° clockwise.
    R270 = 270,
}

impl Rotation {
    fn degrees(self) -> u16 {
        match self {
            Self::R0 => 0,
            Self::R90 => 90,
            Self::R180 => 180,
            Self::R270 => 270,
        }
    }

    fn from_degrees(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::R0),
            90 => Some(Self::R90),
            180 => Some(Self::R180),
            270 => Some(Self::R270),
            _ => None,
        }
    }
}

/// `FRAME` — one JPEG viewfinder image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePacket {
    /// Monotonic per-session sequence number; gaps mean dropped frames.
    pub seq: u64,
    /// `CaptureResult.SENSOR_TIMESTAMP` (CLOCK_BOOTTIME domain, ns).
    pub sensor_timestamp_ns: u64,
    /// JPEG width in pixels.
    pub width: u32,
    /// JPEG height in pixels.
    pub height: u32,
    /// Display rotation hint for the JPEG.
    pub rotation: Rotation,
    /// Frame flags (bit 0: color transform present; bit 1: snapshot).
    pub flags: u8,
    /// Complete JPEG (FF D8 … FF D9).
    pub jpeg: Bytes,
}

const FRAME_HEADER_LEN: usize = 28;

impl FramePacket {
    fn encode_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + self.jpeg.len());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.sensor_timestamp_ns.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.rotation.degrees().to_le_bytes());
        out.push(self.flags);
        out.push(0); // reserved
        out.extend_from_slice(&self.jpeg);
        out
    }

    fn decode_binary(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < FRAME_HEADER_LEN {
            return Err(DecodeError::Binary {
                message_type: "FRAME",
                reason: format!("truncated frame header: {} bytes", bytes.len()),
            });
        }
        let seq = u64::from_le_bytes(bytes[0..8].try_into().expect("slice len"));
        let sensor_timestamp_ns = u64::from_le_bytes(bytes[8..16].try_into().expect("slice len"));
        let width = u32::from_le_bytes(bytes[16..20].try_into().expect("slice len"));
        let height = u32::from_le_bytes(bytes[20..24].try_into().expect("slice len"));
        let rotation_raw = u16::from_le_bytes(bytes[24..26].try_into().expect("slice len"));
        let flags = bytes[26];
        if bytes[27] != 0 {
            return Err(DecodeError::Binary {
                message_type: "FRAME",
                reason: "reserved byte must be 0".to_string(),
            });
        }
        let rotation = Rotation::from_degrees(rotation_raw).ok_or_else(|| DecodeError::Binary {
            message_type: "FRAME",
            reason: format!("invalid rotation {rotation_raw}"),
        })?;
        Ok(Self {
            seq,
            sensor_timestamp_ns,
            width,
            height,
            rotation,
            flags,
            jpeg: Bytes::copy_from_slice(&bytes[FRAME_HEADER_LEN..]),
        })
    }
}

/// Who set a control, in `ControlEntry.source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlSource {
    /// Changed by the host (UI/MCP/API).
    Host,
    /// Changed on the device itself.
    Device,
    /// Set by the camera HAL automatically.
    Auto,
}

/// One control's state in `STATE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlEntry {
    /// Last requested value (absent for device-originated readouts).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub requested: Option<serde_json::Value>,
    /// Latest reported value from `TotalCaptureResult`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub actual: Option<serde_json::Value>,
    /// Whether the control is currently locked (AE/AWB/AF).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub locked: Option<bool>,
    /// Whether the hardware supports this control.
    pub supported: bool,
    /// Who last wrote the value.
    pub source: ControlSource,
}

/// `STATE` — full or delta control/camera state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateMessage {
    /// Monotonic state sequence.
    pub seq: u64,
    /// Milliseconds since epoch (device clock).
    pub timestamp_ms: u64,
    /// Camera the state belongs to.
    pub camera_id: String,
    /// Control entries keyed by control id.
    #[serde(default)]
    pub state: BTreeMap<String, ControlEntry>,
    /// Frames dropped by the app's send queue since boot.
    #[serde(default)]
    pub dropped_frames: u64,
    /// Free internal storage in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_free_mb: Option<u64>,
    /// True when only changed entries are present (header flag 0x01).
    #[serde(skip)]
    pub delta: bool,
}

/// `COMMAND` — host → app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandMessage {
    /// Client-chosen correlation id; echoed in `COMMAND_RESULT`.
    pub id: String,
    /// Command name (`set_controls`, `capture_still`, …).
    pub command: String,
    /// Command parameters (object).
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Per-control outcome in `COMMAND_RESULT`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlResult {
    /// Control id this result is for (absent for whole-command failures).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<String>,
    /// Whether the item succeeded.
    pub ok: bool,
    /// Applied (effective) value, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<serde_json::Value>,
    /// Stable error code on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Human-readable failure detail on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `COMMAND_RESULT` — app → host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResultMessage {
    /// Echoed `CommandMessage.id`.
    pub id: String,
    /// Device time when the command finished.
    pub timestamp_ms: u64,
    /// One result per control (or one global result for non-control commands).
    #[serde(default)]
    pub results: Vec<ControlResult>,
    /// Whole-command error, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `EVENT` — asynchronous app notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMessage {
    /// Event kind (`camera_disconnected`, …; see spec §5.7).
    pub kind: String,
    /// Device time of the event.
    pub timestamp_ms: u64,
    /// Event-specific payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// One media item in `MEDIA_MANIFEST`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    /// Host-visible media id (ULID recommended).
    pub id: String,
    /// `still` or `video`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Capture time (ms since epoch, device clock).
    pub created_ms: u64,
    /// Duration in ms for videos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// File size in bytes.
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    /// Path relative to the app's files dir.
    pub device_path: String,
    /// MIME type, e.g. `video/mp4` or `image/jpeg`.
    pub mime: String,
    /// Control snapshot at capture time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_snapshot: Option<JsonMap>,
    /// Device info block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<JsonMap>,
    /// Host job id if captured under a scheduler job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_job_id: Option<String>,
}

/// `MEDIA_MANIFEST` — a media item is ready to pull.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaManifest {
    /// The item descriptor.
    pub item: ManifestItem,
}

/// Log severity in `LOG_CHUNK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    /// Trace.
    Trace = 0,
    /// Debug.
    Debug = 1,
    /// Info.
    Info = 2,
    /// Warn.
    Warn = 3,
    /// Error.
    Error = 4,
}

impl LogLevel {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Trace,
            1 => Self::Debug,
            2 => Self::Info,
            3 => Self::Warn,
            4 => Self::Error,
            _ => return None,
        })
    }
}

/// `LOG_CHUNK` — device log ring-buffer segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogChunk {
    /// Device time of the log entry batch.
    pub timestamp_ms: u64,
    /// Severity.
    pub level: LogLevel,
    /// Raw UTF-8 text (may contain newlines).
    pub text: String,
}

/// `ERROR` — fatal or transient error report from the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Stable screaming-snake code.
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Actionable remediation hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// `PING`/`PONG` — liveness and latency estimation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingPong {
    /// Sender clock, ms since epoch.
    pub ts_ms: u64,
    /// Echoed probe identifier (host-chosen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// App boottime in ns (PONG only), used for clock-offset estimation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_ts_ns: Option<u64>,
}

/// `BYE` — orderly close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByeMessage {
    /// Close reason (`host_shutdown`, `session_replaced`, `app_shutdown`, `error`).
    pub reason: String,
    /// Optional detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// STATE header flag: payload carries only changed entries.
pub const STATE_DELTA_FLAG: u8 = 0x01;

/// Typed message envelope.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// `HOST_HELLO` (host → app).
    HostHello(HostHello),
    /// `HELLO_ACK` (app → host).
    HelloAck(HelloAck),
    /// `FRAME` (app → host).
    Frame(FramePacket),
    /// `STATE` (app → host).
    State(StateMessage),
    /// `COMMAND` (host → app).
    Command(CommandMessage),
    /// `COMMAND_RESULT` (app → host).
    CommandResult(CommandResultMessage),
    /// `EVENT` (app → host).
    Event(EventMessage),
    /// `MEDIA_MANIFEST` (app → host).
    MediaManifest(MediaManifest),
    /// `LOG_CHUNK` (app → host).
    LogChunk(LogChunk),
    /// `ERROR` (app → host).
    Error(ErrorMessage),
    /// `PING` (host → app).
    Ping(PingPong),
    /// `PONG` (app → host).
    Pong(PingPong),
    /// `BYE` (both).
    Bye(ByeMessage),
}

impl Message {
    /// The type byte for this message.
    #[must_use]
    pub fn msg_type(&self) -> MessageType {
        match self {
            Self::HostHello(_) => MessageType::HostHello,
            Self::HelloAck(_) => MessageType::HelloAck,
            Self::Frame(_) => MessageType::Frame,
            Self::State(_) => MessageType::State,
            Self::Command(_) => MessageType::Command,
            Self::CommandResult(_) => MessageType::CommandResult,
            Self::Event(_) => MessageType::Event,
            Self::MediaManifest(_) => MessageType::MediaManifest,
            Self::LogChunk(_) => MessageType::LogChunk,
            Self::Error(_) => MessageType::Error,
            Self::Ping(_) => MessageType::Ping,
            Self::Pong(_) => MessageType::Pong,
            Self::Bye(_) => MessageType::Bye,
        }
    }

    /// Decode a packet into a typed message.
    pub fn from_packet(packet: Packet) -> Result<Self, DecodeError> {
        let flags = packet.flags;
        let msg_type = packet.msg_type;
        let payload = packet.payload;
        match msg_type {
            MessageType::HostHello => Ok(Self::HostHello(
                serde_json::from_slice(&payload).map_err(|source| DecodeError::Json {
                    message_type: "HOST_HELLO",
                    source,
                })?,
            )),
            MessageType::HelloAck => Ok(Self::HelloAck(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "HELLO_ACK",
                    source,
                },
            )?)),
            MessageType::Frame => Ok(Self::Frame(FramePacket::decode_binary(&payload)?)),
            MessageType::State => {
                let mut state: StateMessage =
                    serde_json::from_slice(&payload).map_err(|source| DecodeError::Json {
                        message_type: "STATE",
                        source,
                    })?;
                state.delta = flags & STATE_DELTA_FLAG != 0;
                Ok(Self::State(state))
            }
            MessageType::Command => Ok(Self::Command(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "COMMAND",
                    source,
                },
            )?)),
            MessageType::CommandResult => Ok(Self::CommandResult(
                serde_json::from_slice(&payload).map_err(|source| DecodeError::Json {
                    message_type: "COMMAND_RESULT",
                    source,
                })?,
            )),
            MessageType::Event => Ok(Self::Event(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "EVENT",
                    source,
                },
            )?)),
            MessageType::MediaManifest => Ok(Self::MediaManifest(
                serde_json::from_slice(&payload).map_err(|source| DecodeError::Json {
                    message_type: "MEDIA_MANIFEST",
                    source,
                })?,
            )),
            MessageType::LogChunk => Ok(Self::LogChunk(decode_log_chunk(&payload)?)),
            MessageType::Error => Ok(Self::Error(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "ERROR",
                    source,
                },
            )?)),
            MessageType::Ping => Ok(Self::Ping(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "PING",
                    source,
                },
            )?)),
            MessageType::Pong => Ok(Self::Pong(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "PONG",
                    source,
                },
            )?)),
            MessageType::Bye => Ok(Self::Bye(serde_json::from_slice(&payload).map_err(
                |source| DecodeError::Json {
                    message_type: "BYE",
                    source,
                },
            )?)),
        }
    }

    /// Encode into a packet (payload CRC computed by the packet).
    pub fn to_packet(&self) -> Result<Packet, DecodeError> {
        fn to_json<T: Serialize>(
            message_type: &'static str,
            value: &T,
        ) -> Result<Bytes, DecodeError> {
            serde_json::to_vec(value)
                .map(Bytes::from)
                .map_err(|source| DecodeError::Json {
                    message_type,
                    source,
                })
        }
        match self {
            Self::HostHello(m) => Ok(Packet::new(
                MessageType::HostHello,
                0,
                to_json("HOST_HELLO", m)?,
            )?),
            Self::HelloAck(m) => Ok(Packet::new(
                MessageType::HelloAck,
                0,
                to_json("HELLO_ACK", m)?,
            )?),
            Self::Frame(m) => Ok(Packet::new(
                MessageType::Frame,
                0,
                Bytes::from(m.encode_binary()),
            )?),
            Self::State(m) => {
                let flags = if m.delta { STATE_DELTA_FLAG } else { 0 };
                Ok(Packet::new(
                    MessageType::State,
                    flags,
                    to_json("STATE", m)?,
                )?)
            }
            Self::Command(m) => Ok(Packet::new(
                MessageType::Command,
                0,
                to_json("COMMAND", m)?,
            )?),
            Self::CommandResult(m) => Ok(Packet::new(
                MessageType::CommandResult,
                0,
                to_json("COMMAND_RESULT", m)?,
            )?),
            Self::Event(m) => Ok(Packet::new(MessageType::Event, 0, to_json("EVENT", m)?)?),
            Self::MediaManifest(m) => Ok(Packet::new(
                MessageType::MediaManifest,
                0,
                to_json("MEDIA_MANIFEST", m)?,
            )?),
            Self::LogChunk(m) => Ok(Packet::new(MessageType::LogChunk, 0, encode_log_chunk(m))?),
            Self::Error(m) => Ok(Packet::new(MessageType::Error, 0, to_json("ERROR", m)?)?),
            Self::Ping(m) => Ok(Packet::new(MessageType::Ping, 0, to_json("PING", m)?)?),
            Self::Pong(m) => Ok(Packet::new(MessageType::Pong, 0, to_json("PONG", m)?)?),
            Self::Bye(m) => Ok(Packet::new(MessageType::Bye, 0, to_json("BYE", m)?)?),
        }
    }

    /// Encode to a full frame (header + payload).
    pub fn encode_frame(&self) -> Result<Bytes, DecodeError> {
        Ok(self.to_packet()?.encode())
    }
}

fn encode_log_chunk(chunk: &LogChunk) -> Bytes {
    let mut out = Vec::with_capacity(9 + chunk.text.len());
    out.extend_from_slice(&chunk.timestamp_ms.to_le_bytes());
    out.push(chunk.level as u8);
    out.extend_from_slice(chunk.text.as_bytes());
    Bytes::from(out)
}

fn decode_log_chunk(bytes: &[u8]) -> Result<LogChunk, DecodeError> {
    if bytes.len() < 9 {
        return Err(DecodeError::Binary {
            message_type: "LOG_CHUNK",
            reason: format!("truncated log chunk: {} bytes", bytes.len()),
        });
    }
    let timestamp_ms = u64::from_le_bytes(bytes[0..8].try_into().expect("slice len"));
    let level = LogLevel::from_u8(bytes[8]).ok_or_else(|| DecodeError::Binary {
        message_type: "LOG_CHUNK",
        reason: format!("invalid log level {}", bytes[8]),
    })?;
    let text = std::str::from_utf8(&bytes[9..])
        .map_err(|e| DecodeError::Binary {
            message_type: "LOG_CHUNK",
            reason: format!("invalid UTF-8: {e}"),
        })?
        .to_string();
    Ok(LogChunk {
        timestamp_ms,
        level,
        text,
    })
}
