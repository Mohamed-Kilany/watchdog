//! Synthetic device backend for UI end-to-end tests (M6-T09).
//!
//! `watchdog serve --mock-device` runs the real host, the real UI server and
//! the real frame hub against this module instead of an Android app session.
//! It publishes a static JPEG viewfinder frame at a steady rate, merges control
//! changes, answers the capture/recording commands and emits the same event and
//! transfer envelopes the app will send once M5 lands.
//!
//! [`MockData`] also backs the M6-T07/T08 routes (`/api/media*`,
//! `/api/settings`, `/api/logs`, `/api/diagnostics`) so Playwright can exercise
//! every page against the real host without a phone. Those routes are mock-only:
//! the real stores arrive with M5/M7/M9.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use watchdog_core::paths::DataPaths;
use watchdog_protocol::{
    CommandMessage, CommandResultMessage, ControlEntry, ControlResult, ControlSource, EventMessage,
    Rotation, StateMessage,
};

use crate::devices::{ActiveDevice, DeviceKind};
use crate::hub::ReceivedFrame;
use crate::provision::{DeviceProps, RestoreRecord, StatusReport};

/// JPEG viewfinder frame published by the mock device.
const MOCK_FRAME: &[u8] = include_bytes!("../assets/mock-frame.jpg");
/// WebM clip served for mock video media.
const MOCK_CLIP: &[u8] = include_bytes!("../assets/mock-clip.webm");
/// Viewfinder width in pixels.
pub const FRAME_WIDTH: u32 = 320;
/// Viewfinder height in pixels.
pub const FRAME_HEIGHT: u32 = 180;
/// Interval between synthetic frames (10 fps keeps CI cheap).
pub const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Oldest log lines kept by the mock ring buffer.
const MAX_LOG_LINES: usize = 300;
/// Seeded media items kept in the mock library.
const SEEDED_MEDIA: usize = 3;

/// Ports, listen addresses and token state reported by `/api/settings`.
#[derive(Debug, Clone)]
pub struct MockConfig {
    /// UI bind address.
    pub ui_listen: String,
    /// MCP bind address.
    pub mcp_listen: String,
    /// Integration API bind address.
    pub api_listen: String,
    /// UI port.
    pub ui_port: u16,
    /// MCP port.
    pub mcp_port: u16,
    /// Integration API port.
    pub api_port: u16,
    /// Non-secret bearer-token hint (`...c0de`), when a token is configured.
    pub token_hint: Option<String>,
}

/// State shared by the mock hub, the mock media library and the mock APIs.
pub struct MockData {
    config: MockConfig,
    paths: DataPaths,
    media: Mutex<Vec<Value>>,
    logs: Mutex<VecDeque<Value>>,
    retention: Mutex<(u64, u64)>,
    recording_started_ms: Mutex<Option<u64>>,
    capture_seq: AtomicU64,
    analysis_seq: AtomicU64,
}

impl MockData {
    /// Create the mock data store, seeding the media library and log history.
    pub fn new(paths: DataPaths, config: MockConfig) -> Self {
        let data = Self {
            config,
            paths,
            media: Mutex::new(Vec::new()),
            logs: Mutex::new(VecDeque::new()),
            retention: Mutex::new((30, 10_240)),
            recording_started_ms: Mutex::new(None),
            capture_seq: AtomicU64::new(0),
            analysis_seq: AtomicU64::new(0),
        };
        data.seed_media();
        data.seed_logs();
        data
    }

    /// Data-directory layout used by the settings page.
    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Configured ports (UI, MCP, API).
    pub fn ports(&self) -> (u16, u16, u16) {
        (
            self.config.ui_port,
            self.config.mcp_port,
            self.config.api_port,
        )
    }

    /// Non-secret token hint, when a bearer token is configured.
    pub fn token_hint(&self) -> Option<&str> {
        self.config.token_hint.as_deref()
    }

    /// Every media item, newest first.
    pub fn list_media(&self) -> Vec<Value> {
        let mut items = lock(&self.media).clone();
        items.sort_by_key(|item| std::cmp::Reverse(number(item, "created_ms")));
        items
    }

    /// One media item by id.
    pub fn get_media(&self, id: &str) -> Option<Value> {
        lock(&self.media)
            .iter()
            .find(|item| item["id"].as_str() == Some(id))
            .cloned()
    }

    /// Bytes and content type of one media file.
    pub fn media_file(&self, id: &str) -> Option<(&'static [u8], &'static str)> {
        let items = lock(&self.media);
        let item = items.iter().find(|item| item["id"].as_str() == Some(id))?;
        if item["type"].as_str() == Some("video") {
            Some((MOCK_CLIP, "video/webm"))
        } else {
            Some((MOCK_FRAME, "image/jpeg"))
        }
    }

    /// Remove one media item; returns false when it did not exist.
    pub fn delete_media(&self, id: &str) -> bool {
        let removed = {
            let mut items = lock(&self.media);
            let before = items.len();
            items.retain(|item| item["id"].as_str() != Some(id));
            items.len() != before
        };
        if removed {
            self.append_log("info", "media", &format!("deleted media {id}"));
        }
        removed
    }

    /// Submit media to the mock analysis queue.
    ///
    /// The real queue arrives with M7; this only validates the ids and returns a
    /// queued id so the media page can be exercised end to end.
    pub fn submit_analysis(&self, ids: &[String]) -> Result<Value, String> {
        if ids.is_empty() {
            return Err("media_ids must not be empty".to_string());
        }
        for id in ids {
            if self.get_media(id).is_none() {
                return Err(format!("media {id} was not found"));
            }
        }
        let seq = self.analysis_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("an-mock-{seq}");
        self.append_log(
            "info",
            "analysis",
            &format!("queued analysis {id} for {} item(s)", ids.len()),
        );
        Ok(json!({ "analysis_id": id, "state": "queued" }))
    }

    /// Effective settings payload for `GET /api/settings`.
    pub fn settings(&self) -> Value {
        let items = lock(&self.media);
        let media_items = items.len();
        let media_bytes: u64 = items.iter().map(|item| number(item, "bytes")).sum();
        drop(items);
        let (max_age_days, max_size_mb) = *lock(&self.retention);
        let addresses = json!({
            "ui": self.config.ui_listen,
            "mcp": self.config.mcp_listen,
            "api": self.config.api_listen,
        });
        let loopback = |address: &str| address == "127.0.0.1" || address == "::1";
        let exposed = [
            &self.config.ui_listen,
            &self.config.mcp_listen,
            &self.config.api_listen,
        ]
        .iter()
        .filter(|address| !loopback(address.as_str()))
        .count();
        let mode = if exposed == 0 {
            "localhost"
        } else if exposed == 3 {
            "lan"
        } else {
            "mixed"
        };
        json!({
            "ports": {
                "ui": self.config.ui_port,
                "mcp": self.config.mcp_port,
                "api": self.config.api_port,
            },
            "listen": { "mode": mode, "addresses": addresses },
            "token": {
                "configured": self.config.token_hint.is_some(),
                "hint": self.config.token_hint,
            },
            "storage": {
                "data_dir": self.paths.root.display().to_string(),
                "media_dir": self.paths.media.display().to_string(),
                "logs_dir": self.paths.logs.display().to_string(),
                "media_items": media_items,
                "media_bytes": media_bytes,
                "free_bytes": 128_u64 * 1024 * 1024 * 1024,
                "total_bytes": 512_u64 * 1024 * 1024 * 1024,
            },
            "retention": {
                "max_age_days": max_age_days,
                "max_size_mb": max_size_mb,
            },
            "ffmpeg": { "available": false },
            "about": {
                "version": env!("CARGO_PKG_VERSION"),
                "commit": "mock-device",
                "build": "mock-device",
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "protocol": watchdog_protocol::PROTOCOL_VERSION,
            },
        })
    }

    /// Persist new retention limits; `0` keeps media forever / unlimited.
    pub fn update_retention(&self, max_age_days: i64, max_size_mb: i64) -> Value {
        let days = max_age_days.max(0) as u64;
        let size = max_size_mb.max(0) as u64;
        *lock(&self.retention) = (days, size);
        self.append_log(
            "info",
            "settings",
            &format!("retention updated: {days} days, {size} MiB"),
        );
        json!({ "max_age_days": days, "max_size_mb": size })
    }

    /// Recent log lines, filtered by level and capped at `limit`.
    pub fn logs(&self, level: Option<&str>, limit: usize) -> Vec<Value> {
        let limit = limit.clamp(1, 1_000);
        let level = level.unwrap_or("all");
        let lines: Vec<Value> = lock(&self.logs)
            .iter()
            .filter(|line| level == "all" || line["level"].as_str() == Some(level))
            .cloned()
            .collect();
        lines.into_iter().rev().take(limit).rev().collect()
    }

    /// JSON diagnostics bundle contents; the UI server wraps it in a zip.
    pub fn diagnostics_json(&self) -> Vec<u8> {
        let settings = self.settings();
        let payload = json!({
            "generated_ms": now_ms(),
            "about": settings["about"],
            "settings": {
                "ports": settings["ports"],
                "listen": settings["listen"],
                "token_configured": settings["token"]["configured"],
                "retention": settings["retention"],
            },
            "media": self
                .list_media()
                .iter()
                .map(|item| {
                    json!({
                        "id": item["id"],
                        "type": item["type"],
                        "bytes": item["bytes"],
                        "sha256": item["sha256"],
                    })
                })
                .collect::<Vec<_>>(),
            "logs": lock(&self.logs).iter().cloned().collect::<Vec<_>>(),
        });
        serde_json::to_vec_pretty(&payload).unwrap_or_default()
    }

    /// Append one log line, keeping the ring buffer bounded.
    pub fn append_log(&self, level: &str, target: &str, message: &str) {
        let mut logs = lock(&self.logs);
        logs.push_back(json!({
            "ts_ms": now_ms(),
            "level": level,
            "target": target,
            "message": message,
        }));
        while logs.len() > MAX_LOG_LINES {
            logs.pop_front();
        }
    }

    /// Record a synthetic still or video and return its media manifest entry.
    fn add_capture(&self, kind: &str, duration_ms: Option<u64>) -> Value {
        let seq = self.capture_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let bytes = if kind == "video" {
            MOCK_CLIP.len()
        } else {
            MOCK_FRAME.len()
        };
        let id = format!("m-mock-{kind}-{seq}");
        let item = json!({
            "id": id,
            "type": kind,
            "created_ms": now_ms(),
            "duration_ms": duration_ms,
            "width": FRAME_WIDTH,
            "height": FRAME_HEIGHT,
            "bytes": bytes,
            "sha256": fake_sha256(&id),
            "mime": if kind == "video" { "video/webm" } else { "image/jpeg" },
            "device": {
                "model": "Mock device",
                "serial": "MOCK-0001",
                "sdk": 34,
            },
            "control_snapshot": control_snapshot(),
        });
        lock(&self.media).push(item.clone());
        self.append_log(
            "info",
            "media",
            &format!("captured {kind} {id} ({bytes} bytes)"),
        );
        item
    }

    fn seed_media(&self) {
        let now = now_ms();
        let device = json!({
            "model": "Mock device",
            "serial": "MOCK-0001",
            "sdk": 34,
        });
        let still = |id: &str, ago: u64| {
            json!({
                "id": id,
                "type": "still",
                "created_ms": now.saturating_sub(ago),
                "width": FRAME_WIDTH,
                "height": FRAME_HEIGHT,
                "bytes": MOCK_FRAME.len(),
                "sha256": fake_sha256(id),
                "mime": "image/jpeg",
                "device": device,
                "control_snapshot": control_snapshot(),
            })
        };
        let video = json!({
            "id": "m-mock-seed-video-1",
            "type": "video",
            "created_ms": now.saturating_sub(2 * 60 * 60 * 1000),
            "duration_ms": 1_500,
            "width": FRAME_WIDTH,
            "height": FRAME_HEIGHT,
            "bytes": MOCK_CLIP.len(),
            "sha256": fake_sha256("m-mock-seed-video-1"),
            "mime": "video/webm",
            "device": device,
            "control_snapshot": control_snapshot(),
        });
        let items = vec![
            still("m-mock-seed-still-1", 90_000),
            video,
            still("m-mock-seed-still-2", 24 * 60 * 60 * 1000),
        ];
        debug_assert_eq!(items.len(), SEEDED_MEDIA);
        lock(&self.media).extend(items);
    }

    fn seed_logs(&self) {
        let now = now_ms();
        let seed: [(&str, &str, &str); 13] = [
            ("info", "host", "watchdog mock device starting"),
            ("info", "config", "mock configuration loaded"),
            ("debug", "config", "loaded mock configuration"),
            ("info", "ui", "web UI listening on the configured port"),
            ("info", "device", "mock session MOCK-0001 activated"),
            ("info", "stream", "viewfinder 320x180 at 10 fps"),
            ("debug", "stream", "synthetic frame published"),
            ("info", "media", "mock media library seeded with 3 items"),
            ("info", "settings", "retention loaded: 30 days, 10240 MiB"),
            (
                "info",
                "analysis",
                "analyzer engine is not bundled in the mock",
            ),
            ("error", "analysis", "no analyzer engine yet"),
            ("info", "host", "disk 128 GiB free of 512 GiB"),
            ("info", "host", "mock device ready for Playwright"),
        ];
        let mut logs = lock(&self.logs);
        for (index, (level, target, message)) in seed.iter().enumerate() {
            let ago = ((seed.len() - index) as u64) * 1_000;
            logs.push_back(json!({
                "ts_ms": now.saturating_sub(ago),
                "level": level,
                "target": target,
                "message": message,
            }));
        }
    }
}

/// One command outcome plus the updates the hub should broadcast.
pub struct MockCommandOutcome {
    /// Reply sent to the caller.
    pub result: CommandResultMessage,
    /// New merged state, when the command changed a control.
    pub state_update: Option<StateMessage>,
    /// Asynchronous app events.
    pub events: Vec<EventMessage>,
    /// Transfer progress envelopes.
    pub transfers: Vec<Value>,
}

/// Handle one command exactly like the app would.
pub fn handle_command(
    command: &CommandMessage,
    state: &mut StateMessage,
    data: &MockData,
) -> MockCommandOutcome {
    let mut events = Vec::new();
    let mut transfers = Vec::new();
    let mut state_update = None;
    let results = match command.command.as_str() {
        "get_state" => vec![ok("command", None)],
        "set_controls" => {
            let controls = command
                .params
                .get("controls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut results = Vec::with_capacity(controls.len());
            for control in controls {
                let Some(id) = control["id"].as_str() else {
                    continue;
                };
                let value = control.get("value").cloned().unwrap_or(Value::Null);
                apply_control(state, id, &value);
                results.push(ok(id, Some(value)));
            }
            if !results.is_empty() {
                bump(state);
                state_update = Some(state.clone());
                data.append_log(
                    "debug",
                    "controls",
                    &format!("applied {} change(s)", results.len()),
                );
            }
            results
        }
        "capture_still" => {
            let item = data.add_capture("still", None);
            let id = text(&item, "id");
            transfers.extend(transfer_sequence(&id, number(&item, "bytes")));
            events.push(event(
                "capture_finished",
                json!({ "media_id": id, "type": "still" }),
            ));
            vec![ok("command", Some(capture_payload(&item)))]
        }
        "capture_burst" => {
            let count = command
                .params
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(1, 50);
            let mut media_ids = Vec::with_capacity(count as usize);
            for done in 1..=count {
                let item = data.add_capture("still", None);
                let id = text(&item, "id");
                media_ids.push(id.clone());
                transfers.extend(transfer_sequence(&id, number(&item, "bytes")));
                events.push(event(
                    "capture_progress",
                    json!({ "done": done, "total": count }),
                ));
            }
            let sequence_id = format!("burst-mock-{}", now_ms());
            vec![ok(
                "command",
                Some(json!({ "sequence_id": sequence_id, "media_ids": media_ids })),
            )]
        }
        "record_start" => {
            let started_ms = now_ms();
            *lock(&data.recording_started_ms) = Some(started_ms);
            set_recording(state, true);
            state_update = Some(state.clone());
            events.push(event(
                "recording_started",
                json!({ "started_ms": started_ms }),
            ));
            data.append_log("info", "media", "recording started");
            vec![ok(
                "command",
                Some(json!({
                    "type": "video",
                    "media_id": "m-mock-recording",
                    "started_ms": started_ms,
                })),
            )]
        }
        "record_stop" => {
            let started = lock(&data.recording_started_ms).take();
            let duration_ms = started.map_or(0, |start| now_ms().saturating_sub(start));
            let item = data.add_capture("video", Some(duration_ms));
            let id = text(&item, "id");
            set_recording(state, false);
            state_update = Some(state.clone());
            events.push(event(
                "recording_stopped",
                json!({ "duration_ms": duration_ms, "media_id": id }),
            ));
            transfers.extend(transfer_sequence(&id, number(&item, "bytes")));
            data.append_log(
                "info",
                "media",
                &format!("recording stopped after {duration_ms} ms"),
            );
            vec![ok(
                "command",
                Some(json!({
                    "type": "video",
                    "media_id": id,
                    "duration_ms": duration_ms,
                    "bytes": number(&item, "bytes"),
                })),
            )]
        }
        other => {
            data.append_log("debug", "commands", &format!("accepted {other}"));
            vec![ok("command", None)]
        }
    };
    MockCommandOutcome {
        result: CommandResultMessage {
            id: command.id.clone(),
            timestamp_ms: now_ms(),
            results,
            error: None,
        },
        state_update,
        events,
        transfers,
    }
}

/// Build the initial merged state reported by the mock app.
pub fn initial_state() -> StateMessage {
    let mut state = StateMessage {
        seq: 1,
        timestamp_ms: now_ms(),
        camera_id: "0".to_string(),
        state: std::collections::BTreeMap::new(),
        dropped_frames: 0,
        storage_free_mb: Some(24_576),
        delta: false,
    };
    for (id, value) in writable_defaults() {
        let entry = if matches!(id, "iso" | "exposure_time_ns" | "frame_duration_ns") {
            auto(value)
        } else {
            host(value)
        };
        state.state.insert(id.to_string(), entry);
    }
    for (id, value) in readout_defaults() {
        state.state.insert(id.to_string(), device(value));
    }
    for id in ["ae_regions", "af_regions", "awb_regions"] {
        state.state.insert(
            id.to_string(),
            ControlEntry {
                requested: Some(json!([])),
                actual: Some(json!([])),
                locked: None,
                supported: true,
                source: ControlSource::Host,
            },
        );
    }
    for id in ["ae_precapture", "af_trigger"] {
        state.state.insert(
            id.to_string(),
            ControlEntry {
                requested: None,
                actual: None,
                locked: None,
                supported: true,
                source: ControlSource::Host,
            },
        );
    }
    state
}

/// One synthetic viewfinder frame.
pub fn frame(seq: u64) -> ReceivedFrame {
    ReceivedFrame {
        seq,
        sensor_timestamp_ns: now_ns(),
        width: FRAME_WIDTH,
        height: FRAME_HEIGHT,
        rotation: Rotation::R0,
        flags: 0,
        jpeg: bytes::Bytes::from_static(MOCK_FRAME),
        received_at: Instant::now(),
        latency_ms: Some(12.0),
    }
}

/// Device identity shown on the devices page and in media metadata.
pub fn active_device() -> ActiveDevice {
    ActiveDevice {
        serial: "MOCK-0001".to_string(),
        kind: DeviceKind::Usb,
        manufacturer: "watchdog".to_string(),
        model: "Mock device".to_string(),
        device: "watchdog-mock".to_string(),
        sdk: 34,
        release: "14".to_string(),
        abi: std::env::consts::ARCH.to_string(),
        app_package: "com.watchdog.camera".to_string(),
        app_version_name: "0.1.0-mock".to_string(),
        app_version_code: 1,
        protocol_version: watchdog_protocol::PROTOCOL_VERSION,
        camera_count: 2,
        battery_percent: Some(87),
        storage_free_mb: Some(24_576),
    }
}

/// Synthetic `watchdog device status` report for the mock device.
pub fn status_report(serial: &str) -> StatusReport {
    let info = active_device();
    let mut restore = RestoreRecord::new(serial);
    restore.applied_at_ms = now_ms();
    StatusReport {
        serial: serial.to_string(),
        props: DeviceProps {
            sdk: info.sdk,
            abi: info.abi,
            model: info.model,
            manufacturer: info.manufacturer,
            release: info.release,
            device: info.device,
        },
        installed: true,
        version_code: Some(info.app_version_code),
        expected_version_name: Some(info.app_version_name.clone()),
        expected_version_code: Some(u32::try_from(info.app_version_code).unwrap_or(1)),
        up_to_date: Some(true),
        service_pid: Some(4242),
        token: None,
        restore: Some(restore),
        appops_camera: Some("allow".to_string()),
        appops_record_audio: Some("allow".to_string()),
    }
}

fn apply_control(state: &mut StateMessage, id: &str, value: &Value) {
    let entry = state.state.entry(id.to_string()).or_insert(ControlEntry {
        requested: None,
        actual: None,
        locked: None,
        supported: true,
        source: ControlSource::Host,
    });
    entry.requested = Some(value.clone());
    entry.actual = Some(value.clone());
    entry.source = ControlSource::Host;
    entry.supported = true;
    if id.ends_with("_lock") {
        entry.locked = Some(value.as_bool().unwrap_or(false));
    }
}

fn set_recording(state: &mut StateMessage, recording: bool) {
    let entry = state
        .state
        .entry("recording".to_string())
        .or_insert_with(|| device(json!(false)));
    entry.actual = Some(json!(recording));
    entry.source = ControlSource::Device;
    bump(state);
}

fn bump(state: &mut StateMessage) {
    state.seq = state.seq.saturating_add(1);
    state.timestamp_ms = now_ms();
}

fn transfer_sequence(media_id: &str, bytes_total: u64) -> Vec<Value> {
    let transfer_id = format!("t-mock-{media_id}");
    let steps = [
        ("queued", 0_u64),
        ("transferring", bytes_total / 2),
        ("verifying", bytes_total),
        ("done", bytes_total),
    ];
    steps
        .iter()
        .map(|(state, bytes_done)| {
            json!({
                "transfer_id": transfer_id,
                "media_id": media_id,
                "state": state,
                "bytes_total": bytes_total,
                "bytes_done": bytes_done,
            })
        })
        .collect()
}

fn capture_payload(item: &Value) -> Value {
    json!({
        "media_id": item["id"],
        "type": item["type"],
        "width": item["width"],
        "height": item["height"],
        "bytes": item["bytes"],
    })
}

fn event(kind: &str, data: Value) -> EventMessage {
    EventMessage {
        kind: kind.to_string(),
        timestamp_ms: now_ms(),
        data: Some(data),
    }
}

fn ok(control: &str, applied: Option<Value>) -> ControlResult {
    ControlResult {
        control: Some(control.to_string()),
        ok: true,
        applied,
        error_code: None,
        message: None,
    }
}

fn host(value: Value) -> ControlEntry {
    ControlEntry {
        requested: Some(value.clone()),
        actual: Some(value),
        locked: None,
        supported: true,
        source: ControlSource::Host,
    }
}

fn auto(value: Value) -> ControlEntry {
    ControlEntry {
        requested: None,
        actual: Some(value),
        locked: None,
        supported: true,
        source: ControlSource::Auto,
    }
}

fn device(value: Value) -> ControlEntry {
    ControlEntry {
        requested: None,
        actual: Some(value),
        locked: None,
        supported: true,
        source: ControlSource::Device,
    }
}

fn writable_defaults() -> Vec<(&'static str, Value)> {
    vec![
        ("control_mode", json!(1)),
        ("ae_mode", json!(1)),
        ("iso", json!(100)),
        ("exposure_time_ns", json!(16_666_666)),
        ("frame_duration_ns", json!(33_333_000)),
        ("ae_compensation", json!(0)),
        ("anti_banding", json!(3)),
        ("ae_fps_range", json!([30, 30])),
        ("ae_lock", json!(false)),
        ("af_mode", json!(4)),
        ("focus_distance", json!(2.5)),
        ("af_lock", json!(false)),
        ("awb_mode", json!(1)),
        ("awb_lock", json!(false)),
        ("effect", json!(0)),
        ("aberration_correction", json!(1)),
        ("zoom_ratio", json!(1)),
        ("filter_density", json!(0)),
        ("optical_stabilization", json!(1)),
        ("video_stabilization", json!(0)),
        ("lens_shading", json!(1)),
        ("distortion_correction", json!(1)),
        ("torch", json!(false)),
        ("flash_mode", json!(0)),
        ("scene_mode", json!(0)),
        ("noise_reduction", json!(1)),
        ("edge_enhancement", json!(1)),
        ("hot_pixel", json!(1)),
        ("tonemap", json!(1)),
        ("tonemap_preset_curve", json!(0)),
        ("face_detect", json!(false)),
        ("test_pattern", json!(0)),
        ("camera_id", json!("0")),
        ("physical_camera_id", json!("none")),
        ("stream_size", json!({ "width": 1280, "height": 720 })),
        ("jpeg_quality", json!(80)),
        ("stream_rotation", json!(0)),
        ("still_format", json!("JPEG")),
        ("still_size", json!({ "width": 4000, "height": 3000 })),
        ("video_size", json!({ "width": 1920, "height": 1080 })),
        ("video_bitrate", json!(8_000_000)),
        ("video_codec", json!("h264")),
        ("video_container", json!("mp4")),
        ("audio", json!(false)),
    ]
}

fn readout_defaults() -> Vec<(&'static str, Value)> {
    vec![
        ("ae_state", json!("converged")),
        ("af_state", json!("passive_focused")),
        ("awb_state", json!("converged")),
        ("focus_distance_diopters", json!(2.5)),
        ("awb_gains", json!({ "r": 1.8, "g": 1.0, "b": 1.5 })),
        ("zoom_actual", json!(1)),
        ("lens_focal_length", json!(4.3)),
        ("lens_aperture", json!(2.0)),
        ("lens_filter_density", json!(0)),
        ("lens_state", json!("stationary")),
        ("tonemap_gamma", json!(2.2)),
        ("face_count", json!(0)),
        ("faces", json!([])),
        ("fps_actual", json!(10)),
        ("recording", json!(false)),
        ("battery_percent", json!(87)),
        ("storage_free_mb", json!(24_576)),
    ]
}

fn control_snapshot() -> Value {
    json!({
        "iso": 100,
        "exposure_time_ns": 16_666_666,
        "awb_mode": 1,
        "zoom_actual": 1,
        "fps_actual": 10,
    })
}

fn fake_sha256(seed: &str) -> String {
    // Deterministic placeholder: the mock never transfers real files and the UI
    // only displays the hash.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in seed.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}{hash:016x}{hash:016x}{hash:016x}")
}

fn number(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
