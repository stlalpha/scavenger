use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub keywords: Vec<Keyword>,
    #[serde(default)]
    pub negative_keywords: Vec<String>,
    pub sources: Vec<String>,
    #[serde(default)]
    pub price_min: Option<f64>,
    #[serde(default)]
    pub price_max: Option<f64>,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_sec: u32,
    #[serde(default = "default_alert_priority")]
    pub alert_priority: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub escalation_keywords: Vec<String>,
    #[serde(default)]
    pub location_radius_mi: Option<u32>,
}

fn default_poll_interval() -> u32 {
    3600
}
fn default_alert_priority() -> String {
    "normal".into()
}
fn default_enabled() -> bool {
    true
}

/// A keyword entry: either a plain string or an OR-group of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Keyword {
    Single(String),
    OrGroup(Vec<String>),
}
