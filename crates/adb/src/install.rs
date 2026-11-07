//! Package management — streaming install over the ADB shell.
//!
//! `cmd package install` (fallback: `pm install`) is invoked via
//! `shell,v2,raw:` and fed the APK bytes on stdin. Exit code and output are
//! captured; `INSTALL_FAILED_*` codes are mapped to actionable hints.

use std::io::Read;
use std::path::Path;

use bytes::Bytes;

use crate::connection::Connection;
use crate::error::{AdbError, Result};
use crate::shell::{self, ids};
use crate::transport::Transport;

const CHUNK: usize = 256 * 1024;

/// Options for [`install_apk`].
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Keep existing data (`-r`); default true.
    pub reinstall: bool,
    /// Allow downgrade to a lower versionCode (`-d`).
    pub allow_downgrade: bool,
    /// Grant all runtime permissions at install time (`-g`).
    pub grant_permissions: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            reinstall: true,
            allow_downgrade: false,
            grant_permissions: false,
        }
    }
}

/// Outcome of an install attempt.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    /// `true` when the device reported success.
    pub success: bool,
    /// Full device output (stdout+stderr merged by the caller when needed).
    pub output: String,
    /// Recognized `INSTALL_FAILED_*` code, if any.
    pub failure_code: Option<String>,
    /// Actionable hint for [`Self::failure_code`].
    pub failure_hint: Option<String>,
}

/// Map a device install failure code to an actionable hint.
#[must_use]
pub fn install_failure_hint(code: &str) -> &'static str {
    match code {
        "INSTALL_FAILED_UPDATE_INCOMPATIBLE" | "INSTALL_FAILED_VERSION_DOWNGRADE" => {
            "A build signed with a different key (or a newer version) is installed. \
             Uninstall it first with `watchdog device uninstall app.watchdog.remote` — this deletes its data."
        }
        "INSTALL_FAILED_NO_MATCHING_ABIS" => {
            "This APK has no matching ABI for the device (watchdog builds arm64-v8a and x86_64)."
        }
        "INSTALL_FAILED_OLDER_SDK" => "The device runs an older Android than the app's minSdk (26).",
        "INSTALL_FAILED_INSUFFICIENT_STORAGE" => "The device is out of storage.",
        "INSTALL_FAILED_INVALID_APK" | "INSTALL_PARSE_FAILED_NOT_APK" | "INSTALL_PARSE_FAILED_BAD_MANIFEST" => {
            "The APK is corrupt or malformed; re-download the release artifact and verify its SHA-256."
        }
        "INSTALL_FAILED_USER_RESTRICTED" => {
            "The OEM/OS blocks installs for this user. Enable 'Install unknown apps' for the installer, or install via the system package manager with user action."
        }
        "INSTALL_FAILED_DUPLICATE_PERMISSION" => {
            "A different app already signs a permission this APK declares; the set must be signed with the same key."
        }
        "INSTALL_FAILED_VERIFICATION_FAILURE" => {
            "Play Protect / package verification blocked the install. Temporarily verify again on the device, or check its policy."
        }
        "INSTALL_FAILED_TEST_ONLY" => {
            "The APK is marked android:testOnly. Both release and debug watchdog APKs install without this flag."
        }
        "INSTALL_FAILED_MISSING_SHARED_LIBRARY" => "The app requires a shared library this device lacks (e.g. an OEM library).",
        "INSTALL_FAILED_CPU_ABI_INCOMPATIBLE" => "The device CPU is incompatible with this APK.",
        "INSTALL_FAILED_CANCELLED" | "INSTALL_FAILED_ABORTED" => "The install was cancelled on the device.",
        _ => "See the device output above for details; re-run with --log-level debug for the full transcript.",
    }
}

/// Extract an `INSTALL_*` token from device output.
fn find_failure_code(output: &str) -> Option<String> {
    output
        .split(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .find(|token| token.starts_with("INSTALL_"))
        .map(str::to_string)
}

/// Stream-install an APK. Returns the parsed outcome (never errors on device-reported failure).
pub async fn install_apk<T: Transport + 'static>(
    connection: &Connection<T>,
    apk: &Path,
    options: &InstallOptions,
    mut progress: Option<&mut dyn FnMut(u64, u64)>,
) -> Result<InstallOutcome> {
    let size = std::fs::metadata(apk)?.len();
    let mut flags = String::new();
    if options.reinstall {
        flags.push(" -r");
    }
    if options.allow_downgrade {
        flags.push(" -d");
    }
    if options.grant_permissions {
        flags.push(" -g");
    }

    // Prefer `cmd package install`; fall back to `pm install` if cmd is missing.
    let use_cmd = has_cmd_package(connection).await;
    install_with(connection, apk, flags.trim(), size, use_cmd, &mut progress).await
}

/// Detect whether `cmd package` exists on the device (probed once per install).
async fn has_cmd_package<T: Transport + 'static>(connection: &Connection<T>) -> bool {
    match crate::shell::run_shell(connection, "cmd package path android", None).await {
        Ok(out) => {
            let combined = format!("{}{}", out.stdout_string(), out.stderr_string());
            !combined.contains("can't find service")
                && !combined.contains("No shell command implementation")
                && !combined.contains("Unknown command")
        }
        Err(_) => false,
    }
}

async fn install_with<T: Transport + 'static>(
    connection: &Connection<T>,
    apk: &Path,
    flags: &str,
    size: u64,
    use_cmd: bool,
    progress: &mut Option<&mut dyn FnMut(u64, u64)>,
) -> Result<InstallOutcome> {
    let command = if use_cmd {
        format!("cmd package install{flags} -S {size}")
    } else {
        format!("pm install{flags} -S {size}")
    };
    let mut stream = connection.open_stream(&format!("shell,v2,raw:{command}")).await?;

    // Feed the APK as shell v2 stdin frames.
    let mut file = std::fs::File::open(apk)?;
    let mut sent: u64 = 0;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        stream
            .write(Bytes::from(shell::encode_shell_packet(ids::STDIN, &buf[..n])))
            .await?;
        sent += n as u64;
        if let Some(cb) = progress.as_deref_mut() {
            cb(sent, size);
        }
    }
    stream
        .write(Bytes::from(shell::encode_shell_packet(ids::CLOSE_STDIN, &[])))
        .await?;

    collect_install_output(stream).await
}

async fn collect_install_output(mut stream: crate::connection::Stream) -> Result<InstallOutcome> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(3600), stream.inbound.recv()).await {
            Ok(Some(chunk)) => {
                pending.extend_from_slice(&chunk);
                loop {
                    match shell::decode_shell_packet(&pending) {
                        Some((packet, consumed)) => {
                            pending.drain(..consumed);
                            match packet {
                                shell::ShellPacket::Stdout(data) => stdout.extend_from_slice(&data),
                                shell::ShellPacket::Stderr(data) => stderr.extend_from_slice(&data),
                                shell::ShellPacket::Exit(_) => {
                                    let output = format!(
                                        "{}{}",
                                        String::from_utf8_lossy(&stdout),
                                        String::from_utf8_lossy(&stderr)
                                    );
                                    let failure = find_failure_code(&output);
                                    return Ok(InstallOutcome {
                                        success: failure.is_none() && output.contains("Success"),
                                        output,
                                        failure_code: failure.clone(),
                                        failure_hint: failure.as_deref().map(install_failure_hint).map(str::to_string),
                                    });
                                }
                                shell::ShellPacket::CloseStdin | shell::ShellPacket::Other(_, _) => {}
                            }
                        }
                        None => break,
                    }
                }
            }
            Ok(None) => {
                let output = format!(
                    "{}{}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
                let failure = find_failure_code(&output);
                return Ok(InstallOutcome {
                    success: failure.is_none() && output.contains("Success"),
                    output,
                    failure_code: failure.clone(),
                    failure_hint: failure.as_deref().map(install_failure_hint).map(str::to_string),
                });
            }
            Err(_) => return Err(AdbError::Timeout("package install timed out".into())),
        }
    }
}

/// Uninstall a package. Returns device output.
pub async fn uninstall_package<T: Transport + 'static>(
    connection: &Connection<T>,
    package: &str,
) -> Result<String> {
    let output = crate::shell::run_shell(connection, &format!("pm uninstall {package}"), None).await?;
    Ok(format!("{}{}", output.stdout_string(), output.stderr_string()))
}

/// Best-effort package info via `dumpsys package`.
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// `versionCode` from the package record.
    pub version_code: i64,
    /// SHA-256 digests of signing certificates (hex, lowercase).
    pub signature_sha256: Vec<String>,
}

/// Query versionCode and signature digests; `Ok(None)` when the package is absent.
pub async fn query_package<T: Transport + 'static>(
    connection: &Connection<T>,
    package: &str,
) -> Result<Option<PackageInfo>> {
    let output = crate::shell::run_shell(connection, &format!("dumpsys package {package}"), None).await?;
    let text = output.stdout_string();
    if text.contains("Unable to find package") || text.contains("Package not found") {
        return Ok(None);
    }
    let version_code = text
        .lines()
        .find_map(|line| line.split("versionCode=").nth(1))
        .and_then(|rest| {
            rest.split(|c: char| !(c.is_ascii_digit()))
                .find(|s| !s.is_empty())
                .and_then(|s| s.parse::<i64>().ok())
        })
        .unwrap_or(-1);
    // Signatures are printed as a hex list inside `signatures=PackageSignatures{[...]}`
    // or as `PackageSignatures{<hex>}`; collect all 64-char hex runs after "signatures".
    let mut signature_sha256 = Vec::new();
    for marker in ["signatures=", "Signatures="] {
        if let Some(idx) = text.find(marker) {
            let slice = &text[idx..];
            let end = slice.find('\n').map(|i| idx + i).unwrap_or(text.len());
            for token in text[idx..end].split(|c: char| !(c.is_ascii_hexdigit())) {
                if token.len() == 64 {
                    signature_sha256.push(token.to_ascii_lowercase());
                }
            }
        }
    }
    Ok(Some(PackageInfo {
        version_code,
        signature_sha256,
    }))
}
