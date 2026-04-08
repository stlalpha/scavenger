use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::models::Profile;

fn default_db_path() -> String {
    "~/.local/share/scavenger/scavenger.db".to_string()
}

fn default_image_cache_path() -> String {
    "~/.cache/scavenger/images".to_string()
}

fn default_log_level() -> String {
    "INFO".to_string()
}

fn default_socket_path() -> String {
    "~/.run/scavenger/daemon.sock".to_string()
}

fn default_tui_refresh_sec() -> f64 {
    2.0
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
    #[serde(default = "default_tui_refresh_sec")]
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
            tui_refresh_sec: default_tui_refresh_sec(),
        }
    }
}

/// Raw TOML structure — `[global]` plus `[[profiles]]`.
#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    global: GlobalConfig,
    #[serde(default)]
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub global_config: GlobalConfig,
    pub profiles: Vec<Profile>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("read config {}: {e}", path.display()))?;
        let raw: RawConfig =
            toml::from_str(&text).map_err(|e| format!("parse config: {e}"))?;
        Ok(Self {
            global_config: raw.global,
            profiles: raw.profiles,
        })
    }

    pub fn db_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.db_path)
    }

    pub fn socket_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.socket_path)
    }

    pub fn log_path(&self) -> PathBuf {
        self.db_path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("daemon.log")
    }
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs_path() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
