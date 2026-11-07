//! Stable, shared error taxonomy for the watchdog host.
//!
//! Every error surfaced by the CLI, the web UI API, the MCP server and the
//! integration API is an [`WatchdogError`]: a stable machine-readable
//! [`ErrorCode`], a human-readable message and an optional actionable hint.
//! Codes are part of the public contract and are documented in
//! `docs/api.md`; they never change meaning once published.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable machine-readable error codes.
///
/// Serialized as `SCREAMING_SNAKE_CASE` strings (e.g. `PORT_IN_USE`).
/// Adding a new variant is a backwards-compatible change; renaming or
/// removing a variant is a breaking change and requires an API version bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // Configuration / startup
    /// The configuration file exists but could not be parsed or validated.
    ConfigInvalid,
    /// The configuration file was requested but does not exist.
    ConfigNotFound,
    /// A required data-directory path is not writable.
    DataDirNotWritable,
    /// A requested TCP port is already bound by another process.
    PortInUse,

    // I/O and internal
    /// Generic filesystem or network I/O failure.
    IoError,
    /// An internal invariant was violated; this is a bug.
    Internal,

    // ADB / device layer
    /// No device matched the request.
    DeviceNotFound,
    /// The device is in `unauthorized` state (USB debugging prompt pending).
    DeviceUnauthorized,
    /// The device is in `offline` state or stopped answering.
    DeviceOffline,
    /// Device pairing failed (bad code, pairing port closed, TLS failure).
    PairingFailed,
    /// The ADB transport (USB/TCP) failed.
    TransportError,
    /// The remote end rejected or closed the stream unexpectedly.
    StreamClosed,
    /// ADB protocol violation or unsupported peer behaviour.
    ProtocolError,
    /// The device and host speak incompatible protocol versions.
    ProtocolVersionMismatch,
    /// A package install failed.
    InstallFailed,
    /// The wireless-debugging or TCP connection dropped.
    ConnectionLost,

    // App / provisioning
    /// The watchdog app on the device is missing or its version does not match.
    AppVersionMismatch,
    /// A provisioning step failed; the hint names the step and remediation.
    ProvisioningFailed,

    // Media / analysis
    /// A media item was not found in the index.
    MediaNotFound,
    /// Integrity verification (size or hash) failed after transfer.
    MediaIntegrityMismatch,
    /// The requested analyzer does not exist or its parameters were rejected.
    AnalyzerError,
    /// Analysis failed while running.
    AnalysisFailed,

    // Scheduler
    /// The schedule expression could not be parsed or is unsatisfiable.
    ScheduleInvalid,
    /// The job is not known.
    JobNotFound,

    // Request validation / auth
    /// A request failed validation (bad parameters, out-of-range values).
    InvalidRequest,
    /// A resource was not found (generic, when not covered above).
    NotFound,
    /// Authentication failed or is required (bad/missing bearer token).
    Unauthorized,
    /// The caller is authenticated but not allowed to perform the action.
    Forbidden,
    /// The request exceeded a configured size or rate limit.
    PayloadTooLarge,
    /// The request conflicts with current state (e.g. idempotency replay).
    Conflict,

    // Not implemented
    /// The requested feature exists but is not implemented in this build.
    NotImplemented,
}

impl ErrorCode {
    /// Stable string form, identical to the serialized representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::ConfigNotFound => "CONFIG_NOT_FOUND",
            Self::DataDirNotWritable => "DATA_DIR_NOT_WRITABLE",
            Self::PortInUse => "PORT_IN_USE",
            Self::IoError => "IO_ERROR",
            Self::Internal => "INTERNAL",
            Self::DeviceNotFound => "DEVICE_NOT_FOUND",
            Self::DeviceUnauthorized => "DEVICE_UNAUTHORIZED",
            Self::DeviceOffline => "DEVICE_OFFLINE",
            Self::PairingFailed => "PAIRING_FAILED",
            Self::TransportError => "TRANSPORT_ERROR",
            Self::StreamClosed => "STREAM_CLOSED",
            Self::ProtocolError => "PROTOCOL_ERROR",
            Self::ProtocolVersionMismatch => "PROTOCOL_VERSION_MISMATCH",
            Self::InstallFailed => "INSTALL_FAILED",
            Self::ConnectionLost => "CONNECTION_LOST",
            Self::AppVersionMismatch => "APP_VERSION_MISMATCH",
            Self::ProvisioningFailed => "PROVISIONING_FAILED",
            Self::MediaNotFound => "MEDIA_NOT_FOUND",
            Self::MediaIntegrityMismatch => "MEDIA_INTEGRITY_MISMATCH",
            Self::AnalyzerError => "ANALYZER_ERROR",
            Self::AnalysisFailed => "ANALYSIS_FAILED",
            Self::ScheduleInvalid => "SCHEDULE_INVALID",
            Self::JobNotFound => "JOB_NOT_FOUND",
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::NotFound => "NOT_FOUND",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::Conflict => "CONFLICT",
            Self::NotImplemented => "NOT_IMPLEMENTED",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The one error type used across CLI, API, MCP and internal layers.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct WatchdogError {
    /// Stable machine-readable code.
    pub code: ErrorCode,
    /// Human-readable description; never contains secrets.
    pub message: String,
    /// Optional actionable remediation hint.
    pub hint: Option<String>,
    /// Optional source error, kept out of serialized output.
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl WatchdogError {
    /// Create a new error with a code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            source: None,
        }
    }

    /// Attach an actionable hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Attach a source error for diagnostics.
    #[must_use]
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Serialize into the wire error body `{code, message, hint?}`.
    #[must_use]
    pub fn to_body(&self) -> ErrorBody {
        ErrorBody {
            code: self.code,
            message: self.message.clone(),
            hint: self.hint.clone(),
        }
    }
}

impl From<std::io::Error> for WatchdogError {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorCode::IoError, format!("I/O error: {err}")).with_source(err)
    }
}

impl From<serde_json::Error> for WatchdogError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(ErrorCode::IoError, format!("JSON error: {err}")).with_source(err)
    }
}

/// Serialized error body returned by the API and MCP layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Stable machine-readable code.
    pub code: ErrorCode,
    /// Human-readable description; never contains secrets.
    pub message: String,
    /// Optional actionable remediation hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, WatchdogError>;
