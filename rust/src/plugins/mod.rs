pub mod craigslist;
pub mod craigslist_cities;

use crate::models::{Listing, Profile};

/// Error indicating the scraper was blocked by bot detection.
#[derive(Debug, thiserror::Error)]
#[error("bot detected on {source}: {message}")]
pub struct BotDetectedError {
    pub source: String,
    pub message: String,
}

/// Trait for marketplace scraper plugins.
#[allow(async_fn_in_trait)]
pub trait Plugin: Send + Sync {
    fn plugin_id(&self) -> &str;
    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>;
    fn supports_geo(&self) -> bool;
}
