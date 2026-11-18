//! Device management service for the web UI (M6-T03).
//!
//! [`DeviceService`] is the single owner of the host-side device state used by
//! `watchdog serve`: the ADB key pair, the mDNS discovery cache, the one active
//! app session (a [`FrameHub`]) and the background provisioning jobs. The UI
//! server talks to it for every `/api/devices*` route and for the live channel,
//! so a page reload or a second browser never opens a competing session.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};

use watchdog_core::error::{ErrorCode, WatchdogError};
use watchdog_core::paths::DataPaths;
use watchdog_core::Result;
use watchdog_protocol::HelloAck;

use crate::connection::DeviceConnection;
use crate::hub::{FrameHub, HubConfig};
use crate::mock::{self, MockConfig, MockData};
use crate::provision::{
    self, ProvisionOptions, RestoreRecord, RestoreStep, StatusReport, StepReport, PACKAGE,
};
use crate::session::LiveSession;

/// How long activating a session waits for the app socket.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long each activation handshake attempt may take.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the devices page's LAN scan browses mDNS.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Provisioning jobs kept for the UI before the oldest are dropped.
const MAX_JOBS: usize = 20;

/// Lock a `std::sync::Mutex`, recovering the data if a previous holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn generate_job_id() -> String {
    use rand::RngCore;
    let random = rand::thread_rng().next_u64() & 0xffff_ffff_ffff;
    format!("prov-{}-{random:012x}", now_ms())
}

fn is_tcp_target(target: &str) -> bool {
    target.contains(':')
}

fn split_host_port(target: &str) -> Result<(String, u16)> {
    let (host, port_str) = target.rsplit_once(':').unwrap_or((target, "5555"));
    let port: u16 = port_str.parse().map_err(|_| {
        WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("invalid port in target {target:?} (expected host[:port])"),
        )
    })?;
    if host.is_empty() {
        return Err(WatchdogError::new(
            ErrorCode::InvalidRequest,
            format!("invalid target {target:?} (expected host[:port])"),
        ));
    }
    Ok((host.to_string(), port))
}

/// How a device is reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// USB serial.
    Usb,
    /// Wireless debugging over the LAN.
    Lan,
}

/// What the devices page can do with an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// The app session is open and healthy.
    Active,
    /// The app session closed; re-activate to recover.
    Closed,
    /// USB or TCP device known but not activated.
    Available,
    /// Advertised over mDNS but not necessarily reachable yet.
    Discovered,
}

/// One device row on the devices page.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceEntry {
    /// USB serial, `host:port`, or the mDNS GUID when no endpoint is known.
    pub serial: String,
    /// USB or LAN.
    pub kind: DeviceKind,
    /// Current state.
    pub state: DeviceState,
    /// `Build.MODEL`, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// USB product string, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// Wireless-debugging connect endpoint (`host:port`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect: Option<String>,
    /// Wireless-debugging pairing endpoint (`host:port`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<String>,
    /// mDNS device GUID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guid: Option<String>,
    /// When the entry was last seen by discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<u64>,
}

/// The active app session as the UI shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveDevice {
    /// Serial (USB serial or `host:port`).
    pub serial: String,
    /// USB or LAN.
    pub kind: DeviceKind,
    /// `Build.MANUFACTURER`.
    pub manufacturer: String,
    /// `Build.MODEL`.
    pub model: String,
    /// `Build.DEVICE`.
    pub device: String,
    /// `Build.VERSION.SDK_INT`.
    pub sdk: i64,
    /// `Build.VERSION.RELEASE`.
    pub release: String,
    /// Primary ABI.
    pub abi: String,
    /// App package id from `HELLO_ACK`.
    pub app_package: String,
    /// App `versionName`.
    pub app_version_name: String,
    /// App `versionCode`.
    pub app_version_code: i64,
    /// Negotiated protocol version.
    pub protocol_version: u8,
    /// Number of cameras reported by the app.
    pub camera_count: usize,
    /// Battery percentage reported at handshake time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_percent: Option<i64>,
    /// Free internal storage at handshake time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_free_mb: Option<u64>,
}

impl ActiveDevice {
    fn from_ack(serial: &str, ack: &HelloAck) -> Self {
        Self {
            serial: serial.to_string(),
            kind: if is_tcp_target(serial) {
                DeviceKind::Lan
            } else {
                DeviceKind::Usb
            },
            manufacturer: ack.device.manufacturer.clone(),
            model: ack.device.model.clone(),
            device: ack.device.device.clone(),
            sdk: ack.device.sdk,
            release: ack.device.release.clone(),
            abi: ack.device.abi.clone(),
            app_package: ack.app.package.clone(),
            app_version_name: ack.app.version_name.clone(),
            app_version_code: ack.app.version_code,
            protocol_version: ack.protocol_version,
            camera_count: ack.cameras.len(),
            battery_percent: ack.device.battery_percent,
            storage_free_mb: ack.device.storage_free_mb,
        }
    }
}

/// The active session (or the reason there is none) sent with every snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    /// True only while the hub is open and healthy.
    pub active: bool,
    /// True when a session existed but its app connection ended.
    pub closed: bool,
    /// Serial of the active or last session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// Handshake details of the active or last session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<ActiveDevice>,
}

impl SessionInfo {
    /// No session has been opened since the host started.
    fn none() -> Self {
        Self {
            active: false,
            closed: false,
            serial: None,
            device: None,
        }
    }
}

/// Broadcast payload sent to every open WebSocket when the session changes.
#[derive(Debug, Clone)]
pub struct SessionNotice {
    /// The new session state.
    pub session: SessionInfo,
}

/// Response of `GET /api/devices`.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    /// Active session, if any.
    pub session: SessionInfo,
    /// Every known device (active first, then USB, then LAN).
    pub devices: Vec<DeviceEntry>,
    /// When the LAN cache was last refreshed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovered_at_ms: Option<u64>,
}

/// Request body of `POST /api/devices/provision`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProvisionRequest {
    /// Device to provision.
    pub serial: String,
    /// Verify the handshake and first frame after launching.
    #[serde(default = "default_true")]
    pub verify: bool,
    /// Apply the stay-awake/screen settings.
    #[serde(default = "default_true")]
    pub screen: bool,
    /// Local APK override (the UI normally uses the embedded build).
    #[serde(default)]
    pub apk: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

/// Lifecycle of one background provisioning job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionJobStatus {
    /// Still running.
    Running,
    /// Every step passed.
    Passed,
    /// At least one step failed.
    Failed,
}

/// Error details attached to a failed job.
#[derive(Debug, Clone, Serialize)]
pub struct JobError {
    /// Stable error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Actionable remediation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl JobError {
    fn from_watchdog(error: &WatchdogError) -> Self {
        Self {
            code: error.code.as_str().to_string(),
            message: error.message.clone(),
            hint: error.hint.clone(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal.as_str().to_string(),
            message: message.into(),
            hint: None,
        }
    }
}

/// One provisioning job as polled by the devices page.
#[derive(Debug, Clone, Serialize)]
pub struct ProvisionJobState {
    /// Job id returned by `POST /api/devices/provision`.
    pub id: String,
    /// Device being provisioned.
    pub serial: String,
    /// Running/passed/failed.
    pub status: ProvisionJobStatus,
    /// Steps that have completed so far.
    pub steps: Vec<StepReport>,
    /// Job start time (Unix milliseconds).
    pub started_ms: u64,
    /// Job end time (Unix milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    /// Failure details, when a step failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JobError>,
    /// Session opened automatically after a successful run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionInfo>,
    /// Why auto-activation failed even though provisioning passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_error: Option<JobError>,
}

impl ProvisionJobState {
    fn running(id: &str, serial: &str) -> Self {
        Self {
            id: id.to_string(),
            serial: serial.to_string(),
            status: ProvisionJobStatus::Running,
            steps: Vec::new(),
            started_ms: now_ms(),
            finished_ms: None,
            error: None,
            session: None,
            session_error: None,
        }
    }
}

/// One open app session plus the hub that owns it.
struct Active {
    info: ActiveDevice,
    hub: Arc<FrameHub>,
}

/// Cached mDNS scan.
#[derive(Default)]
struct DiscoveryCache {
    devices: Vec<watchdog_adb::DiscoveredDevice>,
    at_ms: Option<u64>,
}

/// Provisioning jobs by id, shared with the polling handler.
type JobMap = Mutex<BTreeMap<String, Arc<Mutex<ProvisionJobState>>>>;

/// Host-side device manager used by `watchdog serve`.
pub struct DeviceService {
    keys: Arc<watchdog_adb::AdbKeyPair>,
    paths: DataPaths,
    active: Mutex<Option<Active>>,
    /// Synthetic backend when `watchdog serve --mock-device` is active.
    mock: Option<Arc<MockData>>,
    /// Serializes activation/deactivation so two clicks cannot open two sessions.
    activation: AsyncMutex<()>,
    /// Serializes mDNS scans (the daemon is not re-entrant friendly).
    discovery: AsyncMutex<()>,
    discovered: Mutex<DiscoveryCache>,
    jobs: JobMap,
    notices: broadcast::Sender<SessionNotice>,
}

impl DeviceService {
    /// Load the ADB key pair and create the service.
    ///
    /// No device is contacted until the UI or `--serial` asks for one.
    pub fn new(paths: DataPaths) -> Result<Self> {
        let keys = watchdog_adb::AdbKeyPair::load_or_create(&paths.keys)?;
        let (notices, _) = broadcast::channel(16);
        Ok(Self {
            keys: Arc::new(keys),
            paths,
            active: Mutex::new(None),
            mock: None,
            activation: AsyncMutex::new(()),
            discovery: AsyncMutex::new(()),
            discovered: Mutex::new(DiscoveryCache::default()),
            jobs: Mutex::new(BTreeMap::new()),
            notices,
        })
    }

    /// Create a service backed by the synthetic device (M6-T09).
    ///
    /// No phone, ADB server or app session is contacted. The mock data store
    /// also backs the temporary `/api/media*`, `/api/settings`, `/api/logs` and
    /// `/api/diagnostics` routes used by the end-to-end suite.
    pub fn new_mock(paths: DataPaths, config: MockConfig) -> Result<Self> {
        let data = Arc::new(MockData::new(paths.clone(), config));
        let mut service = Self::new(paths)?;
        service.mock = Some(data);
        Ok(service)
    }

    /// Whether this host runs the synthetic device.
    #[must_use]
    pub fn is_mock(&self) -> bool {
        self.mock.is_some()
    }

    /// Shared mock data store, when the synthetic device is active.
    #[must_use]
    pub fn mock_data(&self) -> Option<Arc<MockData>> {
        self.mock.clone()
    }

    /// Open the synthetic app session and publish it to the UI.
    pub async fn activate_mock(&self) -> Result<ActiveDevice> {
        let Some(data) = self.mock.clone() else {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                "the host is not running with --mock-device",
            ));
        };
        let _gate = self.activation.lock().await;
        if self.take_active().is_some() {
            self.publish_session();
        }
        let info = mock::active_device();
        let hub = Arc::new(FrameHub::spawn_mock(HubConfig::default(), data));
        *lock(&self.active) = Some(Active {
            info: info.clone(),
            hub,
        });
        self.publish_session();
        tracing::info!(
            device = %info.serial,
            "opened the mock app session for the web UI"
        );
        Ok(info)
    }

    /// Reject operations that only make sense with a real phone.
    fn ensure_real(&self, action: &str) -> Result<()> {
        if self.is_mock() {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                format!("{action} is not available while the host runs the mock device"),
            )
            .with_hint("restart `watchdog serve` without --mock-device to use a real phone"));
        }
        Ok(())
    }

    /// Subscribe to session changes for the WebSocket loop.
    pub fn subscribe_sessions(&self) -> broadcast::Receiver<SessionNotice> {
        self.notices.subscribe()
    }

    /// Data-directory layout (presets, media, logs, ...).
    #[must_use]
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Current session state, including a closed session's last handshake.
    pub fn session_info(&self) -> SessionInfo {
        let guard = lock(&self.active);
        match guard.as_ref() {
            None => SessionInfo::none(),
            Some(active) => {
                let closed = active.hub.stats().closed;
                SessionInfo {
                    active: !closed,
                    closed,
                    serial: Some(active.info.serial.clone()),
                    device: Some(active.info.clone()),
                }
            }
        }
    }

    /// The hub commands and frames should use, or `None` when no session is open.
    pub fn active_hub(&self) -> Option<Arc<FrameHub>> {
        let guard = lock(&self.active);
        guard
            .as_ref()
            .filter(|active| !active.hub.stats().closed)
            .map(|active| Arc::clone(&active.hub))
    }

    /// The active device, when the session is healthy.
    pub fn active_device(&self) -> Option<ActiveDevice> {
        let guard = lock(&self.active);
        guard
            .as_ref()
            .filter(|active| !active.hub.stats().closed)
            .map(|active| active.info.clone())
    }

    /// List known devices, optionally refreshing the mDNS cache first.
    pub async fn list(&self, discover: bool) -> Result<DeviceList> {
        if discover && !self.is_mock() {
            let _gate = self.discovery.lock().await;
            let found = watchdog_adb::discover(DISCOVERY_TIMEOUT).await?;
            let mut cache = lock(&self.discovered);
            cache.devices = found;
            cache.at_ms = Some(now_ms());
        }
        Ok(self.build_list())
    }

    fn build_list(&self) -> DeviceList {
        let session = self.session_info();
        let discovered_at_ms = lock(&self.discovered).at_ms;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();

        if let Some(device) = &session.device {
            seen.insert(device.serial.clone());
            entries.push(DeviceEntry {
                serial: device.serial.clone(),
                kind: device.kind,
                state: if session.active {
                    DeviceState::Active
                } else {
                    DeviceState::Closed
                },
                model: Some(device.model.clone()),
                product: None,
                connect: None,
                pairing: None,
                guid: None,
                last_seen_ms: None,
            });
        }

        if self.is_mock() {
            // The synthetic device is always available, even after the UI
            // disconnects it, so the devices page can activate it again.
            if session.device.is_none() {
                let info = mock::active_device();
                entries.push(DeviceEntry {
                    serial: info.serial,
                    kind: info.kind,
                    state: if session.closed {
                        DeviceState::Closed
                    } else {
                        DeviceState::Available
                    },
                    model: Some(info.model),
                    product: Some("watchdog mock device".to_string()),
                    connect: None,
                    pairing: None,
                    guid: None,
                    last_seen_ms: Some(now_ms()),
                });
            }
            return DeviceList {
                session,
                devices: entries,
                discovered_at_ms: None,
            };
        }

        for device in watchdog_adb::list_usb() {
            let serial = device.id();
            if !seen.insert(serial.clone()) {
                continue;
            }
            entries.push(DeviceEntry {
                serial,
                kind: DeviceKind::Usb,
                state: DeviceState::Available,
                model: None,
                product: Some(device.product),
                connect: None,
                pairing: None,
                guid: None,
                last_seen_ms: None,
            });
        }

        for device in &lock(&self.discovered).devices {
            let serial = device
                .connect
                .map_or_else(|| device.guid.clone(), |addr| addr.to_string());
            if serial.is_empty() || !seen.insert(serial.clone()) {
                continue;
            }
            entries.push(DeviceEntry {
                serial,
                kind: DeviceKind::Lan,
                state: DeviceState::Discovered,
                model: None,
                product: None,
                connect: device.connect.map(|addr| addr.to_string()),
                pairing: device.pairing.map(|addr| addr.to_string()),
                guid: Some(device.guid.clone()),
                last_seen_ms: discovered_at_ms,
            });
        }

        DeviceList {
            session,
            devices: entries,
            discovered_at_ms,
        }
    }

    /// Pair with an Android 11+ wireless-debugging endpoint.
    pub async fn pair(&self, target: &str, code: &str) -> Result<String> {
        self.ensure_real("wireless pairing")?;
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                format!("invalid pairing code {code:?} (expected 6 digits)"),
            ));
        }
        let (host, port) = split_host_port(target)?;
        let stream = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|error| {
                WatchdogError::new(
                    ErrorCode::PairingFailed,
                    format!("pairing connect to {host}:{port} failed: {error}"),
                )
                .with_hint("open 'Pair device with pairing code' on the device and use its port")
            })?;
        let userkey = self.keys.public_key_userkey();
        let outcome = watchdog_adb::pair(stream, code, &self.keys, &userkey).await?;
        Ok(outcome.device_guid)
    }

    /// Read `watchdog device status` for one device.
    pub async fn status(&self, serial: &str) -> Result<StatusReport> {
        if self.is_mock() {
            return Ok(mock::status_report(serial));
        }
        let (connection, id) = DeviceConnection::connect(&self.keys, Some(serial)).await?;
        provision::status(&connection, &id, &self.paths).await
    }

    /// Revert host-side changes provisioning made, then drop the session.
    pub async fn restore(&self, serial: &str) -> Result<(String, Vec<RestoreStep>)> {
        if self.is_mock() {
            return Ok((serial.to_string(), Vec::new()));
        }
        let (connection, id) = DeviceConnection::connect(&self.keys, Some(serial)).await?;
        let steps = provision::restore(&connection, &id, &self.paths).await?;
        self.deactivate_if(&id).await;
        Ok((id, steps))
    }

    /// Open the app session for `serial`, replacing any current session.
    pub async fn activate(&self, serial: &str) -> Result<ActiveDevice> {
        if self.is_mock() {
            return self.activate_mock().await;
        }
        let _gate = self.activation.lock().await;
        if self.take_active().is_some() {
            self.publish_session();
        }
        match self.open_session(serial).await {
            Ok(active) => {
                let info = active.info.clone();
                *lock(&self.active) = Some(active);
                self.publish_session();
                tracing::info!(
                    device = %info.serial,
                    app = %info.app_version_name,
                    "opened the app session for the web UI"
                );
                Ok(info)
            }
            Err(error) => {
                self.publish_session();
                Err(error)
            }
        }
    }

    /// Close the current session, if any.
    pub async fn deactivate(&self) -> Option<ActiveDevice> {
        let _gate = self.activation.lock().await;
        let removed = self.take_active();
        if removed.is_some() {
            self.publish_session();
        }
        removed.map(|active| active.info)
    }

    async fn deactivate_if(&self, serial: &str) {
        let _gate = self.activation.lock().await;
        let matches = lock(&self.active)
            .as_ref()
            .is_some_and(|active| active.info.serial == serial);
        if matches {
            self.take_active();
            self.publish_session();
        }
    }

    fn take_active(&self) -> Option<Active> {
        lock(&self.active).take()
    }

    fn publish_session(&self) {
        let _ = self.notices.send(SessionNotice {
            session: self.session_info(),
        });
    }

    async fn open_session(&self, serial: &str) -> Result<Active> {
        let (connection, id) = DeviceConnection::connect(&self.keys, Some(serial))
            .await
            .map_err(|error| {
                error.with_hint(
                    "check that wireless debugging is enabled and the device is paired; the devices page can pair it",
                )
            })?;
        let token = RestoreRecord::load(&self.paths, &id)?
            .and_then(|record| record.token)
            .ok_or_else(|| {
                WatchdogError::new(
                    ErrorCode::NotFound,
                    format!("no session token recorded for {id}"),
                )
                .with_hint(
                    "provision the device first; the web UI reuses the provisioning session token",
                )
            })?;
        let (live, ack) = LiveSession::open_authenticated(
            &connection,
            &token,
            ACTIVATE_TIMEOUT,
            HANDSHAKE_TIMEOUT,
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
        Ok(Active {
            info: ActiveDevice::from_ack(&id, &ack),
            hub: Arc::new(FrameHub::spawn(live, HubConfig::default())),
        })
    }

    /// Start a background provisioning job; returns its id immediately.
    pub fn start_provision(self: &Arc<Self>, request: ProvisionRequest) -> Result<String> {
        self.ensure_real("provisioning")?;
        if request.serial.trim().is_empty() {
            return Err(WatchdogError::new(
                ErrorCode::InvalidRequest,
                "serial must not be empty",
            ));
        }
        let id = generate_job_id();
        let job = Arc::new(Mutex::new(ProvisionJobState::running(&id, &request.serial)));
        {
            let mut jobs = lock(&self.jobs);
            while jobs.len() >= MAX_JOBS {
                let Some(oldest) = jobs.keys().next().cloned() else {
                    break;
                };
                jobs.remove(&oldest);
            }
            jobs.insert(id.clone(), Arc::clone(&job));
        }
        let service = Arc::clone(self);
        let job_id = id.clone();
        tokio::spawn(async move {
            service.run_provision(job_id, request, job).await;
        });
        Ok(id)
    }

    /// Snapshot of one provisioning job.
    pub fn provision_job(&self, id: &str) -> Option<ProvisionJobState> {
        lock(&self.jobs).get(id).map(|job| lock(job).clone())
    }

    async fn run_provision(
        self: Arc<Self>,
        id: String,
        request: ProvisionRequest,
        job: Arc<Mutex<ProvisionJobState>>,
    ) {
        let (progress, mut steps) = mpsc::unbounded_channel();
        let options = ProvisionOptions {
            apk: request.apk.clone(),
            token: None,
            verify_frame: request.verify,
            socket_timeout: Duration::from_secs(20),
            handshake_timeout: Duration::from_secs(10),
            frame_timeout: Duration::from_secs(30),
            skip_screen: !request.screen,
            progress: Some(progress),
        };
        let keys = Arc::clone(&self.keys);
        let paths = self.paths.clone();
        let serial = request.serial.clone();
        let mut task = tokio::spawn(async move {
            let (connection, resolved) = DeviceConnection::connect(&keys, Some(&serial)).await?;
            let report = provision::provision(&connection, &resolved, &paths, &options).await;
            Ok::<_, WatchdogError>((resolved, report))
        });

        let outcome = loop {
            tokio::select! {
                Some(step) = steps.recv() => {
                    lock(&job).steps.push(step);
                }
                result = &mut task => break result,
            }
        };
        while let Ok(step) = steps.try_recv() {
            lock(&job).steps.push(step);
        }

        match outcome {
            Ok(Ok((resolved, report))) => {
                let passed = report.passed();
                let failure = report.failure().map(|step| JobError {
                    code: ErrorCode::ProvisioningFailed.as_str().to_string(),
                    message: format!("provisioning failed at step {}: {}", step.name, step.detail),
                    hint: step.hint.clone(),
                });
                {
                    let mut state = lock(&job);
                    state.steps = report.steps;
                    state.status = if passed {
                        ProvisionJobStatus::Passed
                    } else {
                        ProvisionJobStatus::Failed
                    };
                    state.error = failure;
                    state.finished_ms = Some(now_ms());
                }
                if passed {
                    match self.activate(&resolved).await {
                        Ok(device) => {
                            let session = SessionInfo {
                                active: true,
                                closed: false,
                                serial: Some(device.serial.clone()),
                                device: Some(device),
                            };
                            lock(&job).session = Some(session);
                        }
                        Err(error) => {
                            tracing::warn!(
                                job = %id,
                                %error,
                                "provisioning passed but the session did not open"
                            );
                            lock(&job).session_error = Some(JobError::from_watchdog(&error));
                        }
                    }
                }
            }
            Ok(Err(error)) => {
                let mut state = lock(&job);
                state.status = ProvisionJobStatus::Failed;
                state.error = Some(JobError::from_watchdog(&error));
                state.finished_ms = Some(now_ms());
            }
            Err(error) => {
                let mut state = lock(&job);
                state.status = ProvisionJobStatus::Failed;
                state.error = Some(JobError::internal(format!(
                    "provisioning task stopped unexpectedly: {error}"
                )));
                state.finished_ms = Some(now_ms());
            }
        }
    }
}
