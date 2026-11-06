//! `watchdog` — host CLI entry point.

#![warn(missing_docs)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use watchdog_core::config::{Config, ConfigOverrides};
use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::{logging, paths};
use watchdog_protocol::PROTOCOL_VERSION;

mod cli;

use cli::{analyze, capture, config, device, mcp, media, schedule, serve};

/// watchdog — turns an Android phone into a remotely controlled, observable camera.
#[derive(Debug, Parser)]
#[command(name = "watchdog", version, about, long_about = None, propagate_version = true)]
struct Cli {
    /// Path to watchdog.toml (overrides the platform config location).
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Override the data directory (media, db, logs, keys, tools).
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Bearer token required on all ports (strongly recommended for LAN exposure).
    #[arg(long, global = true, value_name = "TOKEN")]
    token: Option<String>,

    /// Log level: trace|debug|info|warn|error.
    #[arg(long, global = true, value_name = "LEVEL")]
    log_level: Option<String>,

    /// Web UI port (default 7700).
    #[arg(long, global = true, value_name = "PORT")]
    ui_port: Option<u16>,

    /// MCP server port (default 7701).
    #[arg(long, global = true, value_name = "PORT")]
    mcp_port: Option<u16>,

    /// Integration API port (default 7702).
    #[arg(long, global = true, value_name = "PORT")]
    api_port: Option<u16>,

    /// Web UI bind address (default 127.0.0.1; 0.0.0.0 exposes the LAN).
    #[arg(long, global = true, value_name = "ADDR")]
    ui_listen: Option<String>,

    /// MCP bind address.
    #[arg(long, global = true, value_name = "ADDR")]
    mcp_listen: Option<String>,

    /// Integration API bind address.
    #[arg(long, global = true, value_name = "ADDR")]
    api_listen: Option<String>,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    fn overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            config_file: self.config.clone(),
            ui_port: self.ui_port,
            mcp_port: self.mcp_port,
            api_port: self.api_port,
            ui_listen: self.ui_listen.clone(),
            mcp_listen: self.mcp_listen.clone(),
            api_listen: self.api_listen.clone(),
            token: self.token.clone(),
            data_dir: self.data_dir.clone(),
            log_level: self.log_level.clone(),
        }
    }
}

/// Top-level subcommands. Every command has `--help`.
#[derive(Debug, Subcommand)]
enum Command {
    /// Start the web UI, MCP and integration API servers.
    Serve(serve::ServeArgs),
    /// Device management (list, connect, pair, provision, shell, logs, …).
    #[command(subcommand)]
    Device(device::DeviceCommand),
    /// Turn an Android phone into a live, controllable camera for screen testing.
    Capture(capture::CaptureArgs),
    /// Analyze captured media (analyzers, OTT rollup reports).
    #[command(subcommand)]
    Analyze(analyze::AnalyzeCommand),
    /// Recurring and one-off capture+analysis jobs.
    #[command(subcommand)]
    Schedule(schedule::ScheduleCommand),
    /// Manage the media library (list, export, delete).
    #[command(subcommand)]
    Media(media::MediaCommand),
    /// Run the MCP server directly (stdio transport).
    Mcp(mcp::McpArgs),
    /// Inspect and validate the configuration.
    #[command(subcommand)]
    Config(config::ConfigCommand),
    /// Print diagnostics: paths, writability, ports, versions.
    Doctor,
    /// Print host, protocol and (embedded) app versions.
    Version,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            print_error(&err);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> watchdog_core::Result<()> {
    match &cli.command {
        // Commands that do not need config/logging initialisation first.
        Command::Version => {
            println!("watchdog host {}", env!("CARGO_PKG_VERSION"));
            println!("protocol      {PROTOCOL_VERSION}");
            println!("embedded APK  not embedded (release builds embed it; see docs/release.md)");
            Ok(())
        }
        Command::Config(cmd) => config::run(cmd, &cli.overrides()),
        Command::Doctor => doctor(&cli.overrides()),
        _ => {
            let cfg = Config::load(&cli.overrides())?;
            let paths = paths::DataPaths::resolve(&cfg.data_dir);
            paths.create_all()?;
            if let Some(token) = &cfg.token {
                logging::register_secret(token);
            }
            let _guard = logging::init(&cfg.log_level, cfg.log_json, Some(&paths.logs))?;
            tracing::info!(data_dir = %cfg.data_dir.display(), "watchdog starting");
            dispatch(&cli, &cfg)
        }
    }
}

fn dispatch(cli: &Cli, cfg: &Config) -> watchdog_core::Result<()> {
    match &cli.command {
        Command::Serve(args) => serve::run(args, cfg),
        Command::Device(cmd) => device::run(cmd, cfg),
        Command::Capture(args) => capture::run(args, cfg),
        Command::Analyze(cmd) => analyze::run(cmd, cfg),
        Command::Schedule(cmd) => schedule::run(cmd, cfg),
        Command::Media(cmd) => media::run(cmd, cfg),
        Command::Mcp(args) => mcp::run(args, cfg),
        Command::Config(_) | Command::Doctor | Command::Version => unreachable!("handled above"),
    }
}

fn print_error(err: &WatchdogError) {
    eprintln!("error [{}]: {}", err.code, err.message);
    if let Some(hint) = &err.hint {
        eprintln!("hint: {hint}");
    }
}

/// `watchdog doctor` — diagnostics and write-permission checks.
fn doctor(overrides: &ConfigOverrides) -> watchdog_core::Result<()> {
    println!(
        "watchdog doctor — {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("host version     {}", env!("CARGO_PKG_VERSION"));
    println!("protocol version {PROTOCOL_VERSION}");
    println!();

    // Resolve config first; doctor must report config failures, not panic.
    let cfg = match Config::load(overrides) {
        Ok(cfg) => cfg,
        Err(err) => {
            println!(
                "config           INVALID — error [{}]: {}",
                err.code, err.message
            );
            if let Some(hint) = &err.hint {
                println!("                 hint: {hint}");
            }
            return Err(err);
        }
    };
    println!(
        "config file      {}",
        cfg.config_file.as_deref().map_or_else(
            || "(none — using defaults)".to_string(),
            |p| p.display().to_string()
        )
    );
    println!("data dir         {}", cfg.data_dir.display());
    if cfg.is_lan_exposed() {
        println!("LAN exposure     WARNING: at least one port binds a non-loopback address");
        if cfg.token.is_none() {
            println!("                 WARNING: no bearer token set (--token / WATCHDOG_TOKEN / token in watchdog.toml); see docs/security.md");
        }
    } else {
        println!("LAN exposure     none (all ports loopback)");
    }
    println!();

    println!("paths (create + write probe):");
    let paths = paths::DataPaths::resolve(&cfg.data_dir);
    let mut failures = 0u32;
    for (label, dir) in paths.writable_targets() {
        match paths::check_writable(&dir) {
            Ok(()) => println!("  {label:<8} ok    {}", dir.display()),
            Err(err) => {
                failures += 1;
                println!("  {label:<8} FAIL  {} ({})", dir.display(), err.message);
            }
        }
    }
    println!();

    println!("ports (would bind now):");
    for (label, listen, port) in [
        ("ui", cfg.ui_listen, cfg.ui_port),
        ("mcp", cfg.mcp_listen, cfg.mcp_port),
        ("api", cfg.api_listen, cfg.api_port),
    ] {
        match TcpListener::bind((listen, port)) {
            Ok(listener) => {
                drop(listener);
                println!("  {label:<4} ok    {listen}:{port} free");
            }
            Err(err) => {
                println!("  {label:<4} BUSY  {listen}:{port} — {err} (change with the corresponding flag, e.g. --{label}-port)");
            }
        }
    }
    println!();

    println!("ffmpeg           {}", ffmpeg_status(&cfg));
    println!(
        "system adb       not used (watchdog ships its own native ADB; see docs/device-setup.md)"
    );

    if failures > 0 {
        return Err(WatchdogError::new(
            ErrorCode::DataDirNotWritable,
            format!("{failures} path(s) are not writable; see output above"),
        ));
    }
    Ok(())
}

fn ffmpeg_status(cfg: &Config) -> String {
    if let Some(path) = &cfg.ffmpeg_path {
        return format!("debug override {}", path.display());
    }
    "not embedded yet (bundles in M7; here only the debug-only ffmpeg_path override is honored)"
        .to_string()
}
