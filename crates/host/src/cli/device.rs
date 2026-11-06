//! `watchdog device` — device management (native ADB, no system adb).
//!
//! `udev-rules` is implemented now because it is pure text; the rest land
//! with M2 (native ADB) and M3 (provisioning).

use std::io::Write as _;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use watchdog_core::config::Config;

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
    },
    /// Show provisioning/app status for the device.
    Status {
        /// Target device; omit when only one is connected.
        #[arg(long, value_name = "SERIAL")]
        serial: Option<String>,
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
pub fn run(cmd: &DeviceCommand, _cfg: &Config) -> watchdog_core::Result<()> {
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
        other => Err(match other {
            DeviceCommand::List(_)
            | DeviceCommand::Discover
            | DeviceCommand::Connect { .. }
            | DeviceCommand::Pair { .. }
            | DeviceCommand::Disconnect { .. }
            | DeviceCommand::Info { .. }
            | DeviceCommand::Shell { .. }
            | DeviceCommand::Push { .. }
            | DeviceCommand::Pull { .. }
            | DeviceCommand::Install { .. }
            | DeviceCommand::Uninstall { .. }
            | DeviceCommand::Forward { .. }
            | DeviceCommand::Logs { .. }
            | DeviceCommand::UsbDoctor => {
                not_implemented("native ADB device management", "M2 (docs/PROGRESS.md)")
            }
            DeviceCommand::Provision { .. }
            | DeviceCommand::Status { .. }
            | DeviceCommand::Restore { .. }
            | DeviceCommand::Capabilities { .. } => not_implemented(
                "zero-touch provisioning / capability dump",
                "M3–M4 (docs/PROGRESS.md)",
            ),
            DeviceCommand::UdevRules { .. } => unreachable!(),
        }),
    }
}
