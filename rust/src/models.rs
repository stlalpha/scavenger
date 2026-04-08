use serde::{Deserialize, Serialize};

/// A keyword group: either a single term or a list of OR'd variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeywordGroup {
    Single(String),
    Variants(Vec<String>),
}

/// Minimal profile for scoring. Mirrors the fields needed by score_listing.
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
}
