//! `watchdog device` — device management (native ADB, no system adb).

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Subcommand};

use watchdog_core::config::Config;
use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::paths::DataPaths;
use watchdog_device::apk::{self, ApkSource};
use watchdog_device::connection::DeviceConnection;
use watchdog_device::provision::{
    self, ProvisionOptions, ProvisionReport, RestoreRecord, StatusReport, StepStatus,
};
use watchdog_device::session::{LiveSession, Polled};
use watchdog_protocol::{HelloAck, LogChunk, LogLevel, Message};

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
            .map_err(Into::into)
    })?;
    println!("Paired with device {}", outcome.device_guid);
    Ok(())
}

fn with_usb_connection<T, F, Fut>(
    cfg: &Config,
    serial: Option<&str>,
    f: F,
) -> watchdog_core::Result<T>
where
    F: FnOnce(Arc<watchdog_adb::Connection<watchdog_adb::UsbTransport>>) -> Fut,
    Fut: std::future::Future<Output = Result<T, watchdog_adb::AdbError>>,
{
    let keys = adbs_key(cfg)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let out = rt.block_on(async {
        let transport = if let Some(serial) = serial {
            watchdog_adb::UsbTransport::open_by_serial(serial)?
        } else {
            watchdog_adb::UsbTransport::open()?
        };
        let connection =
            Arc::new(watchdog_adb::Connection::connect(transport, "host::watchdog", &keys).await?);
        f(Arc::clone(&connection)).await
    });
    out.map_err(Into::into)
}

fn cli_shell(serial: Option<&str>, command: &[String], cfg: &Config) -> watchdog_core::Result<()> {
    if command.is_empty() {
        return Err(watchdog_core::WatchdogError::new(
            watchdog_core::ErrorCode::InvalidRequest,
            "no command given (usage: watchdog device shell [--serial S] -- cmd…)",
        ));
    }
    let cmd = command.join(" ");
    let output = with_usb_connection(cfg, serial, |conn| async move {
        watchdog_adb::shell::run_shell(&conn, &cmd, None).await
    })?;
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
    let written = with_usb_connection(cfg, serial, |conn| async move {
        watchdog_adb::sync::push(&conn, local, remote, 0o100644, current_mtime(), None).await
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
    let result = with_usb_connection(cfg, serial, |conn| async move {
        watchdog_adb::sync::pull(&conn, remote, local, None).await
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
    let outcome = with_usb_connection(cfg, serial, |conn| async move {
        watchdog_adb::install_apk(&conn, &apk_path, &options, None).await
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
    let output = with_usb_connection(cfg, serial, |conn| async move {
        watchdog_adb::install::uninstall_package(&conn, package).await
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
    with_usb_connection(cfg, serial, |conn| async move {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{local_port}").parse().unwrap();
        let handle = watchdog_adb::start_forward(conn, addr, remote.to_string()).await?;
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
}
