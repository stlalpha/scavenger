pub mod browser;
pub mod craigslist;
pub mod craigslist_cities;
pub mod ebay;
pub mod facebook;
pub mod images;

use crate::error::ScavengerError;
use crate::models::{Listing, Profile};

/// Marker error for bot detection — plugins should return this so callers
/// can decide whether to retry or back off.
pub fn bot_detected(source: &str) -> ScavengerError {
    ScavengerError::Plugin(format!("bot detected on {source}"))
}

/// Trait that all source plugins implement.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn supports_geo(&self) -> bool;
    fn fetch(
        &self,
        profile: &Profile,
    ) -> impl std::future::Future<Output = crate::error::Result<Vec<Listing>>> + Send;
}
