//! In-memory fake ADB device for tests.
//!
//! This crate implements just enough of the ADB wire protocol to exercise
//! watchdog-adb without hardware:
//! - canned CNXN/AUTH handshakes with or without authorization,
//! - OPEN/OKAY/WRTE/CLSE stream behavior scripted per service,
//! - later milestones extend it with shell v2, sync and install flows.
//!
//! It is a dev-dependency of `watchdog-adb`; it is never linked into the
//! host binary.

#![warn(missing_docs)]

mod mock;

pub use mock::{MockReader, MockTap, MockTransport, MockWriter, ScriptStep};
