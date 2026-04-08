use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A keyword group: either a single required keyword or a list of alternatives (any must match).
#[derive(Debug, Clone, PartialEq)]
pub enum KeywordGroup {
    Single(String),
    Any(Vec<String>),
}

impl Serialize for KeywordGroup {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            KeywordGroup::Single(s) => serializer.serialize_str(s),
            KeywordGroup::Any(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for KeywordGroup {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;

        struct KeywordGroupVisitor;

        impl<'de> de::Visitor<'de> for KeywordGroupVisitor {
            type Value = KeywordGroup;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or array of strings")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(KeywordGroup::Single(v.to_owned()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(KeywordGroup::Single(v))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<String>()? {
                    items.push(item);
                }
                Ok(KeywordGroup::Any(items))
            }
        }

        deserializer.deserialize_any(KeywordGroupVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListingStatus {
    New,
    Seen,
    Saved,
    Dismissed,
    Snoozed,
}

impl Default for ListingStatus {
    fn default() -> Self {
        Self::New
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertPriority {
    High,
    Normal,
    Low,
}

impl Default for AlertPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub profile_id: String,
    pub source_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub price: Option<f64>,
    #[serde(default = "default_currency")]
    pub currency: String,
    pub condition: Option<String>,
    pub url: String,
    #[serde(default)]
    pub image_urls: Vec<String>,
    pub location: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default)]
    pub status: ListingStatus,
    pub ai_evaluation: Option<String>,
}

fn default_currency() -> String {
    "USD".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<KeywordGroup>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: i64,
    #[serde(default)]
    pub alert_priority: AlertPriority,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub escalation_keywords: Vec<String>,
    pub location_radius_mi: Option<i64>,
}

fn default_poll_interval() -> i64 {
    3600
}

fn default_true() -> bool {
    true
}

impl Profile {
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.poll_interval_sec < 30 {
            return Err(crate::error::ScavengerError::Config(
                "poll_interval_sec must be >= 30".to_owned(),
            ));
        }
        Ok(())
    }
}

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
    #[serde(default = "default_ollama_url")]
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
            litellm_base_url: default_ollama_url(),
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
    /// Resolve anthropic_api_key from env / dotenv if not set in config.
    pub fn resolve_env_keys(mut self) -> Self {
        if self.anthropic_api_key.is_empty() {
            if let Ok(val) = std::env::var("ANTHROPIC_API_KEY") {
                self.anthropic_api_key = val;
            }
        }
        if self.anthropic_api_key.is_empty() {
            let dotenv = dirs::home_dir()
                .map(|h| h.join(".config/scavenger/.env"));
            if let Some(path) = dotenv {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    for line in contents.lines() {
                        let line = line.trim();
                        if line.starts_with("ANTHROPIC_API_KEY=") && !line.starts_with('#') {
                            if let Some(val) = line.splitn(2, '=').nth(1) {
                                let val = val.trim();
                                if !val.is_empty() {
                                    self.anthropic_api_key = val.to_owned();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        self
    }
}

fn default_ollama_url() -> String {
    "http://localhost:11434/v1".to_owned()
}

fn default_filter_model() -> String {
    "qwen3.5:9b".to_owned()
}

fn default_filter_timeout() -> f64 {
    30.0
}

fn default_escalation_model() -> String {
    "claude-haiku-4-5-20251001".to_owned()
}

fn default_escalation_min_score() -> f64 {
    70.0
}

fn default_escalation_timeout() -> f64 {
    30.0
}
