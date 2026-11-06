//! Structured logging: console + rotating file, JSON option, secret redaction.
//!
//! Secrets (bearer token, pairing codes) registered via [`register_secret`]
//! never appear in any log sink: every byte written to the console or the
//! log file passes through a streaming redactor that replaces any occurrence
//! — even one split across write calls — with `[REDACTED]`.
//!
//! Additionally, secrets should be wrapped in [`Secret`] rather than `String`
//! so accidental `{:?}`/`{}` formatting prints `[REDACTED]`.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock, RwLock};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::error::{Result, WatchdogError};
use crate::paths;

/// Redaction marker emitted in place of a secret.
pub const REDACTED: &str = "[REDACTED]";

static SECRETS: OnceLock<RwLock<Vec<Vec<u8>>>> = OnceLock::new();

fn secrets() -> &'static RwLock<Vec<Vec<u8>>> {
    SECRETS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Register a secret value to be redacted from all log output.
///
/// Call this once at startup for the bearer token and whenever a pairing
/// code is received. Values shorter than 4 bytes are rejected to avoid
/// mangling ordinary output.
pub fn register_secret(value: &str) -> bool {
    let bytes = value.as_bytes().to_vec();
    if bytes.len() < 4 {
        return false;
    }
    let mut guard = secrets().write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !guard.contains(&bytes) {
        guard.push(bytes);
        // Longest first so overlapping matches replace the maximal secret.
        guard.sort_by_key(|b| std::cmp::Reverse(b.len()));
    }
    true
}

/// A secret that prints as `[REDACTED]` in both `Display` and `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the raw secret. Never log the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// `Write` wrapper that redacts registered secrets, tolerating splits across
/// `write` calls. Complete lines plus a trailing partial line are both safe.
struct StreamingRedactor<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: Write> StreamingRedactor<W> {
    fn new(inner: W) -> Self {
        Self { inner, pending: Vec::new() }
    }

    fn emit_ready(&mut self, final_flush: bool) -> std::io::Result<()> {
        let secrets = secrets().read().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let max_keep = secrets.iter().map(Vec::len).max().map_or(0, |m| m.saturating_sub(1));

        // Redact everything we can fully see.
        loop {
            let mut found: Option<(usize, usize)> = None; // (index, secret_len)
            for secret in &secrets {
                if secret.len() > self.pending.len() {
                    continue;
                }
                if let Some(idx) = find_subslice(&self.pending, secret) {
                    match found {
                        Some((cur, _)) if cur <= idx => {}
                        _ => found = Some((idx, secret.len())),
                    }
                }
            }
            if let Some((idx, secret_len)) = found {
                self.inner.write_all(&self.pending[..idx])?;
                self.inner.write_all(REDACTED.as_bytes())?;
                self.pending.drain(..idx + secret_len);
            } else {
                break;
            }
        }

        let keep = if final_flush { 0 } else { max_keep };
        let emit_len = self.pending.len().saturating_sub(keep);
        if emit_len > 0 {
            let ready: Vec<u8> = self.pending.drain(..emit_len).collect();
            self.inner.write_all(&ready)?;
        }
        Ok(())
    }
}

impl<W: Write> Write for StreamingRedactor<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(buf);
        self.emit_ready(false)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.emit_ready(true)?;
        self.inner.flush()
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Shared writer handle returned by [`SharedRedactingWriter`].
pub struct SharedRedactingWriter<W: Write + Send + 'static> {
    shared: std::sync::Arc<Mutex<StreamingRedactor<W>>>,
}

impl<W: Write + Send + 'static> Clone for SharedRedactingWriter<W> {
    fn clone(&self) -> Self {
        Self { shared: std::sync::Arc::clone(&self.shared) }
    }
}

impl<W: Write + Send + 'static> Write for SharedRedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.shared.lock().map_err(|_| std::io::Error::other("log writer poisoned"))?.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.shared.lock().map_err(|_| std::io::Error::other("log writer poisoned"))?.flush()
    }
}

/// `MakeWriter` that returns handles sharing a single redacting writer.
pub struct SharedRedactingWriterFactory<W: Write + Send + 'static> {
    shared: std::sync::Arc<Mutex<StreamingRedactor<W>>>,
}

impl<W: Write + Send + 'static> SharedRedactingWriterFactory<W> {
    fn new(inner: W) -> Self {
        Self { shared: std::sync::Arc::new(Mutex::new(StreamingRedactor::new(inner))) }
    }
}

impl<W: Write + Send + 'static> MakeWriter<'_> for SharedRedactingWriterFactory<W> {
    type Writer = SharedRedactingWriter<W>;

    fn make_writer(&self) -> Self::Writer {
        SharedRedactingWriter { shared: std::sync::Arc::clone(&self.shared) }
    }
}

/// Guards returned by [`init`]; keep alive for the process lifetime.
pub struct LogGuard {
    _file_guard: Option<WorkerGuard>,
}

/// Initialise the global subscriber.
///
/// Logs are written to stderr (console format, or JSON with `log_json`) and,
/// when `log_dir` is provided, to `<log_dir>/watchdog.YYYY-MM-DD` with daily
/// rotation. `RUST_LOG` overrides `level` when set.
pub fn init(level: &str, log_json: bool, log_dir: Option<&Path>) -> Result<LogGuard> {
    if let Some(dir) = log_dir {
        paths::check_writable(dir)?;
    }

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let registry = Registry::default().with(filter);

    let console_factory = SharedRedactingWriterFactory::new(std::io::stderr());
    let json = log_json;

    match log_dir {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(dir, "watchdog");
            let (non_blocking, guard) = tracing_appender::non_blocking(appender);
            let file_factory = SharedRedactingWriterFactory::new(non_blocking);
            if json {
                registry
                    .with(tracing_subscriber::fmt::layer().json().with_writer(console_factory))
                    .with(tracing_subscriber::fmt::layer().json().with_writer(file_factory))
                    .try_init()
                    .map_err(|e| WatchdogError::new(crate::error::ErrorCode::Internal, format!("failed to init logging: {e}")))?;
            } else {
                registry
                    .with(tracing_subscriber::fmt::layer().with_writer(console_factory))
                    .with(tracing_subscriber::fmt::layer().with_writer(file_factory))
                    .try_init()
                    .map_err(|e| WatchdogError::new(crate::error::ErrorCode::Internal, format!("failed to init logging: {e}")))?;
            }
            Ok(LogGuard { _file_guard: Some(guard) })
        }
        None => {
            if json {
                registry
                    .with(tracing_subscriber::fmt::layer().json().with_writer(console_factory))
                    .try_init()
                    .map_err(|e| WatchdogError::new(crate::error::ErrorCode::Internal, format!("failed to init logging: {e}")))?;
            } else {
                registry
                    .with(tracing_subscriber::fmt::layer().with_writer(console_factory))
                    .try_init()
                    .map_err(|e| WatchdogError::new(crate::error::ErrorCode::Internal, format!("failed to init logging: {e}")))?;
            }
            Ok(LogGuard { _file_guard: None })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_written_in_one_write() {
        register_secret("hunter2-token-xyz");
        let shared = std::sync::Arc::new(Mutex::new(StreamingRedactor::new(Vec::new())));
        let mut w = SharedRedactingWriter { shared: std::sync::Arc::clone(&shared) };
        w.write_all(b"connecting with hunter2-token-xyz now").unwrap();
        w.flush().unwrap();
        let out = shared.lock().unwrap().inner.clone();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("hunter2-token-xyz"), "secret leaked: {text}");
        assert!(text.contains(REDACTED));
    }

    #[test]
    fn redacts_secret_split_across_writes() {
        register_secret("split-secret-98765");
        let shared = std::sync::Arc::new(Mutex::new(StreamingRedactor::new(Vec::new())));
        let mut w = SharedRedactingWriter { shared: std::sync::Arc::clone(&shared) };
        for piece in [&b"tok=split-"[..], b"secret-98", b"765&x=1\n"] {
            w.write_all(piece).unwrap();
        }
        w.flush().unwrap();
        let out = shared.lock().unwrap().inner.clone();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("split-secret-98765"), "secret leaked: {text}");
        assert_eq!(text, "tok=[REDACTED]&x=1\n");
    }

    #[test]
    fn flush_emits_everything() {
        register_secret("tail-secret-abc");
        let shared = std::sync::Arc::new(Mutex::new(StreamingRedactor::new(Vec::new())));
        let mut w = SharedRedactingWriter { shared: std::sync::Arc::clone(&shared) };
        w.write_all(b"prefix tail-secret-abc").unwrap();
        w.flush().unwrap();
        let out = shared.lock().unwrap().inner.clone();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, format!("prefix {REDACTED}"));
    }

    #[test]
    fn short_secrets_are_rejected() {
        assert!(!register_secret("abc"));
    }

    #[test]
    fn secret_display_is_redacted() {
        let s = Secret::new("super-secret-value");
        assert_eq!(s.to_string(), REDACTED);
        assert_eq!(format!("{s:?}"), REDACTED);
        assert_eq!(s.expose(), "super-secret-value");
    }

    #[test]
    fn end_to_end_formatting_is_redacted() {
        register_secret("e2e-token-11223344");
        let shared = std::sync::Arc::new(Mutex::new(StreamingRedactor::new(Vec::new())));
        let factory_shared = std::sync::Arc::clone(&shared);
        // Format an event through tracing into the redacting writer.
        let make = SharedRedactingWriterFactory { shared: factory_shared };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(make)
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("dialing device with token e2e-token-11223344");
        });
        shared.lock().unwrap().flush().unwrap();
        let out = shared.lock().unwrap().inner.clone();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("e2e-token-11223344"), "secret leaked in log line: {text}");
        assert!(text.contains(REDACTED));
    }

}
