use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Listing status in the user's workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A marketplace listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub status: ListingStatus,
    #[serde(default)]
    pub score: u32,
    #[serde(default)]
    pub profile_name: String,
    pub created_at: DateTime<Utc>,
}

/// Keyword group: a single string is a literal match, a list of strings is OR'd.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeywordGroup {
    Single(String),
    Any(Vec<String>),
}

/// A search profile defining what to look for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub keywords: Vec<KeywordGroup>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub min_price: Option<f64>,
    #[serde(default)]
    pub max_price: Option<f64>,
    /// Poll interval in seconds.
    #[serde(default = "default_poll_interval")]
    pub poll_interval: u64,
    #[serde(default)]
    pub zip_code: Option<String>,
    #[serde(default)]
    pub search_radius: Option<u32>,
}

fn default_poll_interval() -> u64 {
    300
}
