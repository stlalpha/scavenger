use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

impl std::fmt::Display for ListingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "new"),
            Self::Seen => write!(f, "seen"),
            Self::Saved => write!(f, "saved"),
            Self::Dismissed => write!(f, "dismissed"),
            Self::Snoozed => write!(f, "snoozed"),
        }
    }
}

impl std::str::FromStr for ListingStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "new" => Ok(Self::New),
            "seen" => Ok(Self::Seen),
            "saved" => Ok(Self::Saved),
            "dismissed" => Ok(Self::Dismissed),
            "snoozed" => Ok(Self::Snoozed),
            other => Err(format!("unknown listing status: {other}")),
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
    #[serde(default)]
    pub status: ListingStatus,
    pub ai_evaluation: Option<String>,
}

fn default_currency() -> String {
    "USD".to_string()
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

/// A keyword entry: either a single required term or a list of OR-alternatives.
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
    pub price_min: Option<f64>,
    pub price_max: Option<f64>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u32,
    #[serde(default)]
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
