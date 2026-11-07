//! mDNS discovery for wireless-debugging endpoints (M2-T06).
//!
//! Android publishes two service types while wireless debugging is on:
//! - `_adb-tls-connect._tcp` — the main TLS connect port
//! - `_adb-tls-pairing._tcp` — the one-shot pairing port
//!
//! [`discover`] browses both for a bond (or a timeout) and reports every
//! endpoint with its device GUID (TXT `dev=<guid>`), host and port.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};

use crate::error::Result;

/// A discovered wireless-debugging endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    /// Device GUID from the TXT record (`dev=<guid>`).
    pub guid: String,
    /// Hostname (e.g. `1a2b3c4d`).
    pub host: String,
    /// Connect endpoint (`_adb-tls-connect`).
    pub connect: Option<SocketAddr>,
    /// Pairing endpoint (`_adb-tls-pairing`).
    pub pairing: Option<SocketAddr>,
    /// Human-readable name (may be empty).
    pub name: String,
}

/// Browse both ADB mDNS services for `timeout`.
pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredDevice>> {
    let daemon = ServiceDaemon::new()
        .map_err(|e| crate::error::AdbError::Transport(format!("mDNS daemon start failed: {e}")))?;

    let receiver = daemon.browse("_adb-tls-connect._tcp.local.").map_err(|e| {
        crate::error::AdbError::Transport(format!("mDNS browse connect failed: {e}"))
    })?;
    daemon.browse("_adb-tls-pairing._tcp.local.").map_err(|e| {
        crate::error::AdbError::Transport(format!("mDNS browse pairing failed: {e}"))
    })?;

    let mut by_guid: HashMap<String, DiscoveredDevice> = HashMap::new();
    let start = tokio::time::Instant::now();

    loop {
        let remaining = timeout
            .checked_sub(start.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if remaining.is_zero() {
            break;
        }
        let result = tokio::time::timeout(remaining, receiver.recv_async()).await;
        let event = match result {
            Ok(event) => event,
            Err(_) => break,
        };
        match event {
            ServiceEvent::ServiceFound(_service_type, info) => {
                let guid = info
                    .get_property("dev")
                    .map(|s| s.val().to_string())
                    .unwrap_or_default();
                let name = info.get_hostname().to_string();
                let host = info.get_host_name().to_string();
                let port = info.get_port();
                let candidates = info.get_addresses();
                let addr = candidates
                    .iter()
                    .copied()
                    .next()
                    .map(|ip| SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port));
                let service_type_str = info.get_type().to_string();

                let entry = by_guid
                    .entry(guid.clone())
                    .or_insert_with(|| DiscoveredDevice {
                        guid: guid.clone(),
                        host: name.clone(),
                        connect: None,
                        pairing: None,
                        name: host,
                    });
                if service_type_str.contains("connect") {
                    entry.connect = addr;
                } else if service_type_str.contains("pairing") {
                    entry.pairing = addr;
                }
            }
            ServiceEvent::ServiceRemoved(..) => {}
            ServiceEvent::SearchStarted(_) => {}
            ServiceEvent::SearchStopped(_) => {}
            ServiceEvent::ServiceResolved(_) => {}
            ServiceEvent::ServiceUpdated(..) => {}
            ServiceEvent::BrowserStopped(_) => {}
        }
    }

    daemon.shutdown().ok();
    Ok(by_guid.into_values().collect())
}

/// Convenience: browse for 5 seconds.
pub async fn discover_default() -> Result<Vec<DiscoveredDevice>> {
    discover(Duration::from_secs(5)).await
}
