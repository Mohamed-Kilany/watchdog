//! `watchdog analyze` — run analyzers and read reports.

use clap::Subcommand;

use watchdog_core::config::Config;

use super::not_implemented;

/// Analysis subcommands. The engine lands with M7.
#[derive(Debug, Subcommand)]
pub enum AnalyzeCommand {
    /// Analyze a media item (or file already in the library).
    Run {
        /// Media id from the library.
        media_id: String,
        /// Comma-separated analyzer ids (default: the configured profile).
        #[arg(long, value_name = "IDS")]
        analyzers: Option<String>,
        /// Analysis profile name.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Print the full JSON report to stdout.
        #[arg(long)]
        json: bool,
    },
    /// Print a stored report.
    Report {
        /// Report id.
        report_id: String,
        /// Output format: json or html.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// List available analyzers and their parameter schemas.
    ListAnalyzers {
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Entry point.
pub fn run(cmd: &AnalyzeCommand, _cfg: &Config) -> watchdog_core::Result<()> {
    let _ = cmd;
    Err(not_implemented(
        "analysis engine and reports",
        "M7 (docs/PROGRESS.md)",
    ))
}
