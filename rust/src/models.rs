use serde::{Deserialize, Serialize};

/// A keyword entry: either a single keyword or a group of OR'd variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeywordEntry {
    Single(String),
    Group(Vec<String>),
}

/// Search profile configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<KeywordEntry>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u32,
    #[serde(default = "default_priority")]
    pub alert_priority: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub escalation_keywords: Vec<String>,
    pub location_radius_mi: Option<u32>,
}

fn default_poll_interval() -> u32 {
    3600
}

fn default_priority() -> String {
    "normal".to_string()
}

fn default_true() -> bool {
    true
}

/// A marketplace listing.
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
    pub first_seen: String,
    pub last_seen: String,
    #[serde(default)]
    pub relevance_score: f64,
    #[serde(default = "default_status")]
    pub status: String,
    pub ai_evaluation: Option<String>,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_status() -> String {
    "new".to_string()
}
