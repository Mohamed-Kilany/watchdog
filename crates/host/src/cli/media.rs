//! `watchdog media` — the media library.

use clap::Subcommand;

use watchdog_core::config::Config;

use super::not_implemented;

/// Media library subcommands. The store lands with M5.
#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// List items in the library.
    List {
        /// Filter by device serial.
        #[arg(long, value_name = "SERIAL")]
        device: Option<String>,
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print a media item's metadata (or export it to a path).
    Get {
        /// Media id.
        media_id: String,
        /// Copy the file to this path instead of printing metadata.
        #[arg(long, value_name = "PATH")]
        output: Option<std::path::PathBuf>,
    },
    /// Export one or more items as individual files or a zip.
    Export {
        /// Media ids.
        #[arg(required = true)]
        media_ids: Vec<String>,
        /// Destination zip file.
        #[arg(long, value_name = "ZIP")]
        output: std::path::PathBuf,
    },
    /// Delete items from the library (files removed from disk).
    Rm {
        /// Media ids.
        #[arg(required = true)]
        media_ids: Vec<String>,
    },
}

/// Entry point.
pub fn run(cmd: &MediaCommand, _cfg: &Config) -> watchdog_core::Result<()> {
    let _ = cmd;
    Err(not_implemented("media library", "M5 (docs/PROGRESS.md)"))
}
