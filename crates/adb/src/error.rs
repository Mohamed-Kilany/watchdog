//! Error taxonomy for the adb crate, mapped to watchdog's stable codes.

use watchdog_core::error::{ErrorCode, WatchdogError};

/// ADB-specific failures; converted into the shared taxonomy at the boundary.
#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    /// The device rejected our key (AUTH_RSAPUBLICKEY then failure) — prompt or approve on device.
    #[error("device authorization required (approve the USB debugging prompt on the device)")]
    Unauthorized,

    /// CNXN version we offered is not supported by the peer.
    #[error("protocol version mismatch: {0}")]
    VersionMismatch(String),

    /// Magic field of the packet header did not equal `command ^ 0xffffffff`.
    #[error("bad packet magic (command 0x{command:08x}, magic 0x{magic:08x})")]
    BadMagic {
        /// Command id from the header.
        command: u32,
        /// Magic bytes received.
        magic: u32,
    },

    /// Payload length exceeds the negotiated maximum.
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(usize),

    /// Underlying transport I/O failure.
    #[error("transport error: {0}")]
    Transport(String),

    /// Peer closed the connection or a stream unexpectedly.
    #[error("stream closed")]
    StreamClosed,

    /// Timeout waiting for the peer.
    #[error("timeout: {0}")]
    Timeout(String),

    /// Invalid data received from the peer (not a protocol violation of ours).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Pairing failed (wrong code, TLS error, peer info mismatch).
    #[error("pairing failed: {0}")]
    PairingFailed(String),

    /// Any other I/O-shaped failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<AdbError> for WatchdogError {
    fn from(err: AdbError) -> Self {
        let (code, hint) = match &err {
            AdbError::Unauthorized => (
                ErrorCode::DeviceUnauthorized,
                Some("tap Allow in the USB debugging dialog on the device (check 'always allow' to make it permanent); watchdog uses its own RSA key in <data_dir>/keys".to_string()),
            ),
            AdbError::VersionMismatch(_) => (ErrorCode::ProtocolVersionMismatch, None),
            AdbError::BadMagic { .. } | AdbError::Protocol(_) => (ErrorCode::ProtocolError, None),
            AdbError::PayloadTooLarge(_) => (ErrorCode::InvalidRequest, None),
            AdbError::Transport(_) => (ErrorCode::TransportError, Some("check the cable, udev rules (Linux) or the WinUSB driver (Windows); run `watchdog device usb-doctor`".to_string())),
            AdbError::StreamClosed => (ErrorCode::StreamClosed, None),
            AdbError::Timeout(_) => (ErrorCode::ConnectionLost, None),
            AdbError::PairingFailed(_) => (ErrorCode::PairingFailed, Some("re-open 'Pair device with pairing code' on the device and retry; codes are single-use".to_string())),
            AdbError::Io(_) => (ErrorCode::IoError, None),
        };
        let mut out = WatchdogError::new(code, err.to_string());
        out.hint = hint;
        out
    }
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, AdbError>;
