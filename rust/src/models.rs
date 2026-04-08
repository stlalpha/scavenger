use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, ScavengerError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    "USD".to_string()
}

/// A keyword group: either a single keyword (must match) or a list of alternatives (any must match).
/// In TOML config, keywords can be `"thing"` or `["variant1", "variant2"]`.
#[derive(Debug, Clone, PartialEq)]
pub enum KeywordGroup {
    Single(String),
    Any(Vec<String>),
}

impl Serialize for KeywordGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            KeywordGroup::Single(s) => serializer.serialize_str(s),
            KeywordGroup::Any(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for KeywordGroup {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        use serde::de;

        struct KeywordGroupVisitor;

        impl<'de> de::Visitor<'de> for KeywordGroupVisitor {
            type Value = KeywordGroup;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or array of strings")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<KeywordGroup, E> {
                Ok(KeywordGroup::Single(v.to_string()))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<KeywordGroup, A::Error> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub poll_interval_sec: u64,
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

fn default_poll_interval() -> u64 {
    3600
}

fn default_true() -> bool {
    true
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.poll_interval_sec < 30 {
            return Err(ScavengerError::Config(
                "poll_interval_sec must be >= 30".to_string(),
            ));
        }
        Ok(())
    }
}
