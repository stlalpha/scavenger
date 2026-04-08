use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub url: String,
    pub title: String,
    pub description: String,
    pub price: Option<f64>,
    pub image_url: Option<String>,
    pub source: String,
    pub profile_id: String,
    pub status: ListingStatus,
    pub relevance_score: f64,
    pub ai_evaluation: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A keyword entry: either a single literal or a list of OR-variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeywordEntry {
    Single(String),
    OrGroup(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<KeywordEntry>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u64,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_poll_interval() -> u64 {
    900
}

fn default_enabled() -> bool {
    true
}
