//! CLI subcommand implementations.

pub mod analyze;
pub mod capture;
pub mod config;
pub mod device;
pub mod mcp;
pub mod media;
pub mod schedule;
pub mod serve;

use watchdog_core::error::{ErrorCode, WatchdogError};

/// Every stub reports which milestone delivers it — never a silent no-op.
pub(crate) fn not_implemented(feature: &str, milestone: &str) -> WatchdogError {
    WatchdogError::new(
        ErrorCode::NotImplemented,
        format!("{feature} is not implemented yet"),
    )
    .with_hint(format!(
        "tracked as part of {milestone} (see TASK.md and docs/PROGRESS.md)"
    ))
}
