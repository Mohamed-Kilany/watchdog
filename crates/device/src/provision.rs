//! Zero-touch provisioning pipeline (M3-T13) and the restore/status side of it.
//!
//! Every step is reported with a status and a duration, is idempotent (running
//! provision twice is safe) and resumable (a failed step names the next action).
//! Screen settings changed by provisioning are recorded per device and can be
//! reverted with `watchdog device restore`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::paths::DataPaths;
use watchdog_core::Result;

use crate::apk::{self, ApkSource};
use crate::connection::DeviceConnection;
use crate::session::LiveSession;

/// Android application id of the remote camera app.
pub const PACKAGE: &str = "app.watchdog.remote";
/// Oldest Android release the app supports (Android 8.0).
pub const MIN_SDK: i64 = 26;

const SESSION_EXTRA: &str = "app.watchdog.remote.extra.SESSION_TOKEN";
const START_ACTION: &str = "app.watchdog.remote.action.START";
const PROPS_COMMAND: &str = "getprop ro.build.version.sdk;getprop ro.product.cpu.abi;\
getprop ro.product.model;getprop ro.product.manufacturer;\
getprop ro.build.version.release;getprop ro.product.device";

/// Outcome of one provisioning step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    /// The step completed (including "already configured").
    Passed,
    /// The step failed; later dependent steps are skipped.
    Failed,
    /// The step did not apply to this device or was disabled by a flag.
    Skipped,
}

/// One reported provisioning step.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    /// Stable step name (`device`, `app`, `permissions`, ...).
    pub name: &'static str,
    /// Pass/fail/skipped.
    pub status: StepStatus,
    /// Wall-clock duration of the step.
    pub duration_ms: u64,
    /// Human-readable summary (or failure reason).
    pub detail: String,
    /// Actionable remediation for a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Full provisioning report, also suitable for `--json` output.
#[derive(Debug, Clone, Serialize)]
pub struct ProvisionReport {
    /// Device serial (or `host:port`).
    pub serial: String,
    /// Steps in execution order.
    pub steps: Vec<StepReport>,
    /// Session token used to start the app, when the launch step ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl ProvisionReport {
    fn new(serial: &str) -> Self {
        Self {
            serial: serial.to_string(),
            steps: Vec::new(),
            token: None,
        }
    }

    /// True when no step failed.
    #[must_use]
    pub fn passed(&self) -> bool {
        !self
            .steps
            .iter()
            .any(|step| step.status == StepStatus::Failed)
    }

    /// The first failed step, if any.
    #[must_use]
    pub fn failure(&self) -> Option<&StepReport> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::Failed)
    }
}

/// Options for [`provision`].
#[derive(Debug, Clone)]
pub struct ProvisionOptions {
    /// Local APK override; otherwise the embedded release APK is used.
    pub apk: Option<PathBuf>,
    /// Use a specific session token instead of a fresh random one.
    pub token: Option<String>,
    /// Verify handshake + first frame after launching.
    pub verify_frame: bool,
    /// How long to wait for the app socket to appear.
    pub socket_timeout: Duration,
    /// How long to wait for `HELLO_ACK` on each attempt.
    pub handshake_timeout: Duration,
    /// How long to wait for the first JPEG frame.
    pub frame_timeout: Duration,
    /// Do not change screen settings (useful when a human manages the screen).
    pub skip_screen: bool,
}

impl Default for ProvisionOptions {
    fn default() -> Self {
        Self {
            apk: None,
            token: None,
            verify_frame: true,
            socket_timeout: Duration::from_secs(20),
            handshake_timeout: Duration::from_secs(10),
            frame_timeout: Duration::from_secs(30),
            skip_screen: false,
        }
    }
}

/// Device properties read during step 1.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceProps {
    /// `ro.build.version.sdk`.
    pub sdk: i64,
    /// `ro.product.cpu.abi` (primary ABI).
    pub abi: String,
    /// `ro.product.model`.
    pub model: String,
    /// `ro.product.manufacturer`.
    pub manufacturer: String,
    /// `ro.build.version.release`.
    pub release: String,
    /// `ro.product.device`.
    pub device: String,
}

impl DeviceProps {
    /// Read the properties provisioning and `status` need.
    pub async fn read(connection: &DeviceConnection) -> Result<Self> {
        let output = connection.shell(PROPS_COMMAND).await?;
        let stdout = output.stdout_string();
        let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
        let value = |index: usize| lines.get(index).copied().unwrap_or_default().to_string();
        let sdk = value(0).parse::<i64>().map_err(|_| {
            WatchdogError::new(
                ErrorCode::ProvisioningFailed,
                format!(
                    "could not read ro.build.version.sdk (device said {:?})",
                    value(0)
                ),
            )
            .with_hint(
                "the device may be offline or not fully booted; re-run `watchdog device provision`",
            )
        })?;
        Ok(Self {
            sdk,
            abi: value(1),
            model: value(2),
            manufacturer: value(3),
            release: value(4),
            device: value(5),
        })
    }

    /// One-line human summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} {} ({}) — Android {} (SDK {}), {}",
            self.manufacturer, self.model, self.device, self.release, self.sdk, self.abi
        )
    }
}

/// Run the full provisioning pipeline.
pub async fn provision(
    connection: &DeviceConnection,
    serial: &str,
    paths: &DataPaths,
    options: &ProvisionOptions,
) -> ProvisionReport {
    let mut report = ProvisionReport::new(serial);
    let mut record = match RestoreRecord::load(paths, serial) {
        Ok(record) => record.unwrap_or_else(|| RestoreRecord::new(serial)),
        Err(error) => {
            tracing::warn!(%error, "ignoring unreadable provisioning restore record");
            RestoreRecord::new(serial)
        }
    };

    // 1. Device and properties.
    let step = Step::start("device");
    let props = match DeviceProps::read(connection).await {
        Ok(props) => {
            if props.sdk < MIN_SDK {
                report.steps.push(step.failed(
                    format!(
                        "Android {} (SDK {}) is older than the supported minimum (SDK {})",
                        props.release, props.sdk, MIN_SDK
                    ),
                    Some("watchdog requires Android 8.0 (API 26) or newer".to_string()),
                ));
                skip_remaining(
                    &mut report,
                    &[
                        "app",
                        "permissions",
                        "battery",
                        "screen",
                        "launch",
                        "verify",
                    ],
                    "device is unsupported",
                );
                return report;
            }
            report.steps.push(step.passed(props.summary()));
            props
        }
        Err(error) => {
            report
                .steps
                .push(step.failed(error.message.clone(), error.hint.clone()));
            skip_remaining(
                &mut report,
                &[
                    "app",
                    "permissions",
                    "battery",
                    "screen",
                    "launch",
                    "verify",
                ],
                "device properties unavailable",
            );
            return report;
        }
    };

    // 2. App version, signature and install/update.
    let step = Step::start("app");
    match ensure_app(connection, paths, serial, options).await {
        Ok(detail) => report.steps.push(step.passed(detail)),
        Err(error) => {
            report
                .steps
                .push(step.failed(error.message.clone(), error.hint.clone()));
            skip_remaining(
                &mut report,
                &["permissions", "battery", "screen", "launch", "verify"],
                "app install/update failed",
            );
            return report;
        }
    }

    // 3. Runtime permissions and appops.
    let step = Step::start("permissions");
    match ensure_permissions(connection, props.sdk).await {
        Ok(detail) => report.steps.push(step.passed(detail)),
        Err(error) => {
            report
                .steps
                .push(step.failed(error.message.clone(), error.hint.clone()));
            skip_remaining(
                &mut report,
                &["battery", "screen", "launch", "verify"],
                "permission grants failed",
            );
            return report;
        }
    }

    // 4. Battery: Doze whitelist, active standby bucket, background appops.
    let step = Step::start("battery");
    match ensure_battery(connection).await {
        Ok(detail) => report.steps.push(step.passed(detail)),
        Err(error) => {
            report
                .steps
                .push(step.failed(error.message.clone(), error.hint.clone()));
            skip_remaining(
                &mut report,
                &["screen", "launch", "verify"],
                "battery configuration failed",
            );
            return report;
        }
    }

    // 5. Screen: wake, keyguard, stay-on (original values recorded first).
    let step = Step::start("screen");
    if options.skip_screen {
        report
            .steps
            .push(step.skipped("disabled by --no-screen (stay-on unchanged)"));
    } else {
        match ensure_screen(connection, paths, &mut record).await {
            Ok(detail) => report.steps.push(step.passed(detail)),
            Err(error) => {
                report
                    .steps
                    .push(step.failed(error.message.clone(), error.hint.clone()));
                skip_remaining(&mut report, &["launch", "verify"], "screen setup failed");
                return report;
            }
        }
    }

    // 6. Launch with a fresh token; persist the token before starting.
    let token = options.token.clone().unwrap_or_else(generate_token);
    record.token = Some(token.clone());
    record.applied_at_ms = now_ms();
    if let Err(error) = record.save(paths) {
        let step = Step::start("launch");
        report
            .steps
            .push(step.failed(error.message.clone(), error.hint.clone()));
        skip_remaining(&mut report, &["verify"], "cannot persist the session token");
        return report;
    }
    report.token = Some(token.clone());

    let step = Step::start("launch");
    match launch(connection, &token).await {
        Ok(detail) => report.steps.push(step.passed(detail)),
        Err(error) => {
            report
                .steps
                .push(step.failed(error.message.clone(), error.hint.clone()));
            skip_remaining(&mut report, &["verify"], "capture service did not start");
            return report;
        }
    }

    // 7. Wait for socket, handshake, verify the first frame.
    let step = Step::start("verify");
    if !options.verify_frame {
        report.steps.push(step.skipped("disabled by --no-verify"));
        return report;
    }
    match verify(connection, &token, options).await {
        Ok(detail) => report.steps.push(step.passed(detail)),
        Err(error) => report
            .steps
            .push(step.failed(error.message.clone(), error.hint.clone())),
    }
    report
}

async fn ensure_app(
    connection: &DeviceConnection,
    paths: &DataPaths,
    serial: &str,
    options: &ProvisionOptions,
) -> Result<String> {
    let source = apk::resolve(options.apk.as_deref()).map_err(|error| {
        WatchdogError::new(ErrorCode::NotFound, error.to_string())
            .with_hint("build the host with WATCHDOG_APK set, or pass --apk <path> to provision")
    })?;
    let installed = connection.query_package(PACKAGE).await?;

    if let ApkSource::Embedded(embedded) = &source {
        if let Some(info) = &installed {
            let version_matches = info.version_code >= 0
                && embedded.version_code > 0
                && info.version_code == i64::from(embedded.version_code);
            let signature_matches = signature_matches(info, embedded.signer_sha256);
            if version_matches && signature_matches {
                return Ok(format!(
                    "already installed {} (versionCode {})",
                    embedded.version_name, embedded.version_code
                ));
            }
            if !signature_matches {
                return Err(WatchdogError::new(
                    ErrorCode::InstallFailed,
                    format!(
                        "installed app is signed by a different key than the embedded APK (installed {:?}, expected {:?})",
                        info.signature_sha256, embedded.signer_sha256
                    ),
                )
                .with_hint(format!(
                    "uninstall the old build first: watchdog device uninstall {PACKAGE} --serial {serial} \
                     (this deletes app data), then re-run provision"
                )));
            }
        }
    }

    let apk_path = match &source {
        ApkSource::Local { path, .. } => path.clone(),
        ApkSource::Embedded(embedded) => {
            std::fs::create_dir_all(&paths.tmp)?;
            let path = paths
                .tmp
                .join(format!("watchdog-{}.apk", embedded.version_name));
            std::fs::write(&path, embedded.bytes)?;
            path
        }
    };
    let outcome = connection
        .install(&apk_path, &watchdog_adb::InstallOptions::default())
        .await?;
    if !outcome.success {
        let code = outcome
            .failure_code
            .clone()
            .unwrap_or_else(|| "INSTALL_FAILED".to_string());
        let hint = outcome
            .failure_hint
            .clone()
            .unwrap_or_else(|| outcome.output.clone());
        return Err(WatchdogError::new(
            ErrorCode::InstallFailed,
            format!("install failed ({code})"),
        )
        .with_hint(hint));
    }
    let after = connection.query_package(PACKAGE).await?.ok_or_else(|| {
        WatchdogError::new(
            ErrorCode::InstallFailed,
            "install reported success but the package is missing",
        )
    })?;
    match &source {
        ApkSource::Embedded(embedded) if embedded.version_code > 0 => {
            if after.version_code != i64::from(embedded.version_code) {
                return Err(WatchdogError::new(
                    ErrorCode::AppVersionMismatch,
                    format!(
                        "device reports versionCode {} after installing {}",
                        after.version_code, embedded.version_code
                    ),
                )
                .with_hint("another watchdog build may be installed for a different user; uninstall it and re-run"));
            }
            Ok(format!(
                "installed {} (versionCode {})",
                embedded.version_name, embedded.version_code
            ))
        }
        _ => Ok(format!(
            "installed local APK (device reports versionCode {})",
            after.version_code
        )),
    }
}

fn signature_matches(info: &watchdog_adb::install::PackageInfo, expected: &str) -> bool {
    expected.is_empty()
        || info.signature_sha256.is_empty()
        || info
            .signature_sha256
            .iter()
            .any(|digest| digest.eq_ignore_ascii_case(expected))
}

async fn ensure_permissions(connection: &DeviceConnection, sdk: i64) -> Result<String> {
    let mut permissions = vec![
        "android.permission.CAMERA",
        "android.permission.RECORD_AUDIO",
    ];
    if sdk >= 33 {
        permissions.push("android.permission.POST_NOTIFICATIONS");
    }
    let mut granted = Vec::new();
    let mut warnings = Vec::new();
    for permission in permissions {
        let result = run(connection, &format!("pm grant {PACKAGE} {permission}")).await;
        let short = permission.rsplit('.').next().unwrap_or(permission);
        if result.succeeded() {
            granted.push(short.to_string());
        } else if short == "POST_NOTIFICATIONS" {
            warnings.push(format!(
                "POST_NOTIFICATIONS not granted: {}",
                first_line(&result.output)
            ));
        } else {
            return Err(WatchdogError::new(
                ErrorCode::ProvisioningFailed,
                format!(
                    "pm grant {permission} failed: {}",
                    first_line(&result.output)
                ),
            )
            .with_hint(
                "the app must be installed and declare the permission; \
                 uninstall a stale build and re-run provision",
            ));
        }
    }

    let mut appops = vec![
        "CAMERA",
        "RECORD_AUDIO",
        "WAKE_LOCK",
        "START_FOREGROUND",
        "RUN_ANY_IN_BACKGROUND",
    ];
    if sdk >= 33 {
        appops.push("POST_NOTIFICATION");
    }
    let mut applied = Vec::new();
    for op in appops {
        let result = run(connection, &format!("cmd appops set {PACKAGE} {op} allow")).await;
        if result.succeeded() {
            applied.push(op.to_string());
        } else if matches!(op, "CAMERA" | "RECORD_AUDIO") {
            return Err(WatchdogError::new(
                ErrorCode::ProvisioningFailed,
                format!("cmd appops set {PACKAGE} {op} failed: {}", first_line(&result.output)),
            )
            .with_hint("the device rejected the appop; check the OEM compatibility table in docs/device-setup.md"));
        } else {
            warnings.push(format!(
                "appop {op} skipped: {}",
                first_line(&result.output)
            ));
        }
    }

    let mut detail = format!(
        "granted {}; appops allow {}",
        granted.join(", "),
        applied.join(", ")
    );
    if !warnings.is_empty() {
        detail.push_str(&format!("; warnings: {}", warnings.join("; ")));
    }
    Ok(detail)
}

async fn ensure_battery(connection: &DeviceConnection) -> Result<String> {
    let mut warnings = Vec::new();

    let mut idle = run(
        connection,
        &format!("dumpsys deviceidle whitelist +{PACKAGE}"),
    )
    .await;
    if !idle.succeeded() {
        idle = run(connection, &format!("cmd deviceidle whitelist +{PACKAGE}")).await;
    }
    if !idle.succeeded() {
        warnings.push(format!(
            "deviceidle whitelist: {}",
            first_line(&idle.output)
        ));
    }

    let bucket = run(
        connection,
        &format!("am set-standby-bucket {PACKAGE} active"),
    )
    .await;
    if !bucket.succeeded() {
        warnings.push(format!(
            "set-standby-bucket: {}",
            first_line(&bucket.output)
        ));
    }
    for op in ["RUN_IN_BACKGROUND", "RUN_ANY_IN_BACKGROUND"] {
        let result = run(connection, &format!("cmd appops set {PACKAGE} {op} allow")).await;
        if !result.succeeded() && !result.output.contains("Unknown operation") {
            warnings.push(format!("appop {op}: {}", first_line(&result.output)));
        }
    }

    let bucket_value = run(connection, &format!("am get-standby-bucket {PACKAGE}"))
        .await
        .output
        .trim()
        .to_string();
    let bucket_display = if bucket_value.is_empty() {
        "unknown"
    } else {
        bucket_value.as_str()
    };
    let mut detail =
        format!("Doze whitelist + active standby bucket (device reports {bucket_display})");
    if !bucket_value.is_empty() && bucket_value != "10" {
        warnings.push(format!(
            "standby bucket is {bucket_value}, expected 10 (active)"
        ));
    }
    if !warnings.is_empty() {
        detail.push_str(&format!("; warnings: {}", warnings.join("; ")));
    }
    Ok(detail)
}

async fn ensure_screen(
    connection: &DeviceConnection,
    paths: &DataPaths,
    record: &mut RestoreRecord,
) -> Result<String> {
    const KEY: &str = "global.stay_on_while_plugged_in";
    let original = run(connection, "settings get global stay_on_while_plugged_in").await;
    if !original.succeeded() {
        return Err(WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            format!(
                "cannot read stay_on_while_plugged_in: {}",
                first_line(&original.output)
            ),
        ));
    }
    let original = normalize_setting(&original.output);
    // Preserve the very first original value across repeated provisions.
    record
        .originals
        .entry(KEY.to_string())
        .or_insert(original.clone());
    record.applied_at_ms = now_ms();
    record.save(paths)?;

    let applied = run(connection, "settings put global stay_on_while_plugged_in 7").await;
    if !applied.succeeded() {
        return Err(WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            format!(
                "cannot set stay_on_while_plugged_in: {}",
                first_line(&applied.output)
            ),
        )
        .with_hint("some OEM builds restrict `settings put global`; the screen can still be kept awake manually"));
    }
    record.applied.insert(KEY.to_string(), "7".to_string());
    record.save(paths)?;

    let wake = run(connection, "input keyevent KEYCODE_WAKEUP").await;
    let dismiss = run(connection, "wm dismiss-keyguard").await;
    let mut detail = format!(
        "stay_on_while_plugged_in {} -> 7 (original recorded for restore)",
        original.as_deref().unwrap_or("(unset)")
    );
    if !wake.succeeded() {
        detail.push_str("; wake key failed");
    }
    if !dismiss.succeeded() {
        detail.push_str("; keyguard not dismissed (secure lock? unlock once manually)");
    } else {
        detail.push_str("; screen awake");
    }
    Ok(detail)
}

async fn launch(connection: &DeviceConnection, token: &str) -> Result<String> {
    // Bring the app to the foreground first: some OEMs only allow a
    // foreground-service start from a visible app.
    let activity = run(
        connection,
        &format!("am start -W -n {PACKAGE}/.MainActivity"),
    )
    .await;
    let command = format!(
        "am start-foreground-service -n {PACKAGE}/.CaptureService -a {START_ACTION} \
         --es {SESSION_EXTRA} {token}"
    );
    let result = run(connection, &command).await;
    if !result.succeeded() {
        return Err(WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            format!(
                "cannot start the capture service: {}",
                first_line(&result.output)
            ),
        )
        .with_hint(
            "check that the app is installed and CAMERA is granted; some OEMs block \
             background service starts until the app has been opened once",
        ));
    }
    Ok(if activity.succeeded() {
        "activity foregrounded; capture service started with a fresh session token".to_string()
    } else {
        "capture service started with a fresh session token".to_string()
    })
}

async fn verify(
    connection: &DeviceConnection,
    token: &str,
    options: &ProvisionOptions,
) -> Result<String> {
    let (mut live, ack) = LiveSession::open_authenticated(
        connection,
        token,
        options.socket_timeout,
        options.handshake_timeout,
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
    let frame = live.session().wait_for_frame(options.frame_timeout).await?;
    if frame.jpeg.is_empty() || frame.width == 0 || frame.height == 0 {
        return Err(WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            "the app sent an empty or zero-sized frame",
        )
        .with_hint("check the camera permission and whether another app holds the camera"));
    }
    Ok(format!(
        "handshake ok (app {} versionCode {}, protocol {}); first frame {}x{} JPEG ({} bytes)",
        ack.app.version_name,
        ack.app.version_code,
        ack.protocol_version,
        frame.width,
        frame.height,
        frame.jpeg.len()
    ))
}

/// Per-device record of settings changed by provisioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRecord {
    /// Device serial (or `host:port`).
    pub serial: String,
    /// Session token last used to start the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Unix time (ms) of the last provisioning run.
    #[serde(default)]
    pub applied_at_ms: u64,
    /// Original values keyed by `namespace.name` (`None` = setting was unset).
    #[serde(default)]
    pub originals: BTreeMap<String, Option<String>>,
    /// Values provisioning applied, keyed the same way.
    #[serde(default)]
    pub applied: BTreeMap<String, String>,
}

impl RestoreRecord {
    /// Empty record for `serial`.
    #[must_use]
    pub fn new(serial: &str) -> Self {
        Self {
            serial: serial.to_string(),
            token: None,
            applied_at_ms: 0,
            originals: BTreeMap::new(),
            applied: BTreeMap::new(),
        }
    }

    /// Path of the record for `serial` under `<data_dir>/devices/`.
    #[must_use]
    pub fn path(paths: &DataPaths, serial: &str) -> PathBuf {
        paths
            .root
            .join("devices")
            .join(format!("{}.json", sanitize(serial)))
    }

    /// Load the record, if one exists.
    pub fn load(paths: &DataPaths, serial: &str) -> Result<Option<Self>> {
        let path = Self::path(paths, serial);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let record = serde_json::from_slice(&bytes).map_err(|error| {
            WatchdogError::new(
                ErrorCode::ProvisioningFailed,
                format!("cannot parse {}: {error}", path.display()),
            )
            .with_hint("delete the file if it is corrupt; only screen-settings restoration is lost")
        })?;
        Ok(Some(record))
    }

    /// Persist the record atomically.
    pub fn save(&self, paths: &DataPaths) -> Result<()> {
        let path = Self::path(paths, &self.serial);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(&temporary, bytes)?;
        // Windows refuses to rename over an existing file.
        if path.is_file() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(&temporary, &path)?;
        Ok(())
    }

    /// Delete the record; returns whether a file existed.
    pub fn remove(paths: &DataPaths, serial: &str) -> Result<bool> {
        let path = Self::path(paths, serial);
        if path.is_file() {
            std::fs::remove_file(path)?;
            return Ok(true);
        }
        Ok(false)
    }
}

/// Status snapshot for `watchdog device status`.
#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    /// Device serial (or `host:port`).
    pub serial: String,
    /// Device properties.
    pub props: DeviceProps,
    /// Whether the app package is installed.
    pub installed: bool,
    /// Installed `versionCode`, when readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_code: Option<i64>,
    /// Embedded APK version name, when the host embeds one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version_name: Option<String>,
    /// Embedded APK `versionCode`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version_code: Option<u32>,
    /// Whether the installed build matches the embedded one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_to_date: Option<bool>,
    /// Capture-service PID, when it is running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_pid: Option<u32>,
    /// Last session token recorded by provisioning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Restore record, when provisioning changed settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore: Option<RestoreRecord>,
    /// Current `CAMERA` appop mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appops_camera: Option<String>,
    /// Current `RECORD_AUDIO` appop mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appops_record_audio: Option<String>,
}

/// Collect `watchdog device status` data.
pub async fn status(
    connection: &DeviceConnection,
    serial: &str,
    paths: &DataPaths,
) -> Result<StatusReport> {
    let props = DeviceProps::read(connection).await?;
    let package = connection.query_package(PACKAGE).await?;
    let service_pid = connection
        .shell(&format!("pidof {PACKAGE}"))
        .await
        .ok()
        .and_then(|output| {
            output
                .stdout_string()
                .split_whitespace()
                .next()
                .and_then(|pid| pid.parse::<u32>().ok())
        });
    let record = RestoreRecord::load(paths, serial)?;
    let embedded = apk::embedded();
    let up_to_date = package.as_ref().and_then(|info| {
        embedded.map(|expected| {
            info.version_code == i64::from(expected.version_code)
                && signature_matches(info, expected.signer_sha256)
        })
    });
    Ok(StatusReport {
        serial: serial.to_string(),
        props,
        installed: package.is_some(),
        version_code: package.as_ref().map(|info| info.version_code),
        expected_version_name: embedded.map(|apk| apk.version_name.to_string()),
        expected_version_code: embedded.map(|apk| apk.version_code),
        up_to_date,
        service_pid,
        token: record.as_ref().and_then(|record| record.token.clone()),
        restore: record,
        appops_camera: appop(connection, "CAMERA").await,
        appops_record_audio: appop(connection, "RECORD_AUDIO").await,
    })
}

/// One reverted setting, reported by [`restore`].
#[derive(Debug, Clone, Serialize)]
pub struct RestoreStep {
    /// `namespace.name`.
    pub setting: String,
    /// Value written back (`(unset)` when the setting is deleted).
    pub value: String,
    /// Whether the device accepted the command.
    pub applied: bool,
}

/// Revert settings recorded by provisioning.
pub async fn restore(
    connection: &DeviceConnection,
    serial: &str,
    paths: &DataPaths,
) -> Result<Vec<RestoreStep>> {
    let record = RestoreRecord::load(paths, serial)?.ok_or_else(|| {
        WatchdogError::new(
            ErrorCode::NotFound,
            format!("no provisioning restore record for {serial}"),
        )
        .with_hint("provisioning records screen settings before changing them; run `watchdog device provision` first")
    })?;
    let mut steps = Vec::new();
    let mut failures = Vec::new();
    for (key, original) in &record.originals {
        let Some((namespace, name)) = key.split_once('.') else {
            failures.push(format!("{key}: malformed setting key"));
            continue;
        };
        let command = match original {
            Some(value) => format!("settings put {namespace} {name} {value}"),
            None => format!("settings delete {namespace} {name}"),
        };
        let result = run(connection, &command).await;
        let applied = result.succeeded();
        if !applied {
            failures.push(format!("{key}: {}", first_line(&result.output)));
        }
        steps.push(RestoreStep {
            setting: key.clone(),
            value: original.as_deref().unwrap_or("(unset)").to_string(),
            applied,
        });
    }
    if !failures.is_empty() {
        return Err(WatchdogError::new(
            ErrorCode::ProvisioningFailed,
            format!("restore failed for {}", failures.join("; ")),
        )
        .with_hint(
            "re-run `watchdog device restore`; the record is kept until every setting is restored",
        ));
    }
    RestoreRecord::remove(paths, serial)?;
    Ok(steps)
}

async fn appop(connection: &DeviceConnection, op: &str) -> Option<String> {
    let output = connection
        .shell(&format!("cmd appops get {PACKAGE} {op}"))
        .await
        .ok()?;
    let text = output.stdout_string();
    let line = text.lines().find(|line| line.contains(op))?;
    let (_, mode) = line.split_once(':')?;
    Some(mode.trim().to_string())
}

struct CommandResult {
    ok: bool,
    output: String,
}

impl CommandResult {
    fn succeeded(&self) -> bool {
        self.ok && !error_text(&self.output)
    }
}

async fn run(connection: &DeviceConnection, command: &str) -> CommandResult {
    match connection.shell(command).await {
        Ok(output) => CommandResult {
            ok: output.exit_code.unwrap_or(0) == 0,
            output: format!("{}{}", output.stdout_string(), output.stderr_string())
                .trim()
                .to_string(),
        },
        Err(error) => CommandResult {
            ok: false,
            output: error.to_string(),
        },
    }
}

fn error_text(output: &str) -> bool {
    [
        "Error:",
        "Exception",
        "Unknown option",
        "Unknown command",
        "can't find service",
        "Failure [",
    ]
    .iter()
    .any(|marker| output.contains(marker))
}

fn first_line(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_string()
}

fn normalize_setting(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "null" {
        None
    } else {
        Some(value.to_string())
    }
}

fn sanitize(serial: &str) -> String {
    serial
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Step {
    name: &'static str,
    started: Instant,
}

impl Step {
    fn start(name: &'static str) -> Self {
        Self {
            name,
            started: Instant::now(),
        }
    }

    fn passed(self, detail: impl Into<String>) -> StepReport {
        self.report(StepStatus::Passed, detail, None)
    }

    fn failed(self, detail: impl Into<String>, hint: Option<String>) -> StepReport {
        self.report(StepStatus::Failed, detail, hint)
    }

    fn skipped(self, detail: impl Into<String>) -> StepReport {
        self.report(StepStatus::Skipped, detail, None)
    }

    fn report(
        self,
        status: StepStatus,
        detail: impl Into<String>,
        hint: Option<String>,
    ) -> StepReport {
        StepReport {
            name: self.name,
            status,
            duration_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            detail: detail.into(),
            hint,
        }
    }
}

fn skip_remaining(report: &mut ProvisionReport, names: &[&'static str], reason: &str) {
    for &name in names {
        report.steps.push(StepReport {
            name,
            status: StepStatus::Skipped,
            duration_ms: 0,
            detail: reason.to_string(),
            hint: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_lowercase_hex_characters() {
        let token = generate_token();
        assert_eq!(token.len(), 32);
        assert!(token
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert_ne!(token, generate_token());
    }

    #[test]
    fn serials_are_sanitized_for_file_names() {
        assert_eq!(sanitize("R5CT1234"), "R5CT1234");
        assert_eq!(sanitize("192.0.2.5:5555"), "192_0_2_5_5555");
    }

    #[test]
    fn null_settings_are_treated_as_unset() {
        assert_eq!(normalize_setting("null"), None);
        assert_eq!(normalize_setting(" 0\n"), Some("0".to_string()));
    }

    #[test]
    fn report_fails_when_any_step_failed() {
        let mut report = ProvisionReport::new("serial");
        report.steps.push(StepReport {
            name: "device",
            status: StepStatus::Passed,
            duration_ms: 1,
            detail: "ok".into(),
            hint: None,
        });
        assert!(report.passed());
        report.steps.push(StepReport {
            name: "app",
            status: StepStatus::Failed,
            duration_ms: 2,
            detail: "bad".into(),
            hint: Some("fix".into()),
        });
        assert!(!report.passed());
        assert_eq!(report.failure().map(|step| step.name), Some("app"));
    }
}
