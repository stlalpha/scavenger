use serde::{Deserialize, Serialize};

use crate::ai::models::AIConfig;
use crate::models::Profile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub db_path: Option<String>,
    pub socket_path: Option<String>,
    pub log_level: Option<String>,
    pub home_zip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub global: GlobalConfig,
    pub profiles: Vec<Profile>,
    pub ai: AIConfig,
}

pub fn load_config() -> crate::error::Result<AppConfig> {
    todo!()
}

pub fn load_ai_config() -> crate::error::Result<AIConfig> {
    todo!()
}
