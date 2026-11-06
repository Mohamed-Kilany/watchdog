//! `watchdog capture` — still/burst/record from the host.

use clap::{Args, Subcommand};

use watchdog_core::config::Config;

use super::not_implemented;

/// Capture commands. Still/burst/record land with M5.
#[derive(Debug, Args)]
pub struct CaptureArgs {
    #[command(subcommand)]
    pub command: CaptureCommand,
}

/// Capture subcommands.
#[derive(Debug, Subcommand)]
pub enum CaptureCommand {
    /// Take a full-resolution still.
    Still {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Also fetch the file to this path (default: media library only).
        #[arg(long, value_name = "PATH")]
        output: Option<std::path::PathBuf>,
    },
    /// Take N stills at a fixed interval (or as fast as possible).
    Burst {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Number of frames.
        #[arg(long, default_value_t = 10)]
        count: u32,
        /// Interval between frames in milliseconds (0 = as fast as possible).
        #[arg(long, default_value_t = 100)]
        interval_ms: u64,
        /// Capture from the live stream frames instead of full-res stills (cheap path).
        #[arg(long)]
        from_stream: bool,
    },
    /// Start or stop video recording on the device.
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },
}

/// `capture record` subcommands.
#[derive(Debug, Subcommand)]
pub enum RecordAction {
    /// Start recording (continues on the device even if the link drops).
    Start {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Hard limit in seconds (0 = no limit).
        #[arg(long, default_value_t = 0)]
        duration_secs: u64,
    },
    /// Stop recording and schedule the transfer to the host.
    Stop {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
}

/// Entry point.
pub fn run(args: &CaptureArgs, _cfg: &Config) -> watchdog_core::Result<()> {
    let _ = args;
    Err(not_implemented("capture (still/burst/record)", "M5 (docs/PROGRESS.md)"))
}
