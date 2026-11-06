//! watchdog-core — shared foundation for the watchdog host.
//!
//! Hosts the layered configuration system ([`config`]), the per-OS data
//! directory layout ([`paths`]), the structured logging setup with secret
//! redaction ([`logging`]) and the shared error taxonomy ([`error`]).

#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod logging;
pub mod paths;

pub use error::{ErrorBody, ErrorCode, Result, WatchdogError};
