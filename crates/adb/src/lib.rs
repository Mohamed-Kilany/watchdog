//! watchdog-adb — native ADB protocol implementation (no external adb).
//!
//! Crate layout:
//! - [`packet`] — wire framing (`A_*` constants, incremental decoder)
//! - [`transport`] — the [`Transport`](transport::Transport) trait all IO flows through
//! - [`lifecycle`] — close-reason bookkeeping
//! - [`error`] — error taxonomy mapped to the shared watchdog codes
//!
//! Normative references: AOSP `packages/modules/adb` (OVERVIEW.TXT, protocol.txt,
//! adb.h, types.h) — cited throughout. See `docs/device-setup.md`.

#![warn(missing_docs)]

pub mod auth;
pub mod connection;
pub mod discovery;
pub mod error;
#[allow(unused_imports)]
use std::sync::Arc;
pub mod forward;
pub mod install;
pub mod lifecycle;
pub mod manager;
pub mod packet;
pub mod pairing;
pub mod pairing_flow;
pub mod shell;
pub mod stls;
pub mod sync;
pub mod tcp;
pub mod transport;
pub mod usb;

pub use auth::AdbKeyPair;
pub use connection::{Connection, Stream, StreamWriter, OPEN_TIMEOUT};
pub use discovery::{discover, discover_default, DiscoveredDevice};
pub use error::{AdbError, Result};
pub use forward::{start_forward, ForwardHandle};
pub use install::{install_apk, install_failure_hint, InstallOptions, InstallOutcome};
pub use manager::{connect_tcp, connect_usb, list_usb, refresh_properties};
pub use manager::{DeviceManager, DeviceState, DeviceSummary, DeviceTable, GoneReason};
pub use packet::{AuthType, Command, IncrementalDecoder, Packet};
pub use pairing::{derive_gcm_key, Spake2, SpakeRole};
pub use pairing_flow::{pair, pair_with_password, PairingOutcome};
pub use stls::{upgrade_stls, LanTransport, StlsTransport, STLS_VERSION};
pub use tcp::TcpTransport;
pub use transport::Transport;
pub use usb::{
    enumerate as enumerate_usb_devices, explain_usb_problems, UsbDeviceInfo, UsbTransport,
};
