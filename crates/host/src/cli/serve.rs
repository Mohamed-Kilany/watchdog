//! `watchdog serve` — starts the UI, MCP and integration API servers.

use clap::Args;

use watchdog_core::config::Config;

use super::not_implemented;

/// Start all three servers (web UI :7700, MCP :7701, integration API :7702).
#[derive(Debug, Args)]
pub struct ServeArgs {}

/// Entry point (lands with the server milestones).
pub fn run(_args: &ServeArgs, cfg: &Config) -> watchdog_core::Result<()> {
    let _ = cfg;
    Err(not_implemented(
        "serve (UI + MCP + integration API)",
        "M3/M6/M9 — see docs/PROGRESS.md",
    ))
}
