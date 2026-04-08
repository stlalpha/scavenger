use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

/// Listing status in the pipeline.
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

/// A single marketplace listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub profile_id: String,
    pub source_id: String,
    pub title: String,
    pub description: String,
    pub price: Option<f64>,
    pub currency: String,
    pub condition: Option<String>,
    pub url: String,
    pub image_urls: Vec<String>,
    pub location: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub relevance_score: f64,
    pub status: ListingStatus,
    pub ai_evaluation: Option<String>,
}

/// A keyword entry: either a single term or a list of OR-variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeywordEntry {
    Single(String),
    Variants(Vec<String>),
}

/// Alert priority level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A user-defined search profile.
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

/// Query parameters stripped during URL normalization.
const STRIP_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "ssPageName",
    "_trkparms",
    "_trktoken",
    "hash",
    "ref",
    "mkevt",
    "mkcid",
    "mkrid",
    "campid",
    "toolid",
];

/// Normalize a URL by stripping tracking params.
pub fn normalize_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return raw.to_string();
    };
    let filtered: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            !STRIP_PARAMS.contains(&k.as_ref()) && !k.starts_with("utm_")
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    parsed.set_query(None);
    if !filtered.is_empty() {
        let mut pairs = parsed.query_pairs_mut();
        for (k, v) in &filtered {
            pairs.append_pair(k, v);
        }
    }
    parsed.set_fragment(None);
    parsed.to_string()
}

/// SHA-256 hex digest of the normalized URL. Used as listing primary key.
pub fn content_hash(url: &str) -> String {
    let normalized = normalize_url(url);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_strips_tracking_params() {
        let url = "https://example.com/item?id=1&utm_source=foo&ref=bar";
        let hash1 = content_hash(url);
        let hash2 = content_hash("https://example.com/item?id=1");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("https://example.com/item/123");
        let h2 = content_hash("https://example.com/item/123");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }
}
