//! Device manager: discovery, zero-touch provisioning, app lifecycle. See TASK.md M3.

#![warn(missing_docs)]

/// Embedded release APK metadata and local-APK overrides (M3-T12).
pub mod apk;
/// Connected device abstraction over USB and TCP (M3-T13).
pub mod connection;
/// Frame receiver, sequencer and bounded broadcast hub (M3-T14).
pub mod hub;
/// Zero-touch provisioning, status and settings restore (M3-T13/T16).
pub mod provision;
/// Host side of an authenticated app session (M3-T13).
pub mod session;
