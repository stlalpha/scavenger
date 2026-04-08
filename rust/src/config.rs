use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::models::Profile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub db_path: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
    pub log_level: Option<String>,
    pub home_zip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    #[serde(default)]
    pub enabled: bool,
    pub model: Option<String>,
    pub ollama_url: Option<String>,
    pub escalation_model: Option<String>,
    pub anthropic_api_key: Option<String>,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            ollama_url: None,
            escalation_model: None,
            anthropic_api_key: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub global_config: GlobalConfig,
    pub profiles: Vec<Profile>,
    pub ai: AIConfig,
    pub db_path: PathBuf,
    pub socket_path: PathBuf,
    pub log_level: String,
}

impl AppConfig {
    /// Stub: load config from a TOML file. Full implementation in config work unit.
    pub fn load(_path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        unimplemented!("config loading is implemented in the config work unit")
    }
}
