//! Layered configuration: defaults < `watchdog.toml` < `WATCHDOG_*` env < CLI flags.
//!
//! File location resolution:
//! 1. `--config <path>` / `WATCHDOG_CONFIG` (explicit; must exist),
//! 2. the platform config file (`<config_dir>/watchdog.toml`, e.g.
//!    `~/.config/watchdog/watchdog.toml` on Linux),
//! 3. built-in defaults.
//!
//! A missing platform config file is fine; a missing *explicit* one is
//! [`ErrorCode::ConfigNotFound`]. Invalid values abort with
//! [`ErrorCode::ConfigInvalid`] naming the offending key.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, Result, WatchdogError};
use crate::paths;

/// Default web UI port (serves UI + UI API/WebSocket).
pub const DEFAULT_UI_PORT: u16 = 7700;
/// Default MCP (Streamable HTTP) port.
pub const DEFAULT_MCP_PORT: u16 = 7701;
/// Default integration API port.
pub const DEFAULT_API_PORT: u16 = 7702;
/// Default bind address: loopback only.
pub const DEFAULT_LISTEN: &str = "127.0.0.1";
/// Default maximum age of retained media, in days. `0` keeps media forever.
pub const DEFAULT_RETENTION_MAX_AGE_DAYS: u32 = 30;
/// Default maximum size of retained media, in MiB. `0` means unlimited.
pub const DEFAULT_RETENTION_MAX_SIZE_MB: u64 = 10 * 1024;
/// Environment override for the config file path.
pub const CONFIG_PATH_ENV: &str = "WATCHDOG_CONFIG";

/// Which layer a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    /// Built-in default.
    Default,
    /// `watchdog.toml`.
    File,
    /// `WATCHDOG_*` environment variable.
    Env,
    /// CLI flag.
    Cli,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::File => write!(f, "file"),
            Self::Env => write!(f, "env"),
            Self::Cli => write!(f, "cli"),
        }
    }
}

/// CLI overrides; every field is optional and wins over file and env.
#[derive(Debug, Default, Clone)]
pub struct ConfigOverrides {
    /// Explicit config file path (`--config`).
    pub config_file: Option<PathBuf>,
    /// `--ui-port`
    pub ui_port: Option<u16>,
    /// `--mcp-port`
    pub mcp_port: Option<u16>,
    /// `--api-port`
    pub api_port: Option<u16>,
    /// `--ui-listen`
    pub ui_listen: Option<String>,
    /// `--mcp-listen`
    pub mcp_listen: Option<String>,
    /// `--api-listen`
    pub api_listen: Option<String>,
    /// `--token`
    pub token: Option<String>,
    /// `--data-dir`
    pub data_dir: Option<PathBuf>,
    /// `--log-level`
    pub log_level: Option<String>,
}

/// Resolved, validated configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Web UI port.
    pub ui_port: u16,
    /// MCP (Streamable HTTP) port.
    pub mcp_port: u16,
    /// Integration API port.
    pub api_port: u16,
    /// Web UI bind address.
    pub ui_listen: IpAddr,
    /// MCP bind address.
    pub mcp_listen: IpAddr,
    /// Integration API bind address.
    pub api_listen: IpAddr,
    /// Bearer token required on all ports when set (required for LAN).
    pub token: Option<String>,
    /// Root data directory (media, db, logs, keys, tools, cache).
    pub data_dir: PathBuf,
    /// Retention: delete media older than this many days. `0` = keep forever.
    pub retention_max_age_days: u32,
    /// Retention: delete oldest media beyond this many MiB. `0` = unlimited.
    pub retention_max_size_mb: u64,
    /// Debug-only ffmpeg override; ignored in release builds.
    pub ffmpeg_path: Option<PathBuf>,
    /// Log level filter: trace|debug|info|warn|error.
    pub log_level: String,
    /// Emit logs as JSON lines instead of the console format.
    pub log_json: bool,
    /// Where each key came from (for `watchdog config show`).
    sources: HashMap<String, ConfigSource>,
    /// The file the configuration was loaded from, if any.
    pub config_file: Option<PathBuf>,
}

impl Config {
    /// Load and validate the effective configuration.
    pub fn load(overrides: &ConfigOverrides) -> Result<Self> {
        let mut sources: HashMap<String, ConfigSource> = HashMap::new();
        let mut set = |key: &str, source: ConfigSource| sources.insert(key.to_string(), source);

        // --- resolve config file ---
        let explicit_file = overrides
            .config_file
            .clone()
            .or_else(|| std::env::var_os(CONFIG_PATH_ENV).map(PathBuf::from));
        let config_file = match &explicit_file {
            Some(path) => {
                if !path.exists() {
                    return Err(WatchdogError::new(
                        ErrorCode::ConfigNotFound,
                        format!("config file not found: {}", path.display()),
                    )
                    .with_hint(format!(
                        "create the file or drop {CONFIG_PATH_ENV}/--config (expected TOML, see docs/user-guide.md#configuration)"
                    )));
                }
                Some(path.clone())
            }
            None => {
                let candidate = paths::config_file_path();
                candidate.exists().then_some(candidate)
            }
        };

        // --- layer 1: defaults ---
        let mut values = FileValues::default();
        for key in FileValues::KEYS {
            set(key, ConfigSource::Default);
        }

        // --- layer 2: file ---
        if let Some(path) = &config_file {
            let text = std::fs::read_to_string(path).map_err(|e| {
                WatchdogError::new(
                    ErrorCode::ConfigInvalid,
                    format!("cannot read config file {}: {e}", path.display()),
                )
                .with_source(e)
            })?;
            let parsed: FileValuesFile = toml::from_str(&text).map_err(|e| {
                WatchdogError::new(
                    ErrorCode::ConfigInvalid,
                    format!("invalid TOML in {}: {e}", path.display()),
                )
                .with_hint("run `watchdog config validate` after fixing the file")
            })?;
            merge(&mut values, parsed.into(), ConfigSource::File, &mut set);
        }

        // --- layer 3: environment ---
        merge(
            &mut values,
            FileValues::from_env()?,
            ConfigSource::Env,
            &mut set,
        );

        // --- layer 4: CLI ---
        merge(
            &mut values,
            FileValues::from_overrides(overrides),
            ConfigSource::Cli,
            &mut set,
        );

        // --- validate ---
        let cfg = values.into_config(sources, config_file)?;
        Ok(cfg)
    }

    /// Port of the named service, for wiring and diagnostics.
    #[must_use]
    pub fn is_lan_exposed(&self) -> bool {
        [self.ui_listen, self.mcp_listen, self.api_listen]
            .iter()
            .any(|a| !a.is_loopback())
    }

    /// Where a key came from.
    #[must_use]
    pub fn source_of(&self, key: &str) -> ConfigSource {
        self.sources
            .get(key)
            .copied()
            .unwrap_or(ConfigSource::Default)
    }

    /// All (key, value, source) rows with secrets redacted, for `config show`.
    #[must_use]
    pub fn redacted_rows(&self) -> Vec<(String, String, ConfigSource)> {
        let rows = [
            ("ui_port", self.ui_port.to_string()),
            ("mcp_port", self.mcp_port.to_string()),
            ("api_port", self.api_port.to_string()),
            ("ui_listen", self.ui_listen.to_string()),
            ("mcp_listen", self.mcp_listen.to_string()),
            ("api_listen", self.api_listen.to_string()),
            (
                "token",
                if self.token.is_some() {
                    "set (redacted)".to_string()
                } else {
                    "not set".to_string()
                },
            ),
            ("data_dir", self.data_dir.display().to_string()),
            (
                "retention_max_age_days",
                self.retention_max_age_days.to_string(),
            ),
            (
                "retention_max_size_mb",
                self.retention_max_size_mb.to_string(),
            ),
            (
                "ffmpeg_path",
                self.ffmpeg_path
                    .as_ref()
                    .map_or_else(|| "not set".to_string(), |p| p.display().to_string()),
            ),
            ("log_level", self.log_level.clone()),
            ("log_json", self.log_json.to_string()),
        ];
        rows.into_iter()
            .map(|(k, v)| (k.to_string(), v, self.source_of(k)))
            .collect()
    }
}

/// Merge `newer` into `base`, stamping overridden keys with `source`.
fn merge(
    base: &mut FileValues,
    newer: FileValues,
    source: ConfigSource,
    set: &mut dyn FnMut(&str, ConfigSource) -> Option<ConfigSource>,
) {
    macro_rules! take {
        ($field:ident, $key:literal) => {
            if let Some(value) = newer.$field {
                base.$field = Some(value);
                set($key, source);
            }
        };
    }
    take!(ui_port, "ui_port");
    take!(mcp_port, "mcp_port");
    take!(api_port, "api_port");
    take!(ui_listen, "ui_listen");
    take!(mcp_listen, "mcp_listen");
    take!(api_listen, "api_listen");
    take!(token, "token");
    take!(data_dir, "data_dir");
    take!(retention_max_age_days, "retention_max_age_days");
    take!(retention_max_size_mb, "retention_max_size_mb");
    take!(ffmpeg_path, "ffmpeg_path");
    take!(log_level, "log_level");
    take!(log_json, "log_json");
}

/// Intermediate, all-optional values.
#[derive(Debug, Default, Clone)]
struct FileValues {
    ui_port: Option<u16>,
    mcp_port: Option<u16>,
    api_port: Option<u16>,
    ui_listen: Option<String>,
    mcp_listen: Option<String>,
    api_listen: Option<String>,
    token: Option<String>,
    data_dir: Option<PathBuf>,
    retention_max_age_days: Option<u32>,
    retention_max_size_mb: Option<u64>,
    ffmpeg_path: Option<PathBuf>,
    log_level: Option<String>,
    log_json: Option<bool>,
}

impl FileValues {
    const KEYS: &'static [&'static str] = &[
        "ui_port",
        "mcp_port",
        "api_port",
        "ui_listen",
        "mcp_listen",
        "api_listen",
        "token",
        "data_dir",
        "retention_max_age_days",
        "retention_max_size_mb",
        "ffmpeg_path",
        "log_level",
        "log_json",
    ];

    fn from_env() -> Result<Self> {
        fn opt(key: &str) -> Option<String> {
            std::env::var(key).ok().filter(|v| !v.is_empty())
        }
        fn parse<T: std::str::FromStr>(key: &str) -> Result<Option<T>> {
            match opt(key) {
                None => Ok(None),
                Some(raw) => raw.parse::<T>().map(Some).map_err(|_| {
                    WatchdogError::new(
                        ErrorCode::ConfigInvalid,
                        format!("environment variable {key} has an invalid value: {raw:?}"),
                    )
                }),
            }
        }

        Ok(Self {
            ui_port: parse("WATCHDOG_UI_PORT")?,
            mcp_port: parse("WATCHDOG_MCP_PORT")?,
            api_port: parse("WATCHDOG_API_PORT")?,
            ui_listen: opt("WATCHDOG_UI_LISTEN"),
            mcp_listen: opt("WATCHDOG_MCP_LISTEN"),
            api_listen: opt("WATCHDOG_API_LISTEN"),
            token: opt("WATCHDOG_TOKEN"),
            data_dir: opt("WATCHDOG_DATA_DIR").map(PathBuf::from),
            retention_max_age_days: parse("WATCHDOG_RETENTION_MAX_AGE_DAYS")?,
            retention_max_size_mb: parse("WATCHDOG_RETENTION_MAX_SIZE_MB")?,
            ffmpeg_path: opt("WATCHDOG_FFMPEG_PATH").map(PathBuf::from),
            log_level: opt("WATCHDOG_LOG_LEVEL"),
            log_json: parse("WATCHDOG_LOG_JSON")?,
        })
    }

    fn from_overrides(o: &ConfigOverrides) -> Self {
        Self {
            ui_port: o.ui_port,
            mcp_port: o.mcp_port,
            api_port: o.api_port,
            ui_listen: o.ui_listen.clone(),
            mcp_listen: o.mcp_listen.clone(),
            api_listen: o.api_listen.clone(),
            token: o.token.clone(),
            data_dir: o.data_dir.clone(),
            retention_max_age_days: None,
            retention_max_size_mb: None,
            ffmpeg_path: None,
            log_level: o.log_level.clone(),
            log_json: None,
        }
    }

    fn into_config(
        self,
        sources: HashMap<String, ConfigSource>,
        config_file: Option<PathBuf>,
    ) -> Result<Config> {
        let parse_listen = |key: &str, raw: String| -> Result<IpAddr> {
            raw.parse::<IpAddr>().map_err(|_| {
                WatchdogError::new(
                    ErrorCode::ConfigInvalid,
                    format!("{key} must be an IP address (e.g. 127.0.0.1 or 0.0.0.0), got {raw:?}"),
                )
                .with_hint(
                    "use 0.0.0.0 to expose on the LAN (requires a token, see docs/security.md)",
                )
            })
        };
        let ui_listen = parse_listen(
            "ui_listen",
            self.ui_listen.unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
        )?;
        let mcp_listen = parse_listen(
            "mcp_listen",
            self.mcp_listen
                .unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
        )?;
        let api_listen = parse_listen(
            "api_listen",
            self.api_listen
                .unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
        )?;

        let ui_port = self.ui_port.unwrap_or(DEFAULT_UI_PORT);
        let mcp_port = self.mcp_port.unwrap_or(DEFAULT_MCP_PORT);
        let api_port = self.api_port.unwrap_or(DEFAULT_API_PORT);
        for (key, port) in [
            ("ui_port", ui_port),
            ("mcp_port", mcp_port),
            ("api_port", api_port),
        ] {
            if port == 0 {
                return Err(WatchdogError::new(
                    ErrorCode::ConfigInvalid,
                    format!("{key} must be between 1 and 65535, got 0"),
                ));
            }
        }
        if ui_port == mcp_port || ui_port == api_port || mcp_port == api_port {
            return Err(WatchdogError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "ports must be distinct (ui={ui_port}, mcp={mcp_port}, api={api_port}); \
                     set ui_port/mcp_port/api_port in watchdog.toml or via WATCHDOG_*_PORT / --*-port flags"
                ),
            ));
        }

        let log_level = self
            .log_level
            .unwrap_or_else(|| "info".to_string())
            .to_lowercase();
        const LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
        if !LEVELS.contains(&log_level.as_str()) {
            return Err(WatchdogError::new(
                ErrorCode::ConfigInvalid,
                format!("log_level must be one of {LEVELS:?}, got {log_level:?}"),
            ));
        }

        let ffmpeg_path = self.ffmpeg_path.and_then(|p| {
            if cfg!(debug_assertions) {
                Some(p)
            } else {
                tracing::warn!(
                    "ffmpeg_path override ({}) is ignored in release builds; the bundled ffmpeg is used",
                    p.display()
                );
                None
            }
        });

        Ok(Config {
            ui_port,
            mcp_port,
            api_port,
            ui_listen,
            mcp_listen,
            api_listen,
            token: self.token.filter(|t| !t.trim().is_empty()),
            data_dir: self.data_dir.unwrap_or_else(paths::default_data_dir),
            retention_max_age_days: self
                .retention_max_age_days
                .unwrap_or(DEFAULT_RETENTION_MAX_AGE_DAYS),
            retention_max_size_mb: self
                .retention_max_size_mb
                .unwrap_or(DEFAULT_RETENTION_MAX_SIZE_MB),
            ffmpeg_path,
            log_level,
            log_json: self.log_json.unwrap_or(false),
            sources,
            config_file,
        })
    }
}

/// Serde shape of `watchdog.toml` (all keys optional).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileValuesFile {
    ui_port: Option<u16>,
    mcp_port: Option<u16>,
    api_port: Option<u16>,
    ui_listen: Option<String>,
    mcp_listen: Option<String>,
    api_listen: Option<String>,
    token: Option<String>,
    data_dir: Option<PathBuf>,
    retention_max_age_days: Option<u32>,
    retention_max_size_mb: Option<u64>,
    ffmpeg_path: Option<PathBuf>,
    log_level: Option<String>,
    log_json: Option<bool>,
}

impl From<FileValuesFile> for FileValues {
    fn from(f: FileValuesFile) -> Self {
        Self {
            ui_port: f.ui_port,
            mcp_port: f.mcp_port,
            api_port: f.api_port,
            ui_listen: f.ui_listen,
            mcp_listen: f.mcp_listen,
            api_listen: f.api_listen,
            token: f.token,
            data_dir: f.data_dir,
            retention_max_age_days: f.retention_max_age_days,
            retention_max_size_mb: f.retention_max_size_mb,
            ffmpeg_path: f.ffmpeg_path,
            log_level: f.log_level,
            log_json: f.log_json,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::{Mutex, OnceLock};

    use super::*;

    /// Env vars are process-global; serialize the env-mutating tests.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        saved: Vec<(String, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { saved: Vec::new() }
        }

        fn set(&mut self, key: &str, value: &str) {
            self.saved.push((key.to_string(), std::env::var_os(key)));
            std::env::set_var(key, value);
        }

        fn unset(&mut self, key: &str) {
            self.saved.push((key.to_string(), std::env::var_os(key)));
            std::env::remove_var(key);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                match value {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }

    const ALL_ENV_KEYS: &[&str] = &[
        "WATCHDOG_CONFIG",
        "WATCHDOG_UI_PORT",
        "WATCHDOG_MCP_PORT",
        "WATCHDOG_API_PORT",
        "WATCHDOG_UI_LISTEN",
        "WATCHDOG_MCP_LISTEN",
        "WATCHDOG_API_LISTEN",
        "WATCHDOG_TOKEN",
        "WATCHDOG_DATA_DIR",
        "WATCHDOG_RETENTION_MAX_AGE_DAYS",
        "WATCHDOG_RETENTION_MAX_SIZE_MB",
        "WATCHDOG_FFMPEG_PATH",
        "WATCHDOG_LOG_LEVEL",
        "WATCHDOG_LOG_JSON",
    ];

    fn clean_env(guard: &mut EnvGuard) {
        for key in ALL_ENV_KEYS {
            guard.unset(key);
        }
    }

    fn write_config(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("watchdog.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        path
    }

    #[test]
    fn precedence_defaults_file_env_cli() {
        let _lock = env_lock().lock().unwrap();
        let mut guard = EnvGuard::new();
        clean_env(&mut guard);
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(dir.path(), "ui_port = 7710\nlog_level = \"debug\"\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());

        // File beats default.
        let cfg = Config::load(&ConfigOverrides::default()).unwrap();
        assert_eq!(cfg.ui_port, 7710);
        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.source_of("ui_port"), ConfigSource::File);
        assert_eq!(cfg.source_of("mcp_port"), ConfigSource::Default);

        // Env beats file.
        guard.set("WATCHDOG_UI_PORT", "7720");
        let cfg = Config::load(&ConfigOverrides::default()).unwrap();
        assert_eq!(cfg.ui_port, 7720);
        assert_eq!(cfg.source_of("ui_port"), ConfigSource::Env);

        // CLI beats env.
        let cfg = Config::load(&ConfigOverrides {
            ui_port: Some(7730),
            ..ConfigOverrides::default()
        })
        .unwrap();
        assert_eq!(cfg.ui_port, 7730);
        assert_eq!(cfg.source_of("ui_port"), ConfigSource::Cli);
    }

    #[test]
    fn missing_explicit_config_is_not_found() {
        let _lock = env_lock().lock().unwrap();
        let mut guard = EnvGuard::new();
        clean_env(&mut guard);
        guard.set("WATCHDOG_CONFIG", "C:\\no\\such\\watchdog.toml");
        let err = Config::load(&ConfigOverrides::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigNotFound);
    }

    #[test]
    fn invalid_values_are_rejected() {
        let _lock = env_lock().lock().unwrap();
        let mut guard = EnvGuard::new();
        clean_env(&mut guard);
        let dir = tempfile::tempdir().unwrap();

        // Bad TOML.
        let file = write_config(dir.path(), "ui_port = [1,2,3]\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        assert_eq!(
            Config::load(&ConfigOverrides::default()).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );

        // Unknown key (deny_unknown_fields).
        let file = write_config(dir.path(), "ui_prot = 7710\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        assert_eq!(
            Config::load(&ConfigOverrides::default()).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );

        // Distinct ports required.
        let file = write_config(dir.path(), "ui_port = 7700\nmcp_port = 7700\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        let err = Config::load(&ConfigOverrides::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert!(err.message.contains("distinct"));

        // Bad listen address.
        let file = write_config(dir.path(), "ui_listen = \"not-an-ip\"\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        assert_eq!(
            Config::load(&ConfigOverrides::default()).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );

        // Bad log level.
        let file = write_config(dir.path(), "log_level = \"shouty\"\n");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        assert_eq!(
            Config::load(&ConfigOverrides::default()).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );

        // Env with unparsable integer.
        let file = write_config(dir.path(), "");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        guard.set("WATCHDOG_UI_PORT", "seventy-seven");
        assert_eq!(
            Config::load(&ConfigOverrides::default()).unwrap_err().code,
            ErrorCode::ConfigInvalid
        );
    }

    #[test]
    fn lan_exposure_and_token_reporting() {
        let _lock = env_lock().lock().unwrap();
        let mut guard = EnvGuard::new();
        clean_env(&mut guard);
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(
            dir.path(),
            "api_listen = \"0.0.0.0\"\ntoken = \"local-test-token-123\"\n",
        );
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        let cfg = Config::load(&ConfigOverrides::default()).unwrap();
        assert!(cfg.is_lan_exposed());
        assert_eq!(cfg.token.as_deref(), Some("local-test-token-123"));
        let rendered = format!("{:?}", cfg.redacted_rows());
        assert!(
            !rendered.contains("local-test-token-123"),
            "token leaked into redacted rows: {rendered}"
        );

        let file = write_config(dir.path(), "");
        guard.set("WATCHDOG_CONFIG", file.to_str().unwrap());
        let cfg = Config::load(&ConfigOverrides::default()).unwrap();
        assert!(!cfg.is_lan_exposed());
    }

    #[test]
    fn data_dir_env_moves_layout() {
        let _lock = env_lock().lock().unwrap();
        let mut guard = EnvGuard::new();
        clean_env(&mut guard);
        guard.set("WATCHDOG_DATA_DIR", "D:\\\\watchdog-test-root");
        let cfg = Config::load(&ConfigOverrides::default()).unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("D:\\\\watchdog-test-root"));
        assert_eq!(cfg.source_of("data_dir"), ConfigSource::Env);
    }
}
