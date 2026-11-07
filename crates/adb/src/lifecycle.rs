//! Lifecycle hooks for transports (USB hotplug, network loss).

/// Why the transport closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// Local side asked to disconnect.
    LocalClose,
    /// Remote side closed the connection.
    RemoteClose,
    /// Underlying I/O error.
    IoError,
    /// The device was physically detached (USB).
    Detached,
    /// Authentication was rejected by the device.
    Unauthorized,
}
