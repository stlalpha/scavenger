pub mod ebay;

use crate::models::{Listing, Profile};
use std::future::Future;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("bot detected on {plugin_id} at {url}: {message}")]
    BotDetected {
        plugin_id: String,
        url: String,
        message: String,
    },

    #[error("navigation error: {0}")]
    Navigation(String),

    #[error("browser error: {0}")]
    Browser(String),

    #[error("{0}")]
    Other(String),
}

/// Trait that all scraper plugins implement.
pub trait Plugin: Send + Sync {
    fn plugin_id(&self) -> &str;

    fn fetch(
        &self,
        profile: &Profile,
    ) -> impl Future<Output = Result<Vec<Listing>, PluginError>> + Send;

    fn supports_geo(&self) -> bool;
}
