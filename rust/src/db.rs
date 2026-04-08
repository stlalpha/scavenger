use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::models::Listing;

/// Stub database. Full implementation in the db work unit.
pub struct Database {
    pub path: PathBuf,
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn init(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    pub async fn migrate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    pub async fn close(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    pub async fn get_existing_ids(
        &self,
        _ids: &[String],
    ) -> Result<HashSet<String>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(HashSet::new())
    }

    pub async fn upsert_listing(
        &self,
        _listing: &Listing,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(true)
    }

    pub async fn get_source_state(
        &self,
        _source_id: &str,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, Box<dyn std::error::Error + Send + Sync>>
    {
        Ok(None)
    }

    pub async fn update_source_state(
        &self,
        _source_id: &str,
        _last_polled: Option<chrono::DateTime<chrono::Utc>>,
        _consecutive_errors: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
