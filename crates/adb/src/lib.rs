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
pub mod error;
pub mod lifecycle;
pub mod packet;
pub mod transport;

pub use auth::AdbKeyPair;
pub use error::{AdbError, Result};
pub use packet::{AuthType, Command, IncrementalDecoder, Packet};
pub use transport::Transport;
