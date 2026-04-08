use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListingStatus {
    New,
    Seen,
    Saved,
    Dismissed,
    Snoozed,
}

impl ListingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Seen => "seen",
            Self::Saved => "saved",
            Self::Dismissed => "dismissed",
            Self::Snoozed => "snoozed",
        }
    }

    pub fn from_str_checked(s: &str) -> Result<Self, String> {
        match s {
            "new" => Ok(Self::New),
            "seen" => Ok(Self::Seen),
            "saved" => Ok(Self::Saved),
            "dismissed" => Ok(Self::Dismissed),
            "snoozed" => Ok(Self::Snoozed),
            other => Err(format!(
                "Invalid status: {other:?} (must be one of new, seen, saved, dismissed, snoozed)"
            )),
        }
    }
}

impl fmt::Display for ListingStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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
    pub status: ListingStatus,
    pub ai_evaluation: Option<String>,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_status() -> ListingStatus {
    ListingStatus::New
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub price: f64,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceState {
    pub plugin_id: String,
    pub last_polled: Option<DateTime<Utc>>,
    pub consecutive_errors: i64,
    pub rate_limit_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u32,
    #[serde(default = "default_priority")]
    pub alert_priority: AlertPriority,
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

fn default_true() -> bool {
    true
}

fn default_priority() -> AlertPriority {
    AlertPriority::Normal
}

/// A keyword entry: either a single literal or a list of OR-variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Keyword {
    Single(String),
    OrGroup(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertPriority {
    High,
    Normal,
    Low,
}
