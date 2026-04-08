use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::Profile;

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
    #[serde(default)]
    pub home_zip: Option<String>,
    #[serde(default = "default_tui_refresh")]
    pub tui_refresh_sec: f64,
}

fn default_db_path() -> String {
    "~/.local/share/scavenger/scavenger.db".into()
}
fn default_image_cache_path() -> String {
    "~/.cache/scavenger/images".into()
}
fn default_log_level() -> String {
    "INFO".into()
}
fn default_socket_path() -> String {
    "~/.run/scavenger/daemon.sock".into()
}
fn default_tui_refresh() -> f64 {
    2.0
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

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default, rename = "global")]
    pub global_config: GlobalConfig,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl AppConfig {
    pub fn db_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.db_path)
    }

    pub fn socket_path(&self) -> PathBuf {
        expand_tilde(&self.global_config.socket_path)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AIConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_litellm_base_url")]
    pub litellm_base_url: String,
    #[serde(default = "default_filter_model")]
    pub filter_model: String,
    #[serde(default = "default_filter_timeout")]
    pub filter_timeout_sec: f64,
    #[serde(default = "default_escalation_model")]
    pub escalation_model: String,
    #[serde(default)]
    pub escalation_enabled: bool,
    #[serde(default = "default_escalation_min_score")]
    pub escalation_min_keyword_score: f64,
    #[serde(default = "default_escalation_timeout")]
    pub escalation_timeout_sec: f64,
    #[serde(default)]
    pub anthropic_api_key: String,
}

fn default_litellm_base_url() -> String {
    "http://localhost:11434/v1".into()
}
fn default_filter_model() -> String {
    "qwen3.5:9b".into()
}
fn default_filter_timeout() -> f64 {
    30.0
}
fn default_escalation_model() -> String {
    "claude-haiku-4-5-20251001".into()
}
fn default_escalation_min_score() -> f64 {
    70.0
}
fn default_escalation_timeout() -> f64 {
    30.0
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            litellm_base_url: default_litellm_base_url(),
            filter_model: default_filter_model(),
            filter_timeout_sec: default_filter_timeout(),
            escalation_model: default_escalation_model(),
            escalation_enabled: false,
            escalation_min_keyword_score: default_escalation_min_score(),
            escalation_timeout_sec: default_escalation_timeout(),
            anthropic_api_key: String::new(),
        }
    }
}

impl AIConfig {
    /// Resolve the Anthropic API key from the config value, environment, or dotenv file.
    pub fn resolve_api_key(mut self) -> Self {
        if self.anthropic_api_key.is_empty() {
            if let Ok(val) = env::var("ANTHROPIC_API_KEY") {
                self.anthropic_api_key = val;
            }
        }
        if self.anthropic_api_key.is_empty() {
            let dotenv = expand_tilde("~/.config/scavenger/.env");
            if let Ok(contents) = fs::read_to_string(&dotenv) {
                for line in contents.lines() {
                    let line = line.trim();
                    if line.starts_with("ANTHROPIC_API_KEY=") && !line.starts_with('#') {
                        if let Some(val) = line.splitn(2, '=').nth(1) {
                            let val = val.trim();
                            if !val.is_empty() {
                                self.anthropic_api_key = val.to_string();
                                break;
                            }
                        }
                    }
                }
            }
        }
        self
    }
}

/// Raw TOML structure for deserialization (top-level has optional [ai] section).
#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    global: Option<GlobalConfig>,
    #[serde(default)]
    profiles: Option<Vec<Profile>>,
    #[serde(default)]
    ai: Option<AIConfig>,
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Config file not found: {}", path.display()))?;
    let parsed: RawConfig =
        toml::from_str(&raw).with_context(|| format!("Invalid TOML in {}", path.display()))?;
    Ok(AppConfig {
        global_config: parsed.global.unwrap_or_default(),
        profiles: parsed.profiles.unwrap_or_default(),
    })
}

pub fn load_ai_config(path: &Path) -> Result<AIConfig> {
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return Ok(AIConfig::default()),
    };
    let parsed: RawConfig = toml::from_str(&raw)
        .with_context(|| format!("Invalid AI config TOML in {}", path.display()))?;
    let ai = parsed.ai.unwrap_or_default();
    Ok(ai.resolve_api_key())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

pub fn default_config_path() -> PathBuf {
    expand_tilde("~/.config/scavenger/config.toml")
}
