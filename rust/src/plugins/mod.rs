pub mod facebook;
pub mod images;

use async_trait::async_trait;
use thiserror::Error;

use crate::models::{Listing, Profile};

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("bot detected on {plugin_id} at {url}: {message}")]
    BotDetected {
        plugin_id: String,
        url: String,
        message: String,
    },

    #[error("scrape error: {0}")]
    Scrape(String),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

/// Trait implemented by all marketplace scraper plugins.
#[async_trait]
pub trait Plugin: Send + Sync {
    fn plugin_id(&self) -> &str;
    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, PluginError>;
    fn supports_geo(&self) -> bool;
}
