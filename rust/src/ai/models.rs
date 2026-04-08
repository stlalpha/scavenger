use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEvaluation {
    pub relevant: bool,
    pub reason: String,
    pub notable: Option<String>,
    pub escalate: bool,
}

impl AIEvaluation {
    /// Safe fallback -- never drop a listing due to model error.
    pub fn passthrough() -> Self {
        Self {
            relevant: true,
            reason: String::new(),
            notable: None,
            escalate: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
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

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            litellm_base_url: default_base_url(),
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

fn default_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}

fn default_filter_model() -> String {
    "qwen3.5:9b".to_string()
}

fn default_filter_timeout() -> f64 {
    30.0
}

fn default_escalation_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}

fn default_escalation_min_score() -> f64 {
    70.0
}

fn default_escalation_timeout() -> f64 {
    30.0
}
