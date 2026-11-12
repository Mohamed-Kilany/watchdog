//! Named camera-control presets stored on the host (M4-T11).
//!
//! A preset is a small JSON document: a schema version, a display name, an optional
//! description and a map of control id to value. Presets live as one file per preset under
//! `<data_dir>/presets/<slug>.json`, are applied as a single `set_controls` batch and can be
//! exported/imported as plain JSON.
//!
//! The schema is documented in `docs/presets.md`. Values are intentionally untyped JSON:
//! the device's capability descriptor remains the authority for what a control accepts.

#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use watchdog_core::error::{ErrorCode, Result, WatchdogError};

/// Current preset schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Longest accepted preset name.
pub const MAX_NAME_LEN: usize = 64;
/// Longest accepted description.
pub const MAX_DESCRIPTION_LEN: usize = 512;
/// Largest accepted number of controls in one preset.
pub const MAX_CONTROLS: usize = 128;
/// Longest accepted control id.
pub const MAX_CONTROL_ID_LEN: usize = 64;
/// Largest accepted serialized value for one control.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// Read-only `STATE` entries that must never be stored in a preset.
pub const READ_ONLY_CONTROLS: &[&str] = &[
    "fps_actual",
    "recording",
    "battery_percent",
    "ae_state",
    "af_state",
    "awb_state",
    "focus_distance_diopters",
    "awb_gains",
    "color_transform",
    "zoom_actual",
    "lens_focal_length",
    "lens_aperture",
    "lens_filter_density",
    "lens_state",
    "face_count",
    "faces",
];

/// One-shot trigger controls that are never stored in a preset.
pub const ONE_SHOT_CONTROLS: &[&str] = &["ae_precapture", "af_trigger"];

/// Controls that rebuild the capture session or pipeline.
pub const STRUCTURAL_CONTROLS: &[&str] = &[
    "camera_id",
    "physical_camera_id",
    "stream_size",
    "jpeg_quality",
    "stream_rotation",
    "still_format",
    "still_size",
    "video_size",
    "video_bitrate",
    "video_codec",
    "video_container",
    "audio",
];

/// One control assignment inside a preset or an apply batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlAssignment {
    /// Control id, e.g. `iso` or `zoom_ratio`.
    pub id: String,
    /// JSON value accepted by the control.
    pub value: Value,
}

/// A named, reusable camera-control preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// Preset schema version; currently [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Human-readable name (ASCII letters, digits, spaces, `-`, `_` and `.`).
    pub name: String,
    /// Optional description shown by `preset list`/`preset show`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Control id to value map. Sorted by id in the JSON file.
    pub controls: BTreeMap<String, Value>,
    /// Creation timestamp (RFC 3339, UTC), set by the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Last-save timestamp (RFC 3339, UTC), set by the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Preset {
    /// Build and validate a new preset.
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        controls: BTreeMap<String, Value>,
    ) -> Result<Self> {
        let preset = Self {
            schema_version: SCHEMA_VERSION,
            name: name.into(),
            description,
            controls,
            created_at: None,
            updated_at: None,
        };
        preset.validate()?;
        Ok(preset)
    }

    /// Validate the schema, name, description, control ids and values.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid(format!(
                "preset schema_version {} is not supported (expected {SCHEMA_VERSION})",
                self.schema_version
            )));
        }
        validate_name(&self.name)?;
        if let Some(description) = &self.description {
            if description.len() > MAX_DESCRIPTION_LEN {
                return Err(invalid(format!(
                    "preset description is longer than {MAX_DESCRIPTION_LEN} bytes"
                )));
            }
        }
        if self.controls.is_empty() {
            return Err(invalid("a preset must contain at least one control"));
        }
        if self.controls.len() > MAX_CONTROLS {
            return Err(invalid(format!(
                "a preset may contain at most {MAX_CONTROLS} controls"
            )));
        }
        for (id, value) in &self.controls {
            validate_control_id(id)?;
            if value.is_null() {
                return Err(invalid(format!(
                    "control '{id}' has a null value; omit it instead"
                )));
            }
            let bytes = serde_json::to_vec(value)?;
            if bytes.len() > MAX_VALUE_BYTES {
                return Err(invalid(format!(
                    "control '{id}' value is larger than {MAX_VALUE_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }

    /// Control assignments in apply order: `camera_id`, `physical_camera_id`, then the
    /// remaining ids alphabetically. Camera selection must precede dependent controls.
    #[must_use]
    pub fn assignments(&self) -> Vec<ControlAssignment> {
        let mut ids = self.controls.keys().collect::<Vec<_>>();
        ids.sort_by_key(|id| match id.as_str() {
            "camera_id" => 0,
            "physical_camera_id" => 1,
            _ => 2,
        });
        ids.into_iter()
            .map(|id| ControlAssignment {
                id: id.clone(),
                value: self.controls[id].clone(),
            })
            .collect()
    }
}

/// Summary of one stored preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetSummary {
    /// Display name.
    pub name: String,
    /// File name inside the preset directory.
    pub file: String,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Number of controls in the preset.
    pub controls: usize,
    /// Last-save timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// One-directory store of JSON presets.
#[derive(Debug, Clone)]
pub struct PresetStore {
    root: PathBuf,
}

impl PresetStore {
    /// Create a store rooted at `root` (normally `<data_dir>/presets`).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Directory holding the preset files.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create the preset directory if needed.
    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root).map_err(|error| {
            WatchdogError::new(
                ErrorCode::DataDirNotWritable,
                format!(
                    "cannot create preset directory {}: {error}",
                    self.root.display()
                ),
            )
            .with_hint("check the data directory permissions or pass --data-dir")
            .with_source(error)
        })
    }

    /// List every valid preset, sorted case-insensitively by name.
    pub fn list(&self) -> Result<Vec<PresetSummary>> {
        self.ensure_dir()?;
        let mut summaries = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let preset = read_preset(&path)?;
            summaries.push(PresetSummary {
                name: preset.name.clone(),
                file: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string(),
                description: preset.description.clone(),
                controls: preset.controls.len(),
                updated_at: preset.updated_at.clone(),
            });
        }
        summaries.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.file.cmp(&right.file))
        });
        Ok(summaries)
    }

    /// Load a preset by display name or slug (case-insensitive).
    pub fn load(&self, query: &str) -> Result<Preset> {
        read_preset(&self.resolve(query)?)
    }

    /// Resolve a display name or slug to its file path.
    pub fn resolve(&self, query: &str) -> Result<PathBuf> {
        let query = query.trim();
        if query.is_empty() {
            return Err(invalid("preset name must not be empty"));
        }
        let direct = self.root.join(format!("{}.json", slugify(query)?));
        if direct.is_file() {
            return Ok(direct);
        }
        for summary in self.list()? {
            if summary.name.eq_ignore_ascii_case(query) {
                return Ok(self.root.join(summary.file));
            }
        }
        Err(WatchdogError::new(
            ErrorCode::NotFound,
            format!("preset '{query}' was not found in {}", self.root.display()),
        )
        .with_hint("run `watchdog device preset list` to see saved presets"))
    }

    /// Save a preset, setting `created_at` once and refreshing `updated_at`.
    ///
    /// Returns the file path. Saving a preset with an existing name overwrites it.
    pub fn save(&self, preset: &mut Preset) -> Result<PathBuf> {
        preset.validate()?;
        self.ensure_dir()?;
        let path = self.root.join(format!("{}.json", slugify(&preset.name)?));
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        if preset.created_at.is_none() {
            preset.created_at = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Preset>(&bytes).ok())
                .and_then(|existing| existing.created_at)
                .or_else(|| Some(now.clone()));
        }
        preset.updated_at = Some(now);
        atomic_write(&path, &serde_json::to_vec_pretty(preset)?)?;
        Ok(path)
    }

    /// Delete a preset by display name or slug.
    pub fn delete(&self, query: &str) -> Result<()> {
        fs::remove_file(self.resolve(query)?)?;
        Ok(())
    }

    /// Write a preset to an arbitrary JSON file.
    pub fn export(&self, query: &str, target: &Path) -> Result<()> {
        let preset = self.load(query)?;
        atomic_write(target, &serde_json::to_vec_pretty(&preset)?)
    }

    /// Read a preset from an arbitrary JSON file and store it.
    pub fn import(&self, source: &Path, rename: Option<&str>) -> Result<Preset> {
        let mut preset = read_preset(source)?;
        if let Some(name) = rename {
            preset.name = name.to_string();
            preset.validate()?;
        }
        self.save(&mut preset)?;
        Ok(preset)
    }
}

/// Convert a display name to its file stem.
///
/// ASCII letters/digits are lowercased, runs of spaces, `-`, `_` and `.` become one `-`,
/// and leading/trailing dashes are removed.
pub fn slugify(name: &str) -> Result<String> {
    validate_name(name)?;
    let mut slug = String::new();
    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        return Err(invalid(
            "preset name must contain at least one ASCII letter or digit",
        ));
    }
    Ok(slug)
}

/// Whether `id` is a read-only state entry.
#[must_use]
pub fn is_read_only(id: &str) -> bool {
    READ_ONLY_CONTROLS.contains(&id)
}

/// Whether `id` is a one-shot trigger that is never stored.
#[must_use]
pub fn is_one_shot(id: &str) -> bool {
    ONE_SHOT_CONTROLS.contains(&id)
}

/// Whether `id` changes the capture session or pipeline.
#[must_use]
pub fn is_structural(id: &str) -> bool {
    STRUCTURAL_CONTROLS.contains(&id)
}

fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(invalid("preset name must not be empty"));
    }
    if trimmed != name {
        return Err(invalid("preset name must not start or end with whitespace"));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(invalid(format!(
            "preset name is longer than {MAX_NAME_LEN} bytes"
        )));
    }
    if name
        .chars()
        .any(|character| character.is_control() || character == '/' || character == '\\')
    {
        return Err(invalid(
            "preset name must not contain path separators or control characters",
        ));
    }
    if !name.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.')
    }) {
        return Err(invalid(
            "preset name may only use ASCII letters, digits, spaces, '-', '_' and '.'",
        ));
    }
    Ok(())
}

fn validate_control_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(invalid("preset control ids must not be empty"));
    }
    if id.len() > MAX_CONTROL_ID_LEN {
        return Err(invalid(format!(
            "control id '{id}' is longer than {MAX_CONTROL_ID_LEN} bytes"
        )));
    }
    if !id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err(invalid(format!(
            "control id '{id}' may only use ASCII letters, digits, '_', '-' and '.'"
        )));
    }
    if is_read_only(id) {
        return Err(invalid(format!(
            "control '{id}' is read-only and cannot be stored in a preset"
        )));
    }
    if is_one_shot(id) {
        return Err(invalid(format!(
            "control '{id}' is a one-shot trigger and cannot be stored in a preset"
        )));
    }
    Ok(())
}

fn read_preset(path: &Path) -> Result<Preset> {
    let bytes = fs::read(path).map_err(|error| {
        WatchdogError::new(
            ErrorCode::IoError,
            format!("cannot read preset {}: {error}", path.display()),
        )
        .with_source(error)
    })?;
    // Accept files saved by Windows editors that prepend a UTF-8 BOM.
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let preset: Preset = serde_json::from_slice(bytes).map_err(|error| {
        invalid(format!("cannot parse preset {}: {error}", path.display())).with_source(error)
    })?;
    preset.validate()?;
    Ok(preset)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid(format!("invalid preset path {}", path.display())))?;
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        // Windows cannot rename over an existing file; retry after removing it.
        if path.exists() {
            fs::remove_file(path)?;
            fs::rename(&temporary, path)?;
        } else {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> WatchdogError {
    WatchdogError::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(label: &str) -> PresetStore {
        let root = std::env::temp_dir().join(format!(
            "watchdog-presets-{label}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&root);
        PresetStore::new(root)
    }

    fn sample() -> Preset {
        let mut controls = BTreeMap::new();
        controls.insert("iso".to_string(), Value::from(100));
        controls.insert("ae_mode".to_string(), Value::from(0));
        controls.insert("awb_lock".to_string(), Value::Bool(true));
        Preset::new(
            "TV screen capture",
            Some("manual exposure, locked white balance".to_string()),
            controls,
        )
        .unwrap()
    }

    #[test]
    fn slugify_is_stable() {
        assert_eq!(slugify("TV screen capture").unwrap(), "tv-screen-capture");
        assert_eq!(
            slugify("  A/B  ").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(slugify("v1.2 test").unwrap(), "v1-2-test");
        assert_eq!(
            slugify("../evil").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn round_trip_sets_timestamps() {
        let store = temp_store("round-trip");
        let mut preset = sample();
        let path = store.save(&mut preset).unwrap();
        assert!(path.ends_with("tv-screen-capture.json"));
        assert!(preset.created_at.is_some());
        assert_eq!(preset.created_at, preset.updated_at);
        let loaded = store.load("TV screen capture").unwrap();
        assert_eq!(loaded, preset);
    }

    #[test]
    fn list_is_sorted_and_skips_non_json() {
        let store = temp_store("list");
        let mut first = sample();
        store.save(&mut first).unwrap();
        let mut second = sample();
        second.name = "Alpha".to_string();
        store.save(&mut second).unwrap();
        fs::write(store.root().join("notes.txt"), b"ignore me").unwrap();
        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[1].name, "TV screen capture");
    }

    #[test]
    fn save_preserves_created_at_on_overwrite() {
        let store = temp_store("overwrite");
        let mut preset = sample();
        store.save(&mut preset).unwrap();
        let created = preset.created_at.clone();
        let mut changed = sample();
        changed
            .controls
            .insert("zoom_ratio".to_string(), Value::from(2.0));
        store.save(&mut changed).unwrap();
        assert_eq!(changed.created_at, created);
        assert!(changed.updated_at.is_some());
    }

    #[test]
    fn export_and_import_round_trip() {
        let store = temp_store("export");
        let mut preset = sample();
        store.save(&mut preset).unwrap();
        let target = store.root().join("exported.json");
        store.export("TV screen capture", &target).unwrap();
        let imported = store.import(&target, Some("Imported preset")).unwrap();
        assert_eq!(imported.name, "Imported preset");
        assert_eq!(
            store.load("imported preset").unwrap().controls,
            preset.controls
        );
    }

    #[test]
    fn import_accepts_a_utf8_bom() {
        let store = temp_store("bom");
        let path = store.root().join("bom.json");
        fs::create_dir_all(store.root()).unwrap();
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend(serde_json::to_vec(&sample()).unwrap());
        fs::write(&path, bytes).unwrap();
        let imported = store.import(&path, Some("BOM preset")).unwrap();
        assert_eq!(imported.name, "BOM preset");
        assert_eq!(
            store.load("BOM preset").unwrap().controls,
            sample().controls
        );
    }

    #[test]
    fn delete_removes_the_file() {
        let store = temp_store("delete");
        let mut preset = sample();
        store.save(&mut preset).unwrap();
        store.delete("TV screen capture").unwrap();
        assert_eq!(store.list().unwrap().len(), 0);
        assert_eq!(
            store.load("TV screen capture").unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn validation_rejects_bad_presets() {
        let mut controls = BTreeMap::new();
        controls.insert("iso".to_string(), Value::from(100));
        let mut preset = Preset::new("ok", None, controls).unwrap();
        preset.schema_version = 2;
        assert_eq!(
            preset.validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let mut controls = BTreeMap::new();
        controls.insert("battery_percent".to_string(), Value::from(50));
        assert!(Preset::new("read only", None, controls).is_err());

        let mut controls = BTreeMap::new();
        controls.insert("af_trigger".to_string(), Value::from("start"));
        assert!(Preset::new("one shot", None, controls).is_err());

        let mut controls = BTreeMap::new();
        controls.insert("iso".to_string(), Value::Null);
        assert!(Preset::new("null", None, controls).is_err());

        assert!(Preset::new("", None, BTreeMap::new()).is_err());
        assert!(Preset::new("bad/name", None, BTreeMap::new()).is_err());
    }

    #[test]
    fn assignments_put_camera_first() {
        let mut controls = BTreeMap::new();
        controls.insert("zoom_ratio".to_string(), Value::from(2.0));
        controls.insert("camera_id".to_string(), Value::from("1"));
        controls.insert("iso".to_string(), Value::from(100));
        let preset = Preset::new("order", None, controls).unwrap();
        let ids = preset
            .assignments()
            .into_iter()
            .map(|assignment| assignment.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["camera_id", "iso", "zoom_ratio"]);
    }

    #[test]
    fn structural_classification_matches_the_protocol() {
        assert!(is_structural("camera_id"));
        assert!(is_structural("stream_size"));
        assert!(!is_structural("iso"));
        assert!(is_read_only("awb_gains"));
        assert!(is_one_shot("af_trigger"));
    }
}
