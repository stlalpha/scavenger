use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Keyword {
    Single(String),
    AnyOf(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<Keyword>,
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    pub poll_interval_sec: u32,
    pub alert_priority: String,
    pub enabled: bool,
    pub tags: Vec<String>,
    pub escalation_keywords: Vec<String>,
    pub location_radius_mi: Option<u32>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            keywords: Vec::new(),
            negative_keywords: Vec::new(),
            sources: Vec::new(),
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: "normal".to_string(),
            enabled: true,
            tags: Vec::new(),
            escalation_keywords: Vec::new(),
            location_radius_mi: None,
        }
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
