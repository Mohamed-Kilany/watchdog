//! `watchdog schedule` — recurring and one-off capture+analysis jobs.

use clap::Subcommand;

use watchdog_core::config::Config;

use super::not_implemented;

/// Scheduler subcommands. The engine lands with M8.
#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Add a job (cron expression or preset).
    Add {
        /// Job name, e.g. "nightly OTT check".
        #[arg(long, value_name = "NAME")]
        name: String,
        /// Cron expression, e.g. "*/30 * * * *".
        #[arg(long, value_name = "CRON")]
        cron: Option<String>,
        /// Friendly preset: every-30m, hourly, daily@HH:MM, weekdays@HH:MM.
        #[arg(long, value_name = "PRESET")]
        preset: Option<String>,
        /// Target device serial.
        #[arg(long, value_name = "SERIAL")]
        device: String,
        /// Record this many seconds of video per run.
        #[arg(long, default_value_t = 10)]
        record_secs: u64,
        /// Analysis profile to apply after capture.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// List jobs and their next run times.
    List {
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run a job immediately.
    Run {
        /// Job id.
        job_id: String,
    },
    /// Enable a job.
    Enable {
        /// Job id.
        job_id: String,
    },
    /// Disable a job.
    Disable {
        /// Job id.
        job_id: String,
    },
    /// Delete a job.
    Rm {
        /// Job id.
        job_id: String,
    },
}

/// Entry point.
pub fn run(cmd: &ScheduleCommand, _cfg: &Config) -> watchdog_core::Result<()> {
    let _ = cmd;
    Err(not_implemented("scheduler", "M8 (docs/PROGRESS.md)"))
}
