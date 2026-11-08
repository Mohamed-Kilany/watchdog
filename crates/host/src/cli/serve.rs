//! `watchdog serve` — starts the web UI.
//!
//! M3-T15 serves the minimal live test page: the phone camera as MJPEG plus
//! the rolling stats from the device frame hub. The full UI replaces it in M6;
//! the MCP and integration API servers land in M6/M9.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;

use watchdog_core::config::Config;
use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::paths::DataPaths;
use watchdog_device::connection::DeviceConnection;
use watchdog_device::hub::{FrameHub, HubConfig};
use watchdog_device::provision::{RestoreRecord, PACKAGE};
use watchdog_device::session::LiveSession;
use watchdog_ui_server::TestPageOptions;

/// Start the web UI on the configured UI port.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Target device (USB serial or `host[:port]`); omit when only one is connected.
    #[arg(long, value_name = "SERIAL")]
    pub serial: Option<String>,
}

/// Entry point.
pub fn run(args: &ServeArgs, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = watchdog_adb::AdbKeyPair::load_or_create(&paths.keys)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let (connection, id) = DeviceConnection::connect(&keys, args.serial.as_deref()).await?;
        let token = RestoreRecord::load(&paths, &id)?
            .and_then(|record| record.token)
            .ok_or_else(|| {
                let target = args.serial.as_deref().unwrap_or(id.as_str());
                WatchdogError::new(
                    ErrorCode::NotFound,
                    format!("no session token recorded for {id}"),
                )
                .with_hint(format!(
                    "run `watchdog device provision {target}` first; the test page reuses its session token"
                ))
            })?;
        let (live, ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        if !ack.app.package.is_empty() && ack.app.package != PACKAGE {
            return Err(WatchdogError::new(
                ErrorCode::AppVersionMismatch,
                format!(
                    "the app at localabstract:watchdog-{token} is {}, expected {PACKAGE}",
                    ack.app.package
                ),
            ));
        }
        tracing::info!(
            device = %id,
            app = %ack.app.version_name,
            "opened the app session for the test page"
        );
        println!(
            "device {id}: {} {} (app {} versionCode {}, protocol {})",
            ack.device.manufacturer,
            ack.device.model,
            ack.app.version_name,
            ack.app.version_code,
            ack.protocol_version
        );
        let hub = Arc::new(FrameHub::spawn(live, HubConfig::default()));
        let bind = SocketAddr::new(cfg.ui_listen, cfg.ui_port);
        println!("watchdog test page: http://{bind}/");
        if cfg.token.is_some() {
            println!(
                "bearer token enabled: append ?token=<token> or send Authorization: Bearer <token>"
            );
        }
        println!("press Ctrl-C to stop");
        watchdog_ui_server::serve_test_page(
            TestPageOptions {
                bind,
                token: cfg.token.clone(),
            },
            hub,
        )
        .await
    })
}
