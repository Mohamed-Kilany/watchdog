//! `watchdog mcp` — MCP server entry (stdio transport for spawned clients).

use clap::Args;

use watchdog_core::config::Config;

use super::not_implemented;

/// Run the MCP server on stdio (for clients that spawn the binary).
///
/// Without `--stdio` this is equivalent to the MCP port served by `serve`.
#[derive(Debug, Args)]
pub struct McpArgs {
    /// Serve MCP over stdio instead of the Streamable HTTP port.
    #[arg(long)]
    pub stdio: bool,
}

/// Entry point.
pub fn run(args: &McpArgs, _cfg: &Config) -> watchdog_core::Result<()> {
    let _ = args;
    Err(not_implemented("MCP server", "M9 (docs/PROGRESS.md)"))
}
