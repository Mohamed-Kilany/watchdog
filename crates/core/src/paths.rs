//! Per-OS data directory layout for watchdog.
//!
//! Everything lives under one root (override with `data_dir` / `WATCHDOG_DATA_DIR` /
//! `--data-dir`) so the whole state can be moved or deleted as a unit:
//!
//! ```text
//! <data_dir>/
//!   config/    watchdog.toml (platform default location maps here)
//!   keys/      adbkey / adbkey.pub  (RSA-2048, never ~/.android)
//!   media/     received media items (see media/<shard>/…)
//!   db/        watchdog.db (SQLite index: media, jobs, reports, devices)
//!   logs/      watchdog-YYYY-MM-DD.log (daily rotation)
//!   cache/     transient data (extracted tools staging, thumbnails)
//!   tools/     extracted bundled tools (ffmpeg per OS/arch, verified)
//!   tmp/       scratch space for in-progress transfers
//! ```
//!
//! The platform **config** file lives in the OS config dir (e.g.
//! `~/.config/watchdog/watchdog.toml`) — not under the data dir — so it
//! survives `data_dir` moves. When `data_dir` is overridden, `keys/`,
//! `media/`, `db/`, `logs/`, `cache/`, `tools/` and `tmp/` all move with it.

use std::path::{Path, PathBuf};

use directories::{BaseDirs, ProjectDirs};

use crate::error::{ErrorCode, Result, WatchdogError};

/// Application directory name used on every OS.
const APP_DIR: &str = "watchdog";
/// Name of the watchdog config file inside the platform config dir.
const CONFIG_FILE: &str = "watchdog.toml";

/// Default data directory for this OS.
///
/// - Linux: `$XDG_DATA_HOME/watchdog` or `~/.local/share/watchdog`
/// - macOS: `~/Library/Application Support/watchdog`
/// - Windows: `%LOCALAPPDATA%\watchdog`
#[must_use]
pub fn default_data_dir() -> PathBuf {
    BaseDirs::new().map_or_else(|| PathBuf::from(APP_DIR), |b| b.data_local_dir().join(APP_DIR))
}

/// Platform config directory (may differ from the data dir).
#[must_use]
pub fn config_dir() -> PathBuf {
    ProjectDirs::from("", "", APP_DIR)
        .map_or_else(|| default_data_dir().join("config"), |p| p.config_dir().to_path_buf())
}

/// Full path of the platform `watchdog.toml`.
#[must_use]
pub fn config_file_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Resolved layout rooted at the effective data dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// The root data directory.
    pub root: PathBuf,
    /// `watchdog.toml` inside the root (only when data_dir is overridden).
    pub config: PathBuf,
    /// ADB key pair directory.
    pub keys: PathBuf,
    /// Media store root.
    pub media: PathBuf,
    /// SQLite index directory.
    pub db: PathBuf,
    /// Log files directory.
    pub logs: PathBuf,
    /// Cache directory.
    pub cache: PathBuf,
    /// Extracted third-party tools (ffmpeg) directory.
    pub tools: PathBuf,
    /// Scratch directory for in-progress transfers.
    pub tmp: PathBuf,
}

impl DataPaths {
    /// Resolve the layout for `root`.
    #[must_use]
    pub fn resolve(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            config: root.join("config"),
            keys: root.join("keys"),
            media: root.join("media"),
            db: root.join("db"),
            logs: root.join("logs"),
            cache: root.join("cache"),
            tools: root.join("tools"),
            tmp: root.join("tmp"),
        }
    }

    /// All directories that must be writable, with human labels.
    #[must_use]
    pub fn writable_targets(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            ("config", self.config.clone()),
            ("keys", self.keys.clone()),
            ("media", self.media.clone()),
            ("db", self.db.clone()),
            ("logs", self.logs.clone()),
            ("cache", self.cache.clone()),
            ("tools", self.tools.clone()),
            ("tmp", self.tmp.clone()),
        ]
    }

    /// Create every directory; returns an error naming the failing path.
    pub fn create_all(&self) -> Result<()> {
        for (label, dir) in self.writable_targets() {
            std::fs::create_dir_all(&dir).map_err(|e| {
                WatchdogError::new(
                    ErrorCode::DataDirNotWritable,
                    format!("cannot create {label} directory {}: {e}", dir.display()),
                )
                .with_hint("check existence/permissions of the data dir, or pass --data-dir <writable path>")
                .with_source(e)
            })?;
        }
        Ok(())
    }
}

/// Write-permission probe: create, write and remove a temp file in `dir`.
///
/// Returns `Ok(())` when writable, otherwise an error describing why.
pub fn check_writable(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| {
        WatchdogError::new(
            ErrorCode::DataDirNotWritable,
            format!("cannot create directory {}: {e}", dir.display()),
        )
        .with_source(e)
    })?;
    let probe = dir.join(format!(".watchdog-write-probe-{}", std::process::id()));
    let result = std::fs::write(&probe, b"watchdog")
        .and_then(|()| std::fs::remove_file(&probe));
    result.map_err(|e| {
        let _ = std::fs::remove_file(&probe);
        WatchdogError::new(
            ErrorCode::DataDirNotWritable,
            format!("directory {} is not writable: {e}", dir.display()),
        )
        .with_hint("fix permissions or point data_dir elsewhere (--data-dir)")
        .with_source(e)
    })
}
