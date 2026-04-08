use std::future::Future;
use std::pin::Pin;

use crate::models::{Listing, Profile};

/// Error raised when a plugin detects bot/captcha blocking.
#[derive(Debug)]
pub struct BotDetectedError {
    pub plugin_id: String,
    pub url: String,
    pub message: String,
}

impl std::fmt::Display for BotDetectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bot detected on {}: {}", self.plugin_id, self.message)
    }
}

impl std::error::Error for BotDetectedError {}

type PluginResult = Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>;

/// Plugin trait: each source (ebay, craigslist, etc.) implements this.
pub trait Plugin: Send + Sync {
    fn plugin_id(&self) -> &str;
    fn supports_geo(&self) -> bool;
    fn fetch(&self, profile: &Profile) -> Pin<Box<dyn Future<Output = PluginResult> + Send + '_>>;
}
