//! One connected device (USB or TCP) with the operations provisioning needs.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use watchdog_adb::install::{self, PackageInfo};
use watchdog_adb::manager;
use watchdog_adb::shell::{self, ShellOutput};
use watchdog_adb::{
    AdbKeyPair, Connection, ForwardHandle, InstallOptions, InstallOutcome, LanTransport,
    UsbTransport,
};
use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::Result;

/// A connected ADB device, either over USB or wireless debugging.
///
/// The enum exists so provisioning code is written once instead of being
/// generic over every `Transport` implementation.
#[derive(Clone)]
pub enum DeviceConnection {
    /// USB transport.
    Usb(Arc<Connection<UsbTransport>>),
    /// TCP transport (wireless debugging with STLS, or a plain-TCP emulator).
    Tcp(Arc<Connection<LanTransport>>),
}

impl DeviceConnection {
    /// Connect to `target` (USB serial or `host[:port]`); `None` picks the only
    /// attached USB device. Returns the connection and the device id.
    pub async fn connect(keys: &AdbKeyPair, target: Option<&str>) -> Result<(Self, String)> {
        match target {
            Some(target) if is_tcp_target(target) => {
                let (host, port) = split_host_port(target)?;
                let handle = manager::connect_tcp(keys, &host, port, "host::watchdog").await?;
                Ok((Self::Tcp(handle.connection), handle.id))
            }
            Some(serial) => {
                let transport = UsbTransport::open_by_serial(serial)?;
                let id = transport.name();
                let connection =
                    Arc::new(Connection::connect(transport, "host::watchdog", keys).await?);
                Ok((Self::Usb(connection), id))
            }
            None => {
                let handle = manager::connect_usb(keys, "host::watchdog").await?;
                Ok((Self::Usb(handle.connection), handle.id))
            }
        }
    }

    /// Run one shell command with shell v2 (stdout, stderr and exit code).
    pub async fn shell(&self, command: &str) -> Result<ShellOutput> {
        match self {
            Self::Usb(connection) => shell::run_shell(connection, command, None)
                .await
                .map_err(Into::into),
            Self::Tcp(connection) => shell::run_shell(connection, command, None)
                .await
                .map_err(Into::into),
        }
    }

    /// Stream-install an APK.
    pub async fn install(&self, apk: &Path, options: &InstallOptions) -> Result<InstallOutcome> {
        match self {
            Self::Usb(connection) => install::install_apk(connection, apk, options, None)
                .await
                .map_err(Into::into),
            Self::Tcp(connection) => install::install_apk(connection, apk, options, None)
                .await
                .map_err(Into::into),
        }
    }

    /// Query an installed package (versionCode and signing digests).
    pub async fn query_package(&self, package: &str) -> Result<Option<PackageInfo>> {
        match self {
            Self::Usb(connection) => install::query_package(connection, package)
                .await
                .map_err(Into::into),
            Self::Tcp(connection) => install::query_package(connection, package)
                .await
                .map_err(Into::into),
        }
    }

    /// Uninstall a package and return the device output.
    pub async fn uninstall(&self, package: &str) -> Result<String> {
        match self {
            Self::Usb(connection) => install::uninstall_package(connection, package)
                .await
                .map_err(Into::into),
            Self::Tcp(connection) => install::uninstall_package(connection, package)
                .await
                .map_err(Into::into),
        }
    }

    /// Upload a file over the `sync:` service.
    pub async fn push(&self, local: &Path, remote: &str, mode: u32, mtime: u32) -> Result<u64> {
        match self {
            Self::Usb(connection) => {
                watchdog_adb::sync::push(connection, local, remote, mode, mtime, None)
                    .await
                    .map_err(Into::into)
            }
            Self::Tcp(connection) => {
                watchdog_adb::sync::push(connection, local, remote, mode, mtime, None)
                    .await
                    .map_err(Into::into)
            }
        }
    }

    /// Download a file over the `sync:` service.
    pub async fn pull(&self, remote: &str, local: &Path) -> Result<watchdog_adb::sync::PullResult> {
        match self {
            Self::Usb(connection) => watchdog_adb::sync::pull(connection, remote, local, None)
                .await
                .map_err(Into::into),
            Self::Tcp(connection) => watchdog_adb::sync::pull(connection, remote, local, None)
                .await
                .map_err(Into::into),
        }
    }

    /// Forward a host TCP listener to a device service.
    pub async fn forward(&self, listen: SocketAddr, service: String) -> Result<ForwardHandle> {
        match self {
            Self::Usb(connection) => {
                watchdog_adb::start_forward(Arc::clone(connection), listen, service)
                    .await
                    .map_err(Into::into)
            }
            Self::Tcp(connection) => {
                watchdog_adb::start_forward(Arc::clone(connection), listen, service)
                    .await
                    .map_err(Into::into)
            }
        }
    }
}

/// Whether a CLI target names a TCP endpoint rather than a USB serial.
#[must_use]
pub fn is_tcp_target(target: &str) -> bool {
    target.contains(':')
}

/// Split `host[:port]` (default 5555), accepting `[IPv6]:port`.
pub fn split_host_port(target: &str) -> Result<(String, u16)> {
    let (host, port) = target.rsplit_once(':').unwrap_or((target, "5555"));
    let port = port.parse::<u16>().map_err(|_| {
        WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("invalid port in target {target:?} (expected host[:port])"),
        )
    })?;
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_targets_are_detected_by_colon() {
        assert!(is_tcp_target("192.0.2.5:5555"));
        assert!(is_tcp_target("[::1]:5555"));
        assert!(!is_tcp_target("R5CT1234ABCD"));
    }

    #[test]
    fn host_port_defaults_and_parses() {
        assert_eq!(
            split_host_port("192.0.2.5").expect("host"),
            ("192.0.2.5".to_string(), 5555)
        );
        assert_eq!(
            split_host_port("phone.local:4444").expect("host"),
            ("phone.local".to_string(), 4444)
        );
        assert!(split_host_port("phone:not-a-port").is_err());
    }
}
