pub mod browser;

use anyhow::Result;
use async_trait::async_trait;

use crate::models::{Listing, Profile};

/// Error raised when a scraping target detects bot-like behavior.
#[derive(Debug, thiserror::Error)]
#[error("Bot detected on {plugin_id}: {message}")]
pub struct BotDetectedError {
    pub plugin_id: String,
    pub url: String,
    pub message: String,
}

/// Trait implemented by each marketplace scraper.
///
/// Plugins fetch listings from a single source (eBay, Craigslist, etc.)
/// and return raw results for scoring.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Unique identifier for this plugin (e.g. "ebay", "craigslist").
    fn plugin_id(&self) -> &str;

    /// Fetch listings matching the given profile.
    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>>;

    /// Whether this plugin supports geographic filtering.
    async fn supports_geo(&self) -> bool;
}
