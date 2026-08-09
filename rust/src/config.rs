use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, ScavengerError};
use crate::ai::models::AIConfig;
use crate::models::Profile;

/// Write `contents` to `path` atomically: write a temp file in the same
/// directory, fsync it, then rename it over the destination (an atomic
/// operation on the same filesystem). A crash or disk error can no longer
/// leave a truncated or empty config — the old file survives intact until
/// the rename succeeds.
fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_string());
    let tmp = dir.join(format!(".{file_name}.tmp.{}", std::process::id()));

    let write_tmp = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = write_tmp() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ScavengerError::Config(format!("Cannot write config: {e}")));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ScavengerError::Config(format!("Cannot write config: {e}")));
    }
    // Best-effort durability of the rename itself.
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlobalConfig {
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_image_cache_path")]
    pub image_cache_path: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_socket_path")]
    pub socket_path: String,
    pub home_zip: Option<String>,
    #[serde(default = "default_tui_refresh")]
    pub tui_refresh_sec: f64,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
            image_cache_path: default_image_cache_path(),
            log_level: default_log_level(),
            socket_path: default_socket_path(),
            home_zip: None,
            tui_refresh_sec: default_tui_refresh(),
        }
    }
}

fn default_db_path() -> String {
    "~/.local/share/scavenger/scavenger.db".to_owned()
}
fn default_image_cache_path() -> String {
    "~/.cache/scavenger/images".to_owned()
}
fn default_log_level() -> String {
    "INFO".to_owned()
}
fn default_socket_path() -> String {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|runtime| !runtime.is_empty())
        .map(|runtime| {
            PathBuf::from(runtime)
                .join("scavenger")
                .join("daemon.sock")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "~/.run/scavenger/daemon.sock".to_owned())
}
fn default_tui_refresh() -> f64 {
    2.0
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub global_config: GlobalConfig,
    pub profiles: Vec<Profile>,
}

impl AppConfig {
    pub fn db_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.db_path)
    }

    pub fn socket_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.socket_path)
    }

    pub fn log_level(&self) -> &str {
        &self.global_config.log_level
    }
}

/// Intermediate type for TOML deserialization.
/// Extra sections (like [ai]) are silently ignored by serde.
#[derive(Deserialize)]
struct RawConfig {
    global: Option<GlobalConfig>,
    profiles: Option<Vec<Profile>>,
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScavengerError::Config(format!("Config file not found: {}", path.display()))
        } else {
            ScavengerError::Config(format!("Cannot read config: {e}"))
        }
    })?;

    let parsed: RawConfig = toml::from_str(&raw)
        .map_err(|e| ScavengerError::Config(format!("Invalid TOML: {e}")))?;

    let global_config = parsed.global.unwrap_or_default();
    let profiles = parsed.profiles.unwrap_or_default();

    for p in &profiles {
        p.validate()?;
    }

    Ok(AppConfig {
        global_config,
        profiles,
    })
}

pub fn load_ai_config(path: &Path) -> Result<AIConfig> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AIConfig::default());
        }
        Err(e) => {
            return Err(ScavengerError::Config(format!("Cannot read AI config: {e}")));
        }
    };

    #[derive(Deserialize)]
    struct Wrapper {
        ai: Option<AIConfig>,
    }

    let parsed: Wrapper = toml::from_str(&raw)
        .map_err(|e| ScavengerError::Config(format!("Invalid AI config TOML: {e}")))?;

    match parsed.ai {
        Some(mut cfg) => {
            cfg.resolve_env_keys();
            Ok(cfg)
        }
        None => Ok(AIConfig::default()),
    }
}

fn strings_to_array(items: &[String]) -> toml_edit::Array {
    let mut arr = toml_edit::Array::new();
    for s in items {
        arr.push(s.as_str());
    }
    arr
}

/// Convert a Profile to a toml_edit::Item for round-trip editing.
fn profile_to_item(profile: &Profile) -> toml_edit::Item {
    use toml_edit::{value, Array, Item, Table};

    let mut t = Table::new();
    t.insert("id", value(&profile.id));
    t.insert("name", value(&profile.name));

    let mut kw_arr = Array::new();
    for group in &profile.keywords {
        match group {
            crate::models::KeywordGroup::Single(s) => {
                kw_arr.push(s.as_str());
            }
            crate::models::KeywordGroup::Any(variants) => {
                kw_arr.push(strings_to_array(variants));
            }
        }
    }
    t.insert("keywords", Item::Value(toml_edit::Value::Array(kw_arr)));
    t.insert("sources", Item::Value(toml_edit::Value::Array(strings_to_array(&profile.sources))));
    t.insert("enabled", value(profile.enabled));

    if !profile.negative_keywords.is_empty() {
        t.insert("negative_keywords", Item::Value(toml_edit::Value::Array(strings_to_array(&profile.negative_keywords))));
    }
    if let Some(v) = profile.price_min {
        t.insert("price_min", value(v));
    }
    if let Some(v) = profile.price_max {
        t.insert("price_max", value(v));
    }
    if profile.poll_interval_sec != 3600 {
        t.insert("poll_interval_sec", value(profile.poll_interval_sec as i64));
    }
    if !matches!(profile.alert_priority, crate::models::AlertPriority::Normal) {
        let s = match profile.alert_priority {
            crate::models::AlertPriority::High => "high",
            crate::models::AlertPriority::Normal => "normal",
            crate::models::AlertPriority::Low => "low",
        };
        t.insert("alert_priority", value(s));
    }
    if !profile.tags.is_empty() {
        t.insert("tags", Item::Value(toml_edit::Value::Array(strings_to_array(&profile.tags))));
    }
    if !profile.escalation_keywords.is_empty() {
        t.insert("escalation_keywords", Item::Value(toml_edit::Value::Array(strings_to_array(&profile.escalation_keywords))));
    }
    if let Some(v) = profile.location_radius_mi {
        t.insert("location_radius_mi", value(v as i64));
    }

    Item::Table(t)
}

pub fn append_profile(path: &Path, profile: &Profile) -> Result<Profile> {
    profile.validate()?;

    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ScavengerError::Config(format!("Cannot read config: {e}"))),
    };

    let mut doc: toml_edit::DocumentMut = raw
        .parse()
        .map_err(|e| ScavengerError::Config(format!("Invalid TOML: {e}")))?;

    // Check for duplicate ID
    if let Some(profiles) = doc.get("profiles").and_then(|v| v.as_array_of_tables()) {
        for existing in profiles.iter() {
            if existing.get("id").and_then(|v| v.as_str()) == Some(&profile.id) {
                return Err(ScavengerError::Config(format!(
                    "Profile ID already exists: {}",
                    profile.id
                )));
            }
        }
    }

    // Build the new profile table
    let item = profile_to_item(profile);
    let table = match item {
        toml_edit::Item::Table(t) => t,
        _ => unreachable!(),
    };

    // Append to [[profiles]] array of tables
    if doc.get("profiles").is_none() {
        let arr = toml_edit::ArrayOfTables::new();
        doc.insert("profiles", toml_edit::Item::ArrayOfTables(arr));
    }

    if let Some(arr) = doc.get_mut("profiles").and_then(|v| v.as_array_of_tables_mut()) {
        arr.push(table);
    }

    atomic_write(path, &doc.to_string())?;

    Ok(profile.clone())
}

pub fn update_profile(path: &Path, profile: &Profile) -> Result<Profile> {
    profile.validate()?;

    let raw = std::fs::read_to_string(path)
        .map_err(|e| ScavengerError::Config(format!("Cannot read config: {e}")))?;

    let mut doc: toml_edit::DocumentMut = raw
        .parse()
        .map_err(|e| ScavengerError::Config(format!("Invalid TOML: {e}")))?;

    let profiles = doc
        .get_mut("profiles")
        .and_then(|v| v.as_array_of_tables_mut())
        .ok_or_else(|| ScavengerError::Config(format!("Profile not found: {}", profile.id)))?;

    let idx = profiles
        .iter()
        .position(|t| t.get("id").and_then(|v| v.as_str()) == Some(&profile.id))
        .ok_or_else(|| ScavengerError::Config(format!("Profile not found: {}", profile.id)))?;

    let item = profile_to_item(profile);
    let table = match item {
        toml_edit::Item::Table(t) => t,
        _ => unreachable!(),
    };

    // Replace the table at idx. toml_edit doesn't have replace, so we rebuild.
    let mut new_arr = toml_edit::ArrayOfTables::new();
    for (i, existing) in profiles.iter().enumerate() {
        if i == idx {
            new_arr.push(table.clone());
        } else {
            new_arr.push(existing.clone());
        }
    }
    doc.insert("profiles", toml_edit::Item::ArrayOfTables(new_arr));

    atomic_write(path, &doc.to_string())?;

    Ok(profile.clone())
}

pub fn delete_profile(path: &Path, profile_id: &str) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| ScavengerError::Config(format!("Cannot read config: {e}")))?;

    let mut doc: toml_edit::DocumentMut = raw
        .parse()
        .map_err(|e| ScavengerError::Config(format!("Invalid TOML: {e}")))?;

    let profiles = doc
        .get_mut("profiles")
        .and_then(|v| v.as_array_of_tables_mut())
        .ok_or_else(|| ScavengerError::Config(format!("Profile not found: {profile_id}")))?;

    let idx = profiles
        .iter()
        .position(|t| t.get("id").and_then(|v| v.as_str()) == Some(profile_id))
        .ok_or_else(|| ScavengerError::Config(format!("Profile not found: {profile_id}")))?;

    // Rebuild without the deleted entry
    let mut new_arr = toml_edit::ArrayOfTables::new();
    for (i, existing) in profiles.iter().enumerate() {
        if i != idx {
            new_arr.push(existing.clone());
        }
    }
    doc.insert("profiles", toml_edit::Item::ArrayOfTables(new_arr));

    atomic_write(path, &doc.to_string())?;

    Ok(())
}

/// Default config file path.
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/scavenger/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let p = expand_tilde("~/.config/scavenger/config.toml");
        assert!(!p.to_str().unwrap().starts_with('~'));
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "old contents").unwrap();

        atomic_write(&path, "new contents").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new contents");

        // No leftover temp file in the directory (the rename consumed it).
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
    }
}
