//! `watchdog device` — device management (native ADB, no system adb).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::Instant;

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use watchdog_core::config::Config;
use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::paths::DataPaths;
use watchdog_device::apk::{self, ApkSource};
use watchdog_device::connection::DeviceConnection;
use watchdog_device::provision::{
    self, ProvisionOptions, ProvisionReport, RestoreRecord, StatusReport, StepStatus,
};
use watchdog_device::session::{LiveSession, Polled};
use watchdog_protocol::{
    ByeMessage, CommandMessage, CommandResultMessage, ControlEntry, ControlSource, ErrorMessage,
    HelloAck, LogChunk, LogLevel, Message, StateMessage,
};

use super::not_implemented;

/// Device management subcommands.
#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// List connected (USB) devices and discovered LAN devices.
    List(ListArgs),
    /// Discover wireless-debugging endpoints via mDNS.
    Discover,
    /// Connect to a device over USB or TCP (`host[:port]`).
    Connect {
        /// USB serial number, or `host[:port]` for LAN.
        target: String,
    },
    /// Pair a wireless-debugging endpoint (Android 11+).
    Pair {
        /// `host:port` of the pairing endpoint.
        target: String,
        /// Six-digit pairing code shown on the device.
        code: String,
    },
    /// Disconnect a device.
    Disconnect {
        /// USB serial or `host:port`; omit to disconnect all.
        target: Option<String>,
    },
    /// Show device info (model, SDK, ABI, state).
    Info {
        /// Target device; omit when only one is connected.
        target: Option<String>,
    },
    /// Run a shell command on the device.
    Shell {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Command and arguments (everything after `--`).
        #[arg(trailing_var_arg = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// Push a file to the device (adb sync).
    Push {
        /// Local file.
        local: PathBuf,
        /// Remote absolute path.
        remote: String,
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
    /// Pull a file from the device (adb sync).
    Pull {
        /// Remote path.
        remote: String,
        /// Local destination file.
        local: PathBuf,
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
    /// Install or update the embedded watchdog app on the device.
    Install {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Install a specific APK instead of the embedded one (debug only).
        #[arg(long, value_name = "APK")]
        apk: Option<PathBuf>,
    },
    /// Uninstall the watchdog app from the device.
    Uninstall {
        /// Package name (default: app.watchdog.remote).
        package: Option<String>,
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
    /// Forward a host port to a device socket (e.g. the app's abstract socket).
    Forward {
        /// Local port; 0 picks a free port.
        local_port: u16,
        /// Device service, e.g. `localabstract:watchdog-abc123` or `tcp:8080`.
        remote: String,
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
    /// Stream or tail the watchdog app log from the device.
    Logs {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Keep streaming and print new lines as they arrive.
        #[arg(long)]
        follow: bool,
    },
    /// Zero-touch provision: install, grant permissions, launch, verify.
    Provision {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Install a specific APK instead of the embedded one (debug only).
        #[arg(long, value_name = "APK")]
        apk: Option<PathBuf>,
        /// Skip handshake/first-frame verification after launching.
        #[arg(long)]
        no_verify: bool,
        /// Do not change screen settings (stay-on, keyguard).
        #[arg(long)]
        no_screen: bool,
        /// Seconds to wait for the app socket to appear.
        #[arg(long, value_name = "SECS", default_value_t = 20)]
        socket_timeout: u64,
        /// Seconds to wait for the first camera frame.
        #[arg(long, value_name = "SECS", default_value_t = 30)]
        frame_timeout: u64,
        /// Emit the provisioning report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show provisioning/app status for the device.
    Status {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Emit the status snapshot as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Revert host-side device changes made by provisioning
    /// (wake/stay-on settings), recorded per device.
    Restore {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
    },
    /// Print (or with --write, install) udev rules granting USB access on Linux.
    UdevRules {
        /// Write rules to /etc/udev/rules.d/51-android-watchdog.rules (needs root).
        #[arg(long)]
        write: bool,
    },
    /// Diagnose USB/ADB permission problems (udev, drivers, group membership).
    UsbDoctor,
    /// Dump the device's capability descriptor.
    Capabilities {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the live control state reported by the app.
    Controls {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
    /// Change one or more camera controls (`id=value`, repeatable).
    SetControl {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Assignments such as `iso=400`, `ae_lock=true`, `zoom_ratio=2.5`.
        #[arg(value_name = "ID=VALUE", required = true)]
        assignments: Vec<String>,
        /// Emit raw JSON.
        #[arg(long)]
        json: bool,
    },
    /// Probe every advertised CaptureRequest key and build a compatibility matrix.
    Probe {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Camera id to probe (switches the live camera first).
        #[arg(long, value_name = "ID")]
        camera: Option<String>,
        /// Probe only this key (canonical `android.*` name or CaptureRequest field).
        #[arg(long, value_name = "KEY")]
        key: Option<String>,
        /// Emit the raw JSON report instead of the Markdown matrix.
        #[arg(long)]
        json: bool,
        /// Write the report to a file instead of stdout.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Stream STATE messages (full snapshot + deltas) from the app.
    WatchState {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
        /// Number of STATE messages to print before exiting.
        #[arg(long, default_value_t = 10)]
        count: u32,
        /// Emit one raw JSON object per line.
        #[arg(long)]
        json: bool,
    },
}

/// Arguments for `device list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Include mDNS-discovered wireless-debugging endpoints.
    #[arg(long)]
    pub discover: bool,
    /// Emit raw JSON.
    #[arg(long)]
    pub json: bool,
}

/// Android USB vendor IDs commonly used by ADB devices.
///
/// Sources: AOSP `usb.ids`, vendor documentation. Community-maintained list;
/// unknown vendors can be added with MODE="0666" without a match.
const UDEV_VENDORS: &[(&str, &str)] = &[
    ("18d1", "Google"),
    ("04e8", "Samsung"),
    ("1004", "LG"),
    ("0bb4", "HTC"),
    ("054c", "Sony"),
    ("2717", "Xiaomi"),
    ("12d1", "Huawei"),
    ("2a70", "OnePlus"),
    ("22b8", "Motorola"),
    ("17ef", "Lenovo"),
    ("0955", "NVIDIA"),
    ("2257", "OPPO"),
    ("2e95", "HMD Global (Nokia)"),
    ("0489", "Foxconn (FAI)"),
    ("294c", "Fairphone"),
    ("1bbb", "ZTE"),
    ("05c6", "Qualcomm (reference)"),
    ("0e8d", "MediaTek (reference)"),
];

fn render_udev_rules() -> String {
    let mut out = String::from(
        "# watchdog — Android debug bridge USB access\n\
         # Install: sudo watchdog device udev-rules --write  (or copy to /etc/udev/rules.d/51-android-watchdog.rules)\n\
         # Then:    sudo udevadm control --reload-rules && sudo udevadm trigger\n\
         # Group-based access: add your user to plugdev and set GROUP=\"plugdev\" below instead of MODE=\"0666\".\n",
    );
    for (id, vendor) in UDEV_VENDORS {
        use std::fmt::Write as _;
        let _ = writeln!(
            out,
            "SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{id}\", MODE=\"0666\", GROUP=\"plugdev\", TAG+=\"uaccess\" # {vendor}"
        );
    }
    out
}

/// Entry point.
pub fn run(cmd: &DeviceCommand, cfg: &Config) -> watchdog_core::Result<()> {
    match cmd {
        DeviceCommand::UdevRules { write } => {
            let rules = render_udev_rules();
            if *write {
                if !cfg!(target_os = "linux") {
                    return Err(watchdog_core::WatchdogError::new(
                        watchdog_core::ErrorCode::InvalidRequest,
                        "udev rules only apply to Linux",
                    ));
                }
                let path = PathBuf::from("/etc/udev/rules.d/51-android-watchdog.rules");
                let mut file = std::fs::File::create(&path).map_err(|e| {
                    watchdog_core::WatchdogError::new(
                        watchdog_core::ErrorCode::IoError,
                        format!("cannot write {}: {e}", path.display()),
                    )
                    .with_hint("re-run with elevated privileges (sudo) or write the printed rules manually")
                    .with_source(e)
                })?;
                file.write_all(rules.as_bytes())?;
                println!("wrote {}", path.display());
                println!(
                    "reload with: sudo udevadm control --reload-rules && sudo udevadm trigger"
                );
            } else {
                print!("{rules}");
            }
            Ok(())
        }
        DeviceCommand::List(args) => cli_list(args, cfg),
        DeviceCommand::Discover => cli_discover(cfg),
        DeviceCommand::Connect { target } => cli_connect(target, cfg),
        DeviceCommand::Pair { target, code } => cli_pair(target, code, cfg),
        DeviceCommand::Shell { serial, command } => cli_shell(serial.as_deref(), command, cfg),
        DeviceCommand::Push {
            local,
            remote,
            serial,
        } => cli_push(serial.as_deref(), local, remote, cfg),
        DeviceCommand::Pull {
            remote,
            local,
            serial,
        } => cli_pull(serial.as_deref(), remote, local, cfg),
        DeviceCommand::Install { serial, apk } => {
            cli_install(serial.as_deref(), apk.as_deref(), cfg)
        }
        DeviceCommand::Uninstall { package, serial } => {
            cli_uninstall(serial.as_deref(), package.as_deref(), cfg)
        }
        DeviceCommand::Forward {
            local_port,
            remote,
            serial,
        } => cli_forward(serial.as_deref(), *local_port, remote, cfg),
        DeviceCommand::UsbDoctor => cli_usb_doctor(),
        DeviceCommand::Logs { serial, follow } => cli_logs(serial.as_deref(), *follow, cfg),
        DeviceCommand::Provision {
            serial,
            apk,
            no_verify,
            no_screen,
            socket_timeout,
            frame_timeout,
            json,
        } => cli_provision(
            serial.as_deref(),
            apk.as_deref(),
            !*no_verify,
            !*no_screen,
            *socket_timeout,
            *frame_timeout,
            *json,
            cfg,
        ),
        DeviceCommand::Status { serial, json } => cli_status(serial.as_deref(), *json, cfg),
        DeviceCommand::Restore { serial } => cli_restore(serial.as_deref(), cfg),
        DeviceCommand::Capabilities { serial, json } => {
            cli_capabilities(serial.as_deref(), *json, cfg)
        }
        DeviceCommand::Controls { serial, json } => cli_controls(serial.as_deref(), *json, cfg),
        DeviceCommand::SetControl {
            serial,
            assignments,
            json,
        } => cli_set_control(serial.as_deref(), assignments, *json, cfg),
        DeviceCommand::Probe {
            serial,
            camera,
            key,
            json,
            output,
        } => cli_probe(
            serial.as_deref(),
            camera.as_deref(),
            key.as_deref(),
            *json,
            output.as_deref(),
            cfg,
        ),
        DeviceCommand::WatchState {
            serial,
            count,
            json,
        } => cli_watch_state(serial.as_deref(), *count, *json, cfg),
        other => Err(match other {
            DeviceCommand::Disconnect { .. } | DeviceCommand::Info { .. } => not_implemented(
                "device session management in this CLI process",
                "M2-T13 (docs/PROGRESS.md)",
            ),
            _ => unreachable!("handled above"),
        }),
    }
}

fn adbs_key(cfg: &Config) -> Result<watchdog_adb::AdbKeyPair, watchdog_core::WatchdogError> {
    let paths = watchdog_core::paths::DataPaths::resolve(&cfg.data_dir);
    watchdog_adb::AdbKeyPair::load_or_create(&paths.keys).map_err(Into::into)
}

fn cli_list(args: &ListArgs, cfg: &Config) -> watchdog_core::Result<()> {
    let devices = watchdog_adb::list_usb();
    let mdns = if args.discover {
        futures_discover()
    } else {
        vec![]
    };
    if args.json {
        let usb: Vec<_> = devices
            .iter()
            .map(|d| serde_json::json!({"id": d.id(), "vendor": d.vendor_id, "product": d.product_id, "product_str": d.product}))
            .collect();
        let mdns_json: Vec<_> = mdns
            .iter()
            .map(|d| serde_json::json!({"guid": d.guid, "host": d.host, "connect": d.connect.map(|a| a.to_string()), "pairing": d.pairing.map(|a| a.to_string())}))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"usb": usb, "discovered": mdns_json})
            )?
        );
        return Ok(());
    }
    if devices.is_empty() && mdns.is_empty() {
        println!("No devices found.");
        println!("  usb: nothing (plug in a phone with USB debugging enabled; run `watchdog device usb-doctor`)");
        if !args.discover {
            println!(
                "  lan: run `watchdog device list --discover` for wireless-debugging endpoints"
            );
        }
    }
    for device in &devices {
        println!(
            "{}  {:04x}:{:04x}  {}",
            device.id(),
            device.vendor_id,
            device.product_id,
            device.product
        );
    }
    for discovered in &mdns {
        println!(
            "{}  connect={:?} pairing={:?}  {}",
            discovered.guid, discovered.connect, discovered.pairing, discovered.name
        );
    }
    let _ = cfg;
    Ok(())
}

fn futures_discover() -> Vec<watchdog_adb::DiscoveredDevice> {
    // Runtime built on demand for this one-shot command.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
        .and_then(|rt| rt.block_on(watchdog_adb::discover_default()).ok())
        .unwrap_or_default()
}

fn cli_discover(cfg: &Config) -> watchdog_core::Result<()> {
    let found = futures_discover();
    if found.is_empty() {
        println!("No wireless-debugging endpoints found (mDNS timeout).");
    } else {
        for device in found {
            println!(
                "{}  connect={:?} pairing={:?}",
                device.guid, device.connect, device.pairing
            );
        }
    }
    let _ = cfg;
    Ok(())
}

fn cli_connect(target: &str, cfg: &Config) -> watchdog_core::Result<()> {
    let keys = adbs_key(cfg)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let info = rt.block_on(async {
        if is_tcp_target(target) {
            let (host, port) = split_host_port(target)?;
            let handle =
                watchdog_adb::manager::connect_tcp(&keys, &host, port, "host::watchdog").await?;
            Ok::<_, watchdog_core::WatchdogError>(handle.connection.info.banner.clone())
        } else {
            let handle = watchdog_adb::manager::connect_usb(&keys, "host::watchdog").await?;
            Ok::<_, watchdog_core::WatchdogError>(handle.connection.info.banner.clone())
        }
    })?;
    println!("connected: {info}");
    Ok(())
}

fn is_tcp_target(target: &str) -> bool {
    target.contains(':') && target.chars().any(|c| c == '.' || c == ':')
}

fn split_host_port(target: &str) -> watchdog_core::Result<(String, u16)> {
    let (host, port_str) = target.rsplit_once(':').unwrap_or((target, "5555"));
    let port: u16 = port_str.parse().map_err(|_| {
        watchdog_core::WatchdogError::new(
            watchdog_core::ErrorCode::InvalidRequest,
            format!("invalid port in target {target:?} (expected host[:port])"),
        )
    })?;
    Ok((host.to_string(), port))
}

fn cli_pair(target: &str, code: &str, cfg: &Config) -> watchdog_core::Result<()> {
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("invalid pairing code {code:?} (expected 6 digits)"),
        ));
    }
    let (host, port) = split_host_port(target)?;
    let keys = adbs_key(cfg)?;
    let userkey = keys.public_key_userkey();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let outcome = rt.block_on(async {
        let stream = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| {
                WatchdogError::new(
                    ErrorCode::PairingFailed,
                    format!("pairing connect to {host}:{port} failed: {e}"),
                )
                .with_hint("open 'Pair device with pairing code' on the device and use its port")
            })?;
        watchdog_adb::pair(stream, code, &keys, &userkey)
            .await
            .map_err(|e: watchdog_adb::AdbError| WatchdogError::from(e))
    })?;
    println!("Paired with device {}", outcome.device_guid);
    Ok(())
}

fn cli_shell(serial: Option<&str>, command: &[String], cfg: &Config) -> watchdog_core::Result<()> {
    if command.is_empty() {
        return Err(watchdog_core::WatchdogError::new(
            watchdog_core::ErrorCode::InvalidRequest,
            "no command given (usage: watchdog device shell [--serial S] -- cmd…)",
        ));
    }
    let cmd = command.join(" ");
    let output = with_device(
        cfg,
        serial,
        |conn, _id| async move { conn.shell(&cmd).await },
    )?;
    print!("{}", output.stdout_string());
    let stderr = output.stderr_string();
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    std::process::exit(output.exit_code.unwrap_or(0));
}

fn cli_push(
    serial: Option<&str>,
    local: &Path,
    remote: &str,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let written = with_device(cfg, serial, |conn, _id| async move {
        conn.push(local, remote, 0o100644, current_mtime()).await
    })?;
    println!("pushed {written} bytes to {remote}");
    Ok(())
}

fn current_mtime() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

fn cli_pull(
    serial: Option<&str>,
    remote: &str,
    local: &Path,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let result = with_device(cfg, serial, |conn, _id| async move {
        conn.pull(remote, local).await
    })?;
    println!(
        "pulled {} bytes from {remote} (sha256 {})",
        result.bytes, result.sha256
    );
    Ok(())
}

fn cli_install(
    serial: Option<&str>,
    apk_path: Option<&Path>,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let source = apk::resolve(apk_path).map_err(|error| {
        WatchdogError::new(ErrorCode::NotFound, error.to_string()).with_hint(
            "build the host with WATCHDOG_APK set (CI release builds do), or pass --apk <path>",
        )
    })?;
    let apk_path = match &source {
        ApkSource::Local { path, .. } => path.clone(),
        ApkSource::Embedded(embedded) => {
            let paths = DataPaths::resolve(&cfg.data_dir);
            std::fs::create_dir_all(&paths.tmp)?;
            let path = paths
                .tmp
                .join(format!("watchdog-{}.apk", embedded.version_name));
            std::fs::write(&path, embedded.bytes)?;
            path
        }
    };
    let options = watchdog_adb::InstallOptions::default();
    let display_path = apk_path.clone();
    let outcome = with_device(cfg, serial, |conn, _id| async move {
        conn.install(&apk_path, &options).await
    })?;
    println!("{}", outcome.output);
    if !outcome.success {
        if let (Some(code), Some(hint)) = (&outcome.failure_code, &outcome.failure_hint) {
            return Err(watchdog_core::WatchdogError::new(
                watchdog_core::ErrorCode::InstallFailed,
                format!("install failed ({code})"),
            )
            .with_hint(hint.clone()));
        }
        return Err(watchdog_core::WatchdogError::new(
            watchdog_core::ErrorCode::InstallFailed,
            "install failed with unknown error",
        ));
    }
    println!("installed {}", display_path.display());
    Ok(())
}

fn cli_uninstall(
    serial: Option<&str>,
    package: Option<&str>,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let package = package.unwrap_or("app.watchdog.remote");
    let output = with_device(cfg, serial, |conn, _id| async move {
        conn.uninstall(package).await
    })?;
    println!("{output}");
    Ok(())
}

fn cli_forward(
    serial: Option<&str>,
    local_port: u16,
    remote: &str,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    with_device(cfg, serial, |conn, _id| async move {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{local_port}").parse().unwrap();
        let handle = conn.forward(addr, remote.to_string()).await?;
        println!(
            "forwarding 127.0.0.1:{} -> {}",
            handle.local_addr().port(),
            remote
        );
        println!("(press Ctrl-C to stop)");
        std::future::pending::<()>().await;
        unreachable!()
    })?;
    Ok(())
}

fn cli_usb_doctor() -> watchdog_core::Result<()> {
    for tip in watchdog_adb::explain_usb_problems() {
        println!("- {tip}");
    }
    Ok(())
}

fn device_runtime() -> watchdog_core::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

/// Connect to the selected device (USB serial or `host[:port]`) and run `f`
/// with the connection and its resolved device id.
fn with_device<T, F, Fut>(cfg: &Config, serial: Option<&str>, f: F) -> watchdog_core::Result<T>
where
    F: FnOnce(DeviceConnection, String) -> Fut,
    Fut: std::future::Future<Output = watchdog_core::Result<T>>,
{
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        f(connection, id).await
    })
}

#[allow(clippy::too_many_arguments)]
fn cli_provision(
    serial: Option<&str>,
    apk_path: Option<&Path>,
    verify_frame: bool,
    screen: bool,
    socket_timeout: u64,
    frame_timeout: u64,
    json: bool,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let options = ProvisionOptions {
        apk: apk_path.map(Path::to_path_buf),
        token: None,
        verify_frame,
        socket_timeout: Duration::from_secs(socket_timeout),
        handshake_timeout: Duration::from_secs(10),
        frame_timeout: Duration::from_secs(frame_timeout),
        skip_screen: !screen,
    };
    let report = with_device(cfg, serial, |connection, id| async move {
        Ok(provision::provision(&connection, &id, &paths, &options).await)
    })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    if let Some(step) = report.failure() {
        let mut error = WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            format!("provisioning failed at step {}: {}", step.name, step.detail),
        );
        if let Some(hint) = &step.hint {
            error = error.with_hint(hint.clone());
        }
        return Err(error);
    }
    Ok(())
}

fn print_report(report: &ProvisionReport) {
    println!("provisioning {}", report.serial);
    for step in &report.steps {
        let label = match step.status {
            StepStatus::Passed => "PASS",
            StepStatus::Failed => "FAIL",
            StepStatus::Skipped => "SKIP",
        };
        println!(
            "  [{label}] {:<12} {:>6} ms  {}",
            step.name, step.duration_ms, step.detail
        );
        if let Some(hint) = &step.hint {
            println!("         hint: {hint}");
        }
    }
    if let Some(token) = &report.token {
        println!("session token: {token}");
    }
    if report.passed() {
        println!("provisioning complete");
    } else {
        println!("provisioning failed");
    }
}

fn cli_status(serial: Option<&str>, json: bool, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let report = with_device(cfg, serial, |connection, id| async move {
        provision::status(&connection, &id, &paths).await
    })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_status(&report);
    }
    Ok(())
}

fn print_status(status: &StatusReport) {
    println!("serial:  {}", status.serial);
    println!("device:  {}", status.props.summary());
    match (
        status.version_code,
        &status.expected_version_name,
        status.expected_version_code,
    ) {
        (Some(installed), Some(name), Some(expected)) => {
            println!("app:     installed versionCode {installed}; embedded {name} ({expected})");
            match status.up_to_date {
                Some(true) => println!("         up to date"),
                Some(false) => println!("         OUTDATED - run `watchdog device provision`"),
                None => {}
            }
        }
        (Some(installed), _, _) => {
            println!("app:     installed versionCode {installed} (no embedded APK in this build)")
        }
        (None, _, _) => println!("app:     not installed - run `watchdog device provision`"),
    }
    println!(
        "service: {}",
        status
            .service_pid
            .map_or_else(|| "not running".to_string(), |pid| format!("pid {pid}"))
    );
    println!(
        "appops:  CAMERA={}, RECORD_AUDIO={}",
        status.appops_camera.as_deref().unwrap_or("unknown"),
        status.appops_record_audio.as_deref().unwrap_or("unknown")
    );
    println!(
        "token:   {}",
        status.token.as_deref().unwrap_or("(none recorded)")
    );
    if let Some(record) = &status.restore {
        if record.originals.is_empty() {
            println!("restore: no settings changed");
        } else {
            println!(
                "restore: {} recorded setting(s); `watchdog device restore` reverts them",
                record.originals.len()
            );
        }
    }
}

fn cli_restore(serial: Option<&str>, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let (id, steps) = with_device(cfg, serial, |connection, id| async move {
        let steps = provision::restore(&connection, &id, &paths).await?;
        Ok((id, steps))
    })?;
    if steps.is_empty() {
        println!("no recorded settings to restore on {id}");
        return Ok(());
    }
    for step in &steps {
        println!(
            "{} {} = {}",
            if step.applied { "restored" } else { "FAILED  " },
            step.setting,
            step.value
        );
    }
    println!("restored {} setting(s) on {id}", steps.len());
    Ok(())
}

fn cli_logs(serial: Option<&str>, follow: bool, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "logs")?;
        let (mut live, ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        println!(
            "# connected to {id} (app {} versionCode {})",
            ack.app.version_name, ack.app.version_code
        );
        let idle = Duration::from_millis(1000);
        loop {
            match live.session().poll(idle).await? {
                Polled::Message(message) => match *message {
                    Message::LogChunk(chunk) => print_log_chunk(&chunk),
                    Message::Error(error) => {
                        eprintln!("app error: {} ({})", error.message, error.code);
                    }
                    Message::Bye(bye) => {
                        println!("# app closed the session: {}", bye.reason);
                        break;
                    }
                    _ => {}
                },
                Polled::Timeout => {
                    if !follow {
                        break;
                    }
                }
                Polled::Closed => {
                    println!("# app closed the connection");
                    break;
                }
            }
        }
        Ok(())
    })
}

fn cli_capabilities(serial: Option<&str>, json: bool, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "capabilities")?;
        let (live, ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        // The handshake is the whole exchange; closing releases the app's slot.
        drop(live);
        if json {
            println!("{}", serde_json::to_string_pretty(&ack.capabilities)?);
        } else {
            print!("{}", render_capabilities(&id, &ack));
        }
        Ok(())
    })
}

fn session_token(paths: &DataPaths, id: &str, command: &str) -> watchdog_core::Result<String> {
    RestoreRecord::load(paths, id)?
        .and_then(|record| record.token)
        .ok_or_else(|| {
            WatchdogError::new(
                ErrorCode::NotFound,
                format!("no session token recorded for {id}"),
            )
            .with_hint(format!(
                "run `watchdog device provision` first; {command} reuses its session token"
            ))
        })
}

/// Timeout for one control command round trip.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// A full key probe can take a few minutes on a slow HAL.
const PROBE_TIMEOUT: Duration = Duration::from_secs(240);
/// Camera switches rebuild the capture session; allow the app to settle.
const CAMERA_SWITCH_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for the STATE delta carrying actual values after a change.
const SETTLE_TIMEOUT: Duration = Duration::from_millis(750);
/// Idle timeout between STATE messages while watching.
const STATE_TIMEOUT: Duration = Duration::from_secs(5);

fn cli_controls(serial: Option<&str>, json: bool, cfg: &Config) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "controls")?;
        let (mut live, _ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        let state = request_state(&mut live, COMMAND_TIMEOUT).await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&state_value(&state)?)?);
        } else {
            print!("{}", render_state(&id, &state));
        }
        Ok(())
    })
}

fn cli_set_control(
    serial: Option<&str>,
    assignments: &[String],
    json: bool,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let parsed = assignments
        .iter()
        .map(|raw| parse_assignment(raw))
        .collect::<watchdog_core::Result<Vec<_>>>()?;
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "set-control")?;
        let (mut live, _ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        let command_id = next_command_id("set");
        let controls = parsed
            .iter()
            .map(|(control, value)| json!({ "id": control, "value": value }))
            .collect::<Vec<_>>();
        let started = Instant::now();
        live.session()
            .send(&Message::Command(CommandMessage {
                id: command_id.clone(),
                command: "set_controls".to_string(),
                params: json!({ "controls": controls }),
            }))
            .await?;
        let result = await_command_result(&mut live, &command_id, COMMAND_TIMEOUT).await?;
        let elapsed = started.elapsed();
        let ids = parsed
            .iter()
            .map(|(control, _)| control.clone())
            .collect::<Vec<_>>();
        let actuals = collect_actuals(&mut live, &ids, SETTLE_TIMEOUT).await;
        if json {
            let value = json!({
                "round_trip_ms": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                "results": &result.results,
                "error": &result.error,
                "actual": actuals,
            });
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            print!("{}", render_set_result(&id, elapsed, &result, &actuals));
        }
        let failures = result.results.iter().filter(|item| !item.ok).count()
            + if result.error.is_some() { 1 } else { 0 };
        if failures > 0 {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                format!("{failures} control assignment(s) were rejected"),
            ));
        }
        Ok(())
    })
}

/// Probe every advertised request key (M4-T10) and render a matrix.
fn cli_probe(
    serial: Option<&str>,
    camera: Option<&str>,
    key: Option<&str>,
    json: bool,
    output: Option<&Path>,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "probe")?;
        let (mut live, _ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        if let Some(camera) = camera {
            let command_id = next_command_id("probe-camera");
            live.session()
                .send(&Message::Command(CommandMessage {
                    id: command_id.clone(),
                    command: "set_controls".to_string(),
                    params: json!({ "controls": [{ "id": "camera_id", "value": camera }] }),
                }))
                .await?;
            let result = await_command_result(&mut live, &command_id, COMMAND_TIMEOUT).await?;
            if result.error.is_some() || result.results.iter().any(|item| !item.ok) {
                let detail = result
                    .results
                    .iter()
                    .find(|item| !item.ok)
                    .and_then(|item| item.message.clone())
                    .or(result.error.clone())
                    .unwrap_or_else(|| "camera switch was rejected".to_string());
                return Err(WatchdogError::new(ErrorCode::InvalidRequest, detail));
            }
            let state = request_state(&mut live, CAMERA_SWITCH_TIMEOUT).await?;
            if state.camera_id != camera {
                return Err(WatchdogError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "camera switch did not take effect (device reports {})",
                        state.camera_id
                    ),
                ));
            }
        }
        let command_id = next_command_id("probe");
        let started = Instant::now();
        let params = key.map_or_else(|| json!({}), |key| json!({ "key": key }));
        live.session()
            .send(&Message::Command(CommandMessage {
                id: command_id.clone(),
                command: "probe_keys".to_string(),
                params,
            }))
            .await?;
        let result = await_command_result(&mut live, &command_id, PROBE_TIMEOUT).await?;
        if let Some(error) = result.error.as_deref() {
            return Err(WatchdogError::new(ErrorCode::InvalidRequest, error));
        }
        let item = result
            .results
            .first()
            .ok_or_else(|| WatchdogError::new(ErrorCode::Internal, "probe returned no result"))?;
        if !item.ok {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                item.message
                    .clone()
                    .unwrap_or_else(|| "probe was rejected".to_string()),
            ));
        }
        let report = item
            .applied
            .as_ref()
            .ok_or_else(|| WatchdogError::new(ErrorCode::Internal, "probe returned no report"))?;
        if let Some(error) = report.get("error").and_then(Value::as_str) {
            return Err(WatchdogError::new(ErrorCode::InvalidRequest, error));
        }
        let text = if json {
            serde_json::to_string_pretty(report)?
        } else {
            render_probe_matrix(&id, report)
        };
        match output {
            Some(path) => {
                std::fs::write(path, &text)?;
                println!(
                    "wrote {} ({} bytes, {} ms)",
                    path.display(),
                    text.len(),
                    started.elapsed().as_millis()
                );
            }
            None => println!("{text}"),
        }
        Ok(())
    })
}

/// Render the raw probe report as the per-device compatibility matrix.
fn render_probe_matrix(serial: &str, report: &Value) -> String {
    use std::fmt::Write as _;

    let empty = json!({});
    let device = report.get("device").unwrap_or(&empty);
    let model = device
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown device");
    let android = device
        .get("android")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let sdk = device.get("sdk").and_then(Value::as_i64).unwrap_or(0);
    let app = device
        .get("app_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let camera_id = report
        .get("camera_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let facing = report
        .get("facing")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let level = report
        .get("hardware_level")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let entries = report
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut accepted = 0;
    let mut ignored = 0;
    let mut errors = 0;
    let mut skipped = 0;
    let mut unprobed = 0;
    for entry in &entries {
        match entry.get("status").and_then(Value::as_str) {
            Some("skipped") => skipped += 1,
            Some("unprobed") => unprobed += 1,
            Some("probed") => {
                for value in entry
                    .get("values")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match value.get("outcome").and_then(Value::as_str) {
                        Some("accepted") => accepted += 1,
                        Some("ignored") => ignored += 1,
                        Some("error") => errors += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Control compatibility matrix — {model} (Android {android}, SDK {sdk})\n"
    );
    let _ = writeln!(out, "- Serial: `{serial}`");
    let _ = writeln!(
        out,
        "- Camera: `{camera_id}` ({facing}), hardware level {level}"
    );
    let _ = writeln!(out, "- App: `{app}`");
    let _ = writeln!(
        out,
        "- Request keys: {} available, {} probed; result keys: {} available",
        report
            .get("available_request_keys")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("probed_keys")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("available_result_keys")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    let _ = writeln!(
        out,
        "- Values: {accepted} accepted, {ignored} ignored, {errors} error"
    );
    let _ = writeln!(out, "- Keys: {skipped} skipped, {unprobed} unprobed");
    let _ = writeln!(
        out,
        "- Aborted: {}\n",
        match report.get("aborted").and_then(Value::as_bool) {
            Some(true) => report
                .get("abort_reason")
                .and_then(Value::as_str)
                .unwrap_or("yes"),
            _ => "no",
        }
    );
    let _ = writeln!(
        out,
        "`accepted` = the request was submitted and the capture result reflected the value; \
         `ignored` = the request was submitted but the result did not reflect it; \
         `error` = the HAL rejected the request or no result arrived.\n"
    );

    let mut sorted = entries;
    sorted.sort_by(|left, right| {
        left.get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("key").and_then(Value::as_str).unwrap_or(""))
    });
    let _ = writeln!(
        out,
        "| Key | Requested | Label | Outcome | Reflected | Result | ms |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for entry in &sorted {
        if entry.get("status").and_then(Value::as_str) != Some("probed") {
            continue;
        }
        let key = entry.get("key").and_then(Value::as_str).unwrap_or("?");
        for value in entry
            .get("values")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let requested = value
                .get("requested")
                .map_or_else(|| "null".to_string(), Value::to_string);
            let label = value.get("label").and_then(Value::as_str).unwrap_or("");
            let outcome = value
                .get("outcome")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let reflected = value
                .get("reflected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let result = if outcome == "error" {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map_or_else(|| "—".to_string(), |error| format!("error: {error}"))
            } else {
                value
                    .get("result")
                    .map_or_else(|| "—".to_string(), Value::to_string)
            };
            let elapsed = value.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0);
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | {} | {} | `{}` | {} |",
                md(key),
                md(&requested),
                md(label),
                outcome,
                if reflected { "yes" } else { "no" },
                md(&result),
                elapsed
            );
        }
    }

    let _ = writeln!(out, "\n## Keys not probed\n");
    let _ = writeln!(out, "| Key | Status | Reason |");
    let _ = writeln!(out, "|---|---|---|");
    for entry in &sorted {
        let status = entry.get("status").and_then(Value::as_str).unwrap_or("");
        if status == "probed" {
            continue;
        }
        let key = entry.get("key").and_then(Value::as_str).unwrap_or("?");
        let reason = entry.get("reason").and_then(Value::as_str).unwrap_or("");
        let _ = writeln!(out, "| `{}` | {} | {} |", md(key), status, md(reason));
    }

    if let Some(missing) = report
        .get("not_advertised")
        .and_then(Value::as_array)
        .filter(|missing| !missing.is_empty())
    {
        let _ = writeln!(out, "\n## Known keys not advertised by this camera\n");
        for entry in missing {
            let field = entry.get("field").and_then(Value::as_str).unwrap_or("?");
            let key = entry.get("key").and_then(Value::as_str).unwrap_or("");
            let _ = writeln!(out, "- `{field}` (`{key}`)");
        }
    }
    out
}

fn md(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn cli_watch_state(
    serial: Option<&str>,
    count: u32,
    json: bool,
    cfg: &Config,
) -> watchdog_core::Result<()> {
    let count = count.max(1);
    let paths = DataPaths::resolve(&cfg.data_dir);
    let keys = adbs_key(cfg)?;
    let rt = device_runtime()?;
    rt.block_on(async {
        let (connection, id) = DeviceConnection::connect(&keys, serial).await?;
        let token = session_token(&paths, &id, "watch-state")?;
        let (mut live, ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            Duration::from_secs(10),
            Duration::from_secs(5),
        )
        .await?;
        // Ask for a full snapshot immediately; the app also sends one on connect.
        live.session()
            .send(&Message::Command(CommandMessage {
                id: next_command_id("watch"),
                command: "get_state".to_string(),
                params: json!({}),
            }))
            .await?;
        if !json {
            println!(
                "# connected to {id} (app {}), waiting for {count} STATE message(s)",
                ack.app.version_name
            );
        }
        let mut seen = 0u32;
        while seen < count {
            match live.session().poll(STATE_TIMEOUT).await? {
                Polled::Message(message) => match *message {
                    Message::State(state) => {
                        seen += 1;
                        print_state(&id, &state, json)?;
                    }
                    Message::Error(error) => return Err(app_session_error(&error)),
                    Message::Bye(bye) => {
                        if !json {
                            println!("# app closed the session: {}", bye.reason);
                        }
                        break;
                    }
                    _ => {}
                },
                Polled::Timeout => return Err(state_timeout()),
                Polled::Closed => {
                    if !json {
                        println!("# app closed the connection");
                    }
                    break;
                }
            }
        }
        Ok(())
    })
}

async fn request_state(
    live: &mut LiveSession,
    timeout: Duration,
) -> watchdog_core::Result<StateMessage> {
    live.session()
        .send(&Message::Command(CommandMessage {
            id: next_command_id("state"),
            command: "get_state".to_string(),
            params: json!({}),
        }))
        .await?;
    await_full_state(live, timeout).await
}

/// Wait for a full STATE snapshot, folding any deltas that arrive first.
async fn await_full_state(
    live: &mut LiveSession,
    timeout: Duration,
) -> watchdog_core::Result<StateMessage> {
    let deadline = Instant::now() + timeout;
    let mut merged = empty_state();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(state_timeout());
        }
        match live.session().poll(remaining).await? {
            Polled::Message(message) => match *message {
                Message::State(state) => {
                    if !state.delta {
                        return Ok(state);
                    }
                    merge_state(&mut merged, &state);
                }
                Message::Error(error) => return Err(app_session_error(&error)),
                Message::Bye(bye) => return Err(bye_error(&bye)),
                _ => {}
            },
            Polled::Timeout => {}
            Polled::Closed => {
                return Err(WatchdogError::new(
                    ErrorCode::StreamClosed,
                    "app closed the session while waiting for STATE",
                ))
            }
        }
    }
}

async fn await_command_result(
    live: &mut LiveSession,
    command_id: &str,
    timeout: Duration,
) -> watchdog_core::Result<CommandResultMessage> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(WatchdogError::new(
                ErrorCode::ConnectionLost,
                format!("timed out waiting for COMMAND_RESULT {command_id}"),
            )
            .with_hint("check that the app is still streaming and the camera is open"));
        }
        match live.session().poll(remaining).await? {
            Polled::Message(message) => match *message {
                Message::CommandResult(result) if result.id == command_id => return Ok(result),
                Message::Error(error) => return Err(app_session_error(&error)),
                Message::Bye(bye) => return Err(bye_error(&bye)),
                _ => {}
            },
            Polled::Timeout => {}
            Polled::Closed => {
                return Err(WatchdogError::new(
                    ErrorCode::StreamClosed,
                    "app closed the session before acknowledging the command",
                ))
            }
        }
    }
}

/// Best-effort read of the actual values that follow a control change.
///
/// The app sends a STATE delta immediately after COMMAND_RESULT, before the
/// first frame produced with the new request, so that delta still carries the
/// previous actuals. Drain the whole settle window and keep the latest value
/// seen for each id instead of returning on the first snapshot.
async fn collect_actuals(
    live: &mut LiveSession,
    ids: &[String],
    timeout: Duration,
) -> BTreeMap<String, Value> {
    let deadline = Instant::now() + timeout;
    let mut out = BTreeMap::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match live.session().poll(remaining).await {
            Ok(Polled::Message(message)) => {
                if let Message::State(state) = *message {
                    for (id, entry) in state.state {
                        if ids.contains(&id) {
                            if let Some(actual) = entry.actual {
                                out.insert(id, actual);
                            }
                        }
                    }
                }
            }
            Ok(Polled::Timeout | Polled::Closed) | Err(_) => break,
        }
    }
    out
}

fn merge_state(base: &mut StateMessage, delta: &StateMessage) {
    base.seq = delta.seq;
    base.timestamp_ms = delta.timestamp_ms;
    base.camera_id.clone_from(&delta.camera_id);
    base.dropped_frames = delta.dropped_frames;
    base.storage_free_mb = delta.storage_free_mb;
    for (id, entry) in &delta.state {
        base.state.insert(id.clone(), entry.clone());
    }
}

fn empty_state() -> StateMessage {
    StateMessage {
        seq: 0,
        timestamp_ms: 0,
        camera_id: String::new(),
        state: BTreeMap::new(),
        dropped_frames: 0,
        storage_free_mb: None,
        delta: false,
    }
}

fn state_timeout() -> WatchdogError {
    WatchdogError::new(ErrorCode::ConnectionLost, "timed out waiting for STATE").with_hint(
        "update the app to a build with M4 state sync and re-run `watchdog device provision`",
    )
}

fn app_session_error(error: &ErrorMessage) -> WatchdogError {
    WatchdogError::new(
        ErrorCode::ProtocolError,
        format!("app error: {} ({})", error.message, error.code),
    )
}

fn bye_error(bye: &ByeMessage) -> WatchdogError {
    WatchdogError::new(
        ErrorCode::StreamClosed,
        format!("app closed the session: {}", bye.reason),
    )
}

/// Parse one `id=value` CLI assignment into a JSON value.
fn parse_assignment(raw: &str) -> watchdog_core::Result<(String, Value)> {
    let Some((id, value)) = raw.split_once('=') else {
        return Err(WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("'{raw}' is not an id=value assignment"),
        )
        .with_hint("examples: iso=400, ae_lock=true, zoom_ratio=2.5, awb_mode=1"));
    };
    let id = id.trim();
    if id.is_empty() {
        return Err(WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("'{raw}' has an empty control id"),
        ));
    }
    Ok((id.to_string(), parse_control_value(value.trim())))
}

fn parse_control_value(value: &str) -> Value {
    if value.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if value.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Value::from(integer);
    }
    if let Ok(float) = value.parse::<f64>() {
        return Value::from(float);
    }
    if value.starts_with('[') || value.starts_with('{') {
        if let Ok(json) = serde_json::from_str::<Value>(value) {
            return json;
        }
    }
    let unquoted = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        });
    Value::String(unquoted.unwrap_or(value).to_string())
}

fn next_command_id(prefix: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
}

fn state_value(state: &StateMessage) -> watchdog_core::Result<Value> {
    let mut value = serde_json::to_value(state)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("delta".to_string(), Value::Bool(state.delta));
    }
    Ok(value)
}

fn print_state(serial: &str, state: &StateMessage, json: bool) -> watchdog_core::Result<()> {
    if json {
        println!("{}", serde_json::to_string(&state_value(state)?)?);
    } else {
        print!("{}", render_state(serial, state));
    }
    Ok(())
}

/// Human-readable control state. Pure so it can be unit-tested.
fn render_state(serial: &str, state: &StateMessage) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "serial:   {serial}");
    let camera = if state.camera_id.is_empty() {
        "-"
    } else {
        &state.camera_id
    };
    let _ = writeln!(out, "camera:   {camera}");
    let _ = writeln!(
        out,
        "seq:      {} ({})",
        state.seq,
        if state.delta { "delta" } else { "full" }
    );
    let _ = writeln!(out, "dropped:  {} frame(s)", state.dropped_frames);
    if let Some(free) = state.storage_free_mb {
        let _ = writeln!(out, "storage:  {free} MiB free");
    }
    let _ = writeln!(out, "controls: {}", state.state.len());
    for (id, entry) in &state.state {
        let _ = writeln!(out, "  {}", render_control_entry(id, entry));
    }
    out
}

fn render_control_entry(id: &str, entry: &ControlEntry) -> String {
    let mut line = format!("{id:<24} ");
    if let Some(requested) = &entry.requested {
        line.push_str(&format!("requested={} ", compact(requested)));
    }
    if let Some(actual) = &entry.actual {
        line.push_str(&format!("actual={} ", compact(actual)));
    }
    if let Some(locked) = entry.locked {
        line.push_str(&format!("locked={locked} "));
    }
    line.push_str(&format!(
        "supported={} source={}",
        entry.supported,
        source_name(entry.source)
    ));
    line
}

fn render_set_result(
    serial: &str,
    elapsed: Duration,
    result: &CommandResultMessage,
    actuals: &BTreeMap<String, Value>,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "serial:     {serial}");
    let _ = writeln!(
        out,
        "round-trip: {} ms (request -> COMMAND_RESULT)",
        elapsed.as_millis()
    );
    if let Some(error) = &result.error {
        let _ = writeln!(out, "error:      {error}");
    }
    for item in &result.results {
        let control = item.control.as_deref().unwrap_or("(command)");
        if item.ok {
            let applied = item
                .applied
                .as_ref()
                .map_or_else(|| "-".to_string(), compact);
            let actual = item
                .control
                .as_ref()
                .and_then(|id| actuals.get(id))
                .map_or_else(|| "-".to_string(), compact);
            let _ = writeln!(
                out,
                "  {control:<20} ok   applied={applied} actual={actual}"
            );
        } else {
            let _ = writeln!(
                out,
                "  {control:<20} FAIL {} {}",
                item.error_code.as_deref().unwrap_or("ERROR"),
                item.message.as_deref().unwrap_or("")
            );
        }
    }
    out
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "?".to_string())
}

fn source_name(source: ControlSource) -> &'static str {
    match source {
        ControlSource::Host => "host",
        ControlSource::Device => "device",
        ControlSource::Auto => "auto",
    }
}

/// Human-readable capability summary. Kept pure so it can be unit-tested
/// without a device.
fn render_capabilities(serial: &str, ack: &HelloAck) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "serial:  {serial}");
    let _ = writeln!(
        out,
        "app:     {} ({})",
        ack.app.version_name, ack.app.version_code
    );
    let _ = writeln!(
        out,
        "device:  {} {} (SDK {})",
        ack.device.manufacturer, ack.device.model, ack.device.sdk
    );
    let _ = writeln!(out, "cameras: {}", ack.capabilities.len());
    for (id, descriptor) in &ack.capabilities {
        let _ = writeln!(out, "camera {id}:");
        if let Some(error) = descriptor.get("error").and_then(serde_json::Value::as_str) {
            let _ = writeln!(out, "  error: {error}");
            continue;
        }
        let _ = writeln!(
            out,
            "  lens: {} / {}, capabilities: {}",
            descriptor
                .get("lens_facing")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown"),
            descriptor
                .get("hardware_level")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("UNKNOWN"),
            array_len(descriptor, "capabilities")
        );
        render_controls(&mut out, descriptor);
        render_streams(&mut out, descriptor);
        let _ = writeln!(
            out,
            "  keys: {} request, {} result, {} characteristics",
            array_len(descriptor, "available_request_keys"),
            array_len(descriptor, "available_result_keys"),
            array_len(descriptor, "available_characteristics_keys")
        );
        let unsupported = descriptor
            .get("unsupported")
            .and_then(serde_json::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| {
                        format!(
                            "{} ({})",
                            entry
                                .get("control")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("?"),
                            entry
                                .get("reason")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown")
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !unsupported.is_empty() {
            let _ = writeln!(out, "  unsupported: {}", unsupported.join(", "));
        }
    }
    out
}

fn render_controls(out: &mut String, descriptor: &serde_json::Value) {
    use std::fmt::Write as _;

    const METADATA: &[&str] = &[
        "hardware_level",
        "lens_facing",
        "sensor_orientation",
        "capabilities",
        "physical_ids",
        "lens",
        "streams",
        "available_request_keys",
        "available_result_keys",
        "available_characteristics_keys",
        "unsupported",
    ];
    let Some(fields) = descriptor.as_object() else {
        return;
    };
    let mut controls = Vec::new();
    for (name, value) in fields {
        if METADATA.contains(&name.as_str())
            || value.get("supported").and_then(serde_json::Value::as_bool) != Some(true)
        {
            continue;
        }
        if let Some(modes) = value.get("modes").and_then(serde_json::Value::as_array) {
            let modes = modes
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join(",");
            controls.push(format!("{name}[{modes}]"));
        } else if let (Some(min), Some(max)) = (value.get("min"), value.get("max")) {
            let step = value
                .get("step")
                .map_or_else(|| "1".to_string(), serde_json::Value::to_string);
            controls.push(format!("{name}[{min}..{max} step {step}]"));
        } else if let Some(sizes) = value.get("sizes").and_then(serde_json::Value::as_array) {
            controls.push(format!("{name}[{} size(s)]", sizes.len()));
        } else if let Some(by_format) = value
            .get("sizes_by_format")
            .and_then(serde_json::Value::as_object)
        {
            let formats = by_format
                .iter()
                .map(|(format, sizes)| format!("{format} {}", sizes.as_array().map_or(0, Vec::len)))
                .collect::<Vec<_>>()
                .join(", ");
            controls.push(format!("{name}[{formats}]"));
        } else if let Some(values) = value.get("values").and_then(serde_json::Value::as_array) {
            let values = values
                .iter()
                .map(|item| {
                    item.as_str()
                        .map_or_else(|| item.to_string(), |text| text.to_string())
                })
                .collect::<Vec<_>>()
                .join(",");
            controls.push(format!("{name}[{values}]"));
        } else if value.get("structural").and_then(serde_json::Value::as_bool) == Some(true) {
            controls.push(format!("{name}[on/off]"));
        } else {
            controls.push(name.clone());
        }
    }
    if !controls.is_empty() {
        let _ = writeln!(out, "  controls: {}", controls.join(", "));
    }
}

fn render_streams(out: &mut String, descriptor: &serde_json::Value) {
    use std::fmt::Write as _;

    let Some(streams) = descriptor.get("streams") else {
        return;
    };
    if streams
        .get("available")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        let _ = writeln!(out, "  streams: unavailable");
        return;
    }
    let formats = streams
        .get("formats")
        .and_then(serde_json::Value::as_object)
        .map(|formats| {
            formats
                .iter()
                .map(|(name, sizes)| format!("{name} {}", sizes.as_array().map_or(0, Vec::len)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let _ = writeln!(out, "  streams: {formats} size(s)");
    let _ = writeln!(
        out,
        "  fps: {} range(s); high-speed: {} size(s)",
        array_len(streams, "fps_ranges"),
        streams
            .get("high_speed_video")
            .map_or(0, |video| array_len(video, "sizes"))
    );
}

fn array_len(value: &serde_json::Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

fn print_log_chunk(chunk: &LogChunk) {
    let level = match chunk.level {
        LogLevel::Trace => "T",
        LogLevel::Debug => "D",
        LogLevel::Info => "I",
        LogLevel::Warn => "W",
        LogLevel::Error => "E",
    };
    for line in chunk.text.lines() {
        println!("[{level}] {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ack(capabilities: serde_json::Value) -> HelloAck {
        HelloAck {
            protocol_version: 1,
            app: watchdog_protocol::AppInfo {
                version_code: 7,
                version_name: "1.2.3".to_string(),
                package: "app.watchdog.remote".to_string(),
            },
            device: watchdog_protocol::DeviceInfo {
                manufacturer: "Acme".to_string(),
                model: "Cam".to_string(),
                device: "cam".to_string(),
                sdk: 34,
                release: "14".to_string(),
                abi: "arm64-v8a".to_string(),
                battery_percent: Some(80),
                storage_free_mb: Some(1024),
                temperature_c: None,
            },
            cameras: Vec::new(),
            capabilities: capabilities
                .as_object()
                .unwrap()
                .clone()
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn renders_controls_streams_and_keys() {
        let ack = ack(json!({
            "0": {
                "hardware_level": "LEVEL_3",
                "lens_facing": "back",
                "sensor_orientation": 90,
                "capabilities": ["BACKWARD_COMPATIBLE", "RAW"],
                "physical_ids": [],
                "lens": {},
                "control_mode": {"supported": true, "modes": [0, 1]},
                "iso": {"supported": true, "min": 50, "max": 12800, "step": 1},
                "zoom_ratio": {"supported": false},
                "stream_size": {
                    "supported": true,
                    "structural": true,
                    "sizes": [{"width": 1280, "height": 720}]
                },
                "still_format": {"supported": true, "structural": true, "values": ["JPEG", "RAW_SENSOR"]},
                "still_size": {
                    "supported": true,
                    "structural": true,
                    "sizes_by_format": {"JPEG": [{"width": 4000, "height": 3000}]}
                },
                "audio": {"supported": true, "structural": true},
                "streams": {
                    "available": true,
                    "formats": {"JPEG": [{"width": 1920, "height": 1080}], "YUV_420_888": []},
                    "fps_ranges": [[15, 30]],
                    "high_speed_video": {"sizes": [], "fps_ranges": []}
                },
                "available_request_keys": [
                    {"name": "android.sensor.iso", "type": "java.lang.Integer"}
                ],
                "available_result_keys": [],
                "available_characteristics_keys": [
                    {"name": "android.sensor.info.sensitivityRange", "type": "android.util.Range"}
                ],
                "unsupported": [{"control": "zoom_ratio", "reason": "no digital zoom"}]
            }
        }));
        let text = render_capabilities("emulator-5554", &ack);
        assert!(text.contains("serial:  emulator-5554"));
        assert!(text.contains("app:     1.2.3 (7)"));
        assert!(text.contains("camera 0:"));
        assert!(text.contains("lens: back / LEVEL_3, capabilities: 2"));
        assert!(text.contains("control_mode[0,1]"));
        assert!(text.contains("iso[50..12800 step 1]"));
        assert!(text.contains("stream_size[1 size(s)]"));
        assert!(text.contains("still_format[JPEG,RAW_SENSOR]"));
        assert!(text.contains("still_size[JPEG 1]"));
        assert!(text.contains("audio[on/off]"));
        assert!(!text.contains("zoom_ratio["));
        assert!(text.contains("streams: JPEG 1, YUV_420_888 0 size(s)"));
        assert!(text.contains("fps: 1 range(s); high-speed: 0 size(s)"));
        assert!(text.contains("keys: 1 request, 0 result, 1 characteristics"));
        assert!(text.contains("unsupported: zoom_ratio (no digital zoom)"));
    }

    #[test]
    fn renders_camera_errors_and_unavailable_streams() {
        let ack = ack(json!({
            "1": {"error": "camera disconnected"},
            "2": {"streams": {"available": false}}
        }));
        let text = render_capabilities("serial", &ack);
        assert!(text.contains("camera 1:\n  error: camera disconnected"));
        assert!(text.contains("camera 2:"));
        assert!(text.contains("streams: unavailable"));
    }

    fn entry(
        requested: Option<Value>,
        actual: Option<Value>,
        locked: Option<bool>,
        supported: bool,
        source: ControlSource,
    ) -> ControlEntry {
        ControlEntry {
            requested,
            actual,
            locked,
            supported,
            source,
        }
    }

    #[test]
    fn parses_control_assignments_as_json_values() {
        assert_eq!(
            parse_assignment("iso=400").unwrap(),
            ("iso".to_string(), json!(400))
        );
        assert_eq!(parse_assignment("ae_lock=TRUE").unwrap().1, json!(true));
        assert_eq!(parse_assignment("zoom_ratio=2.5").unwrap().1, json!(2.5));
        assert_eq!(parse_assignment("awb_mode=1").unwrap().1, json!(1));
        assert_eq!(parse_assignment("label=\"auto\"").unwrap().1, json!("auto"));
        assert_eq!(parse_assignment("label='auto'").unwrap().1, json!("auto"));
        assert_eq!(
            parse_assignment("ae_fps_range=[30,30]").unwrap().1,
            json!([30, 30])
        );
        assert_eq!(
            parse_assignment("ae_regions=[{\"x\":0.25,\"y\":0.25,\"w\":0.5,\"h\":0.5}]")
                .unwrap()
                .1,
            json!([{ "x": 0.25, "y": 0.25, "w": 0.5, "h": 0.5 }])
        );
        assert_eq!(parse_assignment("iso=null").unwrap().1, Value::Null);
        assert!(parse_assignment("iso").is_err());
        assert!(parse_assignment("=1").is_err());
    }

    #[test]
    fn renders_full_state_with_requested_actual_and_source() {
        let mut state = empty_state();
        state.seq = 7;
        state.camera_id = "0".to_string();
        state.dropped_frames = 2;
        state.storage_free_mb = Some(1234);
        state.state.insert(
            "iso".to_string(),
            entry(
                Some(json!(400)),
                Some(json!(398)),
                None,
                true,
                ControlSource::Host,
            ),
        );
        state.state.insert(
            "torch".to_string(),
            entry(
                None,
                Some(json!(false)),
                Some(false),
                false,
                ControlSource::Device,
            ),
        );

        let text = render_state("serial", &state);
        assert!(text.contains("serial:   serial"));
        assert!(text.contains("camera:   0"));
        assert!(text.contains("seq:      7 (full)"));
        assert!(text.contains("dropped:  2 frame(s)"));
        assert!(text.contains("storage:  1234 MiB free"));
        assert!(text.contains("requested=400 actual=398 supported=true source=host"));
        assert!(text.contains("actual=false locked=false supported=false source=device"));
    }

    #[test]
    fn renders_set_results_with_actuals_and_failures() {
        let result = CommandResultMessage {
            id: "set-1".to_string(),
            timestamp_ms: 5,
            results: vec![
                watchdog_protocol::ControlResult {
                    control: Some("iso".to_string()),
                    ok: true,
                    applied: Some(json!(400)),
                    error_code: None,
                    message: None,
                },
                watchdog_protocol::ControlResult {
                    control: Some("awb_mode".to_string()),
                    ok: false,
                    applied: None,
                    error_code: Some("OUT_OF_RANGE".to_string()),
                    message: Some("mode 9 is not available".to_string()),
                },
            ],
            error: None,
        };
        let mut actuals = BTreeMap::new();
        actuals.insert("iso".to_string(), json!(398));

        let text = render_set_result("serial", Duration::from_millis(12), &result, &actuals);
        assert!(text.contains("round-trip: 12 ms"));
        assert!(text.contains("applied=400 actual=398"));
        assert!(text.contains("FAIL OUT_OF_RANGE mode 9 is not available"));
    }

    #[test]
    fn state_json_marks_deltas() {
        let mut state = empty_state();
        state.delta = true;
        state.state.insert(
            "iso".to_string(),
            entry(
                Some(json!(400)),
                Some(json!(398)),
                None,
                true,
                ControlSource::Host,
            ),
        );

        let value = state_value(&state).unwrap();
        assert_eq!(value["delta"], json!(true));
        assert_eq!(value["state"]["iso"]["actual"], json!(398));
        assert_eq!(value["state"]["iso"]["source"], json!("host"));
    }

    #[test]
    fn renders_probe_matrix_with_outcomes() {
        let report = json!({
            "camera_id": "0",
            "facing": "back",
            "hardware_level": "LIMITED",
            "device": {
                "model": "SM-A515F",
                "android": "13",
                "sdk": 33,
                "app_version": "0.1.0-dev+abc"
            },
            "available_request_keys": 4,
            "available_result_keys": 5,
            "probed_keys": 2,
            "probed_values": 3,
            "aborted": false,
            "entries": [
                {
                    "key": "android.control.aeMode",
                    "status": "probed",
                    "values": [
                        {
                            "label": "mode 1 (ON)",
                            "requested": 1,
                            "outcome": "accepted",
                            "reflected": true,
                            "result": 1,
                            "elapsed_ms": 12
                        },
                        {
                            "label": "mode 0 (OFF)",
                            "requested": 0,
                            "outcome": "ignored",
                            "reflected": false,
                            "result": 1,
                            "elapsed_ms": 9
                        },
                        {
                            "label": "mode 9",
                            "requested": 9,
                            "outcome": "error",
                            "reflected": false,
                            "error": "IllegalArgumentException: bad mode",
                            "elapsed_ms": 3
                        }
                    ]
                },
                {
                    "key": "android.control.afTrigger",
                    "status": "skipped",
                    "reason": "one-shot 3A trigger"
                },
                {
                    "key": "vendor.key",
                    "status": "unprobed",
                    "reason": "vendor key"
                }
            ],
            "not_advertised": [
                {"field": "CONTROL_ZOOM_RATIO", "key": "android.control.zoomRatio"}
            ]
        });

        let text = render_probe_matrix("serial", &report);
        assert!(text.contains("# Control compatibility matrix — SM-A515F (Android 13, SDK 33)"));
        assert!(text.contains("- Camera: `0` (back), hardware level LIMITED"));
        assert!(text.contains("1 accepted, 1 ignored, 1 error"));
        assert!(text.contains(
            "| `android.control.aeMode` | `1` | mode 1 (ON) | accepted | yes | `1` | 12 |"
        ));
        assert!(text.contains(
            "| `android.control.aeMode` | `0` | mode 0 (OFF) | ignored | no | `1` | 9 |"
        ));
        assert!(text.contains(
            "| `android.control.aeMode` | `9` | mode 9 | error | no | `error: IllegalArgumentException: bad mode` | 3 |"
        ));
        assert!(text.contains("| `android.control.afTrigger` | skipped | one-shot 3A trigger |"));
        assert!(text.contains("- `CONTROL_ZOOM_RATIO` (`android.control.zoomRatio`)"));
    }
}
