//! `watchdog config` — inspect and validate the layered configuration.

use std::io::Write as _;

use clap::Subcommand;

use watchdog_core::config::{Config, ConfigOverrides};
use watchdog_core::paths;

/// Configuration inspection commands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration with the source of each key.
    Show,
    /// Print the resolved config file path and whether it exists.
    Path,
    /// Validate the configuration (file, env, CLI layers) and report.
    Validate,
}

/// Entry point.
pub fn run(cmd: &ConfigCommand, overrides: &ConfigOverrides) -> watchdog_core::Result<()> {
    match cmd {
        ConfigCommand::Path => {
            let resolved = overrides
                .config_file
                .clone()
                .or_else(|| std::env::var_os(watchdog_core::config::CONFIG_PATH_ENV).map(Into::into))
                .unwrap_or_else(paths::config_file_path);
            let exists = resolved.exists();
            println!("{}", resolved.display());
            println!("exists: {}", if exists { "yes" } else { "no (defaults will be used)" });
            Ok(())
        }
        ConfigCommand::Show => {
            let cfg = Config::load(overrides)?;
            println!("# watchdog configuration (token values are never printed)");
            let mut stdout = std::io::stdout().lock();
            let mut out = Vec::new();
            for (key, value, source) in cfg.redacted_rows() {
                let _ = writeln!(out, "{key:<24} {value:<48} # {source}");
            }
            stdout.write_all(&out)?;
            if let Some(file) = &cfg.config_file {
                println!("\nconfig file: {}", file.display());
            } else {
                println!("\nconfig file: (none — using built-in defaults)");
            }
            if cfg.is_lan_exposed() {
                println!("WARNING: LAN exposure enabled; all ports accept non-loopback connections. Ensure a token is set (see docs/security.md).");
            }
            Ok(())
        }
        ConfigCommand::Validate => {
            let cfg = Config::load(overrides)?;
            paths::check_writable(&paths::DataPaths::resolve(&cfg.data_dir).root)?;
            println!("ok: configuration is valid");
            println!("data dir: {}", cfg.data_dir.display());
            println!(
                "ports: ui={} mcp={} api={} (listen {} / {} / {})",
                cfg.ui_port, cfg.mcp_port, cfg.api_port, cfg.ui_listen, cfg.mcp_listen, cfg.api_listen
            );
            Ok(())
        }
    }
}
