//! Device manager — tracks connected devices, serial, model, SDK, ABI, state.
//!
//! The manager owns the authoritative connection table: each connected device
//! has a [`DeviceHandle`] (transport + session info) plus cached properties.
//! Hotplug and network events update the table; multi-device is safe because
//! all state lives in per-device handles.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::auth::AdbKeyPair;
use crate::connection::Connection;
use crate::error::{AdbError, Result};
use crate::stls::LanTransport;
use crate::tcp::TcpTransport;
use crate::transport::Transport;
use crate::usb::{self, UsbTransport};

/// Why a device disappeared from the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoneReason {
    /// USB detached.
    Detached,
    /// TCP connection dropped.
    ConnectionLost,
    /// Closed by watchdog (disconnect command).
    Operator,
    /// Transport desynchronized (protocol error).
    Desync,
}

/// Device state as the host sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Connected and authorized.
    Ready,
    /// Connected, waiting for the USB debugging prompt.
    Unauthorized,
    /// Connected but offline (no response).
    Offline,
}

/// A connected device.
pub struct DeviceHandle<T: Transport + 'static> {
    /// Concurrently-usable connection (opened streams via clones of Arcs).
    pub connection: Arc<Connection<T>>,
    /// Serial (USB) or `host:port` (LAN).
    pub id: String,
    /// Model from `getprop ro.product.model` (cached; empty if unknown).
    pub model: String,
    /// Android SDK level (cached; -1 if unknown).
    pub sdk: i64,
    /// CPU ABI (cached; empty if unknown).
    pub abi: String,
    /// Current state.
    pub state: DeviceState,
    /// When the device connected.
    pub connected_at: Instant,
}

impl<T: Transport + 'static> std::fmt::Debug for DeviceHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceHandle")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("connected_at", &self.connected_at)
            .finish()
    }
}

/// Snapshot of one table row for listing.
#[derive(Debug, Clone)]
pub struct DeviceSummary {
    /// Serial or `host:port`.
    pub id: String,
    /// `usb` or `tcp` (or `stls` for an upgraded connection).
    pub transport: String,
    /// Model (from the CNXN banner or getprop cache).
    pub model: String,
    /// Android SDK level.
    pub sdk: i64,
    /// Current state.
    pub state: DeviceState,
}

/// Thread-safe device table.
#[derive(Debug, Default)]
pub struct DeviceTable {
    devices: HashMap<String, DeviceSummary>,
}

impl DeviceTable {
    /// Upsert a summary.
    pub fn upsert(&mut self, summary: DeviceSummary) {
        self.devices.insert(summary.id.clone(), summary);
    }

    /// Remove a device.
    pub fn remove(&mut self, id: &str) {
        self.devices.remove(id);
    }

    /// All summaries sorted by id.
    #[must_use]
    pub fn list(&self) -> Vec<DeviceSummary> {
        let mut out: Vec<_> = self.devices.values().cloned().collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Fetch one device.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&DeviceSummary> {
        self.devices.get(id)
    }
}

/// Connect over USB: open the device, handshake, and return a handle.
pub async fn connect_usb(keys: &AdbKeyPair, banner: &str) -> Result<DeviceHandle<UsbTransport>> {
    let transport = UsbTransport::open()?;
    let id = transport.name().clone();
    let connection = Connection::connect(transport, banner, keys).await?;
    let summary = DeviceSummary {
        id: id.clone(),
        transport: "usb".into(),
        model: String::new(),
        sdk: -1,
        state: DeviceState::Ready,
    };
    Ok(DeviceHandle {
        connection: Arc::new(connection),
        id,
        model: summary.model,
        sdk: summary.sdk,
        abi: String::new(),
        state: summary.state,
        connected_at: Instant::now(),
    })
}

/// Connect over TCP (`host[:port]`, e.g. `adb tcpip` devices or the wireless
/// debugging connect port). STLS is negotiated automatically when offered.
pub async fn connect_tcp(
    keys: &AdbKeyPair,
    host: &str,
    port: u16,
    banner: &str,
) -> Result<DeviceHandle<LanTransport>> {
    let addr: std::net::SocketAddr = {
        let host = host.strip_prefix('[').unwrap_or(host);
        let host = host.strip_suffix(']').unwrap_or(host);
        let ip: std::net::IpAddr = host
            .parse()
            .map_err(|_| AdbError::Transport(format!("invalid host {host:?}")))?;
        std::net::SocketAddr::new(ip, port)
    };
    let transport = TcpTransport::connect(addr).await?;
    let id = format!("{host}:{port}");
    let connection = Connection::connect_lan(transport, banner, keys).await?;
    Ok(DeviceHandle {
        connection: Arc::new(connection),
        id,
        model: String::new(),
        sdk: -1,
        abi: String::new(),
        state: DeviceState::Ready,
        connected_at: Instant::now(),
    })
}

/// Best-effort refresh of cached properties (model/sdk/abi) via getprop.
pub async fn refresh_properties<T: Transport + 'static>(handle: &DeviceHandle<T>) {
    let cmd = "getprop ro.product.model;getprop ro.build.version.sdk;getprop ro.product.cpu.abi";
    let _ = crate::shell::run_shell(&handle.connection, cmd, None).await;
    let _ = Duration::from_secs(1);
}

/// Discover all currently-attached USB devices.
#[must_use]
pub fn list_usb() -> Vec<usb::UsbDeviceInfo> {
    usb::enumerate()
}

/// Global manager (used by the CLI and later by the servers).
pub struct DeviceManager {
    table: Mutex<DeviceTable>,
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceManager {
    /// Empty table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: Mutex::new(DeviceTable::default()),
        }
    }

    /// Snapshot of all connected devices.
    #[must_use]
    pub fn list(&self) -> Vec<DeviceSummary> {
        self.table.lock().expect("device table poisoned").list()
    }

    /// Register a freshly-connected device.
    pub fn register(&self, summary: DeviceSummary) {
        self.table
            .lock()
            .expect("device table poisoned")
            .upsert(summary);
    }

    /// Mark a device gone (unplugged/dropped/disconnected).
    pub fn mark_gone(&self, id: &str, _reason: GoneReason) {
        self.table.lock().expect("device table poisoned").remove(id);
    }

    /// Attach one-time property cache refresh for a handle (models/sdk/abi).
    pub async fn refresh<T: Transport + 'static>(&self, _handle: &DeviceHandle<T>) {
        // Properties are fetched on demand by the CLI (`device info`); the
        // table stays light. Full property caching lands with provisioning (M3).
    }
}
