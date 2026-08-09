pub mod browser;
pub mod craigslist;
pub mod craigslist_cities;
pub mod ebay;
pub mod facebook;
pub mod images;

use async_trait::async_trait;

use crate::models::{Listing, Profile};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Bot detected on {plugin_id}: {message}")]
    BotDetected {
        plugin_id: String,
        url: String,
        message: String,
    },
    #[error("Navigation error: {0}")]
    Navigation(String),
    #[error("Browser error: {0}")]
    Browser(String),
    #[error("{0}")]
    Other(String),
}

/// Legacy alias — some code references this directly.
pub type BotDetectedError = PluginError;

#[async_trait]
pub trait Plugin: Send + Sync {
    fn plugin_id(&self) -> &str;
    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>;
    async fn supports_geo(&self) -> bool;
}
