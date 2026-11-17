//! `watchdog serve` — starts the web UI.
//!
//! The host embeds the static UI build and serves it together with the device
//! management API, the live MJPEG stream and stats. The MCP and integration API
//! servers land in M6/M9.
//!
//! The server starts without a device: the devices page can pair, provision and
//! activate one. `--serial` activates a device at startup when it is already
//! provisioned.

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Args;

use watchdog_core::config::Config;
use watchdog_core::paths::DataPaths;
use watchdog_device::devices::DeviceService;
use watchdog_ui_server::UiOptions;

/// Start the web UI on the configured UI port.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Activate this device at startup (USB serial or `host[:port]`).
    #[arg(long, value_name = "SERIAL")]
    pub serial: Option<String>,
}

/// Entry point.
pub fn run(args: &ServeArgs, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let service = Arc::new(DeviceService::new(paths)?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let bind = SocketAddr::new(cfg.ui_listen, cfg.ui_port);
        if let Some(serial) = &args.serial {
            match service.activate(serial).await {
                Ok(device) => {
                    println!(
                        "device {}: {} {} (app {} versionCode {}, protocol {})",
                        device.serial,
                        device.manufacturer,
                        device.model,
                        device.app_version_name,
                        device.app_version_code,
                        device.protocol_version
                    );
                }
                Err(error) => {
                    println!("could not activate {serial}: {}", error.message);
                    if let Some(hint) = &error.hint {
                        println!("hint: {hint}");
                    }
                    println!("the devices page can pair, provision and activate it");
                }
            }
        }
        println!("watchdog web UI: http://{bind}/");
        if watchdog_ui_server::embedded_asset_count() == 0 {
            println!(
                "this build has no embedded UI; serving the minimal live page (build ui/ and rebuild the host)"
            );
        }
        if cfg.token.is_some() {
            println!(
                "bearer token enabled: append ?token=<token> or send Authorization: Bearer <token>"
            );
        }
        println!("press Ctrl-C to stop");
        watchdog_ui_server::serve_ui(
            UiOptions {
                bind,
                token: cfg.token.clone(),
            },
            service,
        )
        .await
    })
}
