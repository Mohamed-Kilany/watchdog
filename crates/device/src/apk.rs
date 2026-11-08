//! Embedded release APK metadata (M3-T12).
//!
//! Release builds compile the Android APK into the host binary; the metadata
//! is recorded by the build script so provisioning can compare the installed
//! app version and signing certificate before replacing it.

use std::path::{Path, PathBuf};

/// Metadata for the APK compiled into this host build.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedApk {
    /// Complete APK bytes.
    pub bytes: &'static [u8],
    /// `versionName` recorded by the Android build.
    pub version_name: &'static str,
    /// `versionCode` recorded by the Android build.
    pub version_code: u32,
    /// SHA-256 of the APK file, lowercase hex.
    pub sha256: &'static str,
    /// SHA-256 of the signing certificate, lowercase hex (empty if unknown).
    pub signer_sha256: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_apk.rs"));

/// The APK compiled into this binary, or `None` for development builds.
pub fn embedded() -> Option<EmbeddedApk> {
    EMBEDDED
}

/// Why an APK source could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum ApkSourceError {
    /// A local override path does not exist or cannot be read.
    #[error("cannot read local APK {}: {source}", path.display())]
    LocalUnreadable {
        /// The invalid path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// No local override and no embedded APK.
    #[error("no embedded APK in this build; pass --apk <path> or set WATCHDOG_APK when building")]
    NotEmbedded,
}

/// APK bytes plus whatever version metadata is known.
#[derive(Debug)]
pub enum ApkSource {
    /// APK compiled into the host binary.
    Embedded(EmbeddedApk),
    /// Development override loaded from disk.
    Local {
        /// File the APK was read from.
        path: PathBuf,
        /// APK bytes.
        bytes: Vec<u8>,
    },
}

impl ApkSource {
    /// Complete APK bytes.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Embedded(apk) => apk.bytes,
            Self::Local { bytes, .. } => bytes,
        }
    }

    /// Known `versionName` (local overrides are not parsed).
    pub fn version_name(&self) -> Option<&str> {
        match self {
            Self::Embedded(apk) => Some(apk.version_name),
            Self::Local { .. } => None,
        }
    }

    /// Known `versionCode` (local overrides are not parsed).
    pub fn version_code(&self) -> Option<u32> {
        match self {
            Self::Embedded(apk) => Some(apk.version_code),
            Self::Local { .. } => None,
        }
    }
}

/// Resolves the APK to install: `local` wins, otherwise the embedded APK.
pub fn resolve(local: Option<&Path>) -> Result<ApkSource, ApkSourceError> {
    if let Some(path) = local {
        let bytes = std::fs::read(path).map_err(|source| ApkSourceError::LocalUnreadable {
            path: path.to_path_buf(),
            source,
        })?;
        return Ok(ApkSource::Local {
            path: path.to_path_buf(),
            bytes,
        });
    }
    embedded()
        .map(ApkSource::Embedded)
        .ok_or(ApkSourceError::NotEmbedded)
}
