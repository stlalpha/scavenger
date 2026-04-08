use std::collections::HashMap;

use chrono::{Duration, Utc};

use crate::db::Database;
use crate::models::{Listing, ListingStatus};

const SNOOZE_HOURS: i64 = 24;

pub struct DataLayer<'a> {
    db: &'a Database,
}

impl<'a> DataLayer<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Unsnooze expired listings, then return active listings for the given profile.
    pub fn get_listings(
        &self,
        profile_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Listing>, rusqlite::Error> {
        let _ = self.db.unsnooze_expired();
        self.db.get_active_listings(profile_id, limit)
    }

    /// Return `{profile_id: new_count}`.
    pub fn get_profile_stats(&self) -> Result<HashMap<String, u32>, rusqlite::Error> {
        self.db.count_new_by_profile()
    }

    /// Mark a listing's status (save, dismiss, snooze).
    pub fn mark_status(&self, listing_id: &str, status: &str) -> Result<(), rusqlite::Error> {
        let listing = self.db.get_listing(listing_id)?;
        let Some(_listing) = listing else {
            return Ok(());
        };
        if status == "snoozed" {
            let until = Utc::now() + Duration::hours(SNOOZE_HOURS);
            self.db.snooze_listing(listing_id, until)
        } else {
            self.db.update_listing_status(listing_id, status)
        }
    }

    /// Transition new -> seen without downgrading other statuses.
    pub fn mark_seen(&self, listing_id: &str) -> Result<(), rusqlite::Error> {
        let listing = self.db.get_listing(listing_id)?;
        if let Some(l) = listing {
            if l.status == ListingStatus::New {
                self.db.update_listing_status(listing_id, "seen")?;
            }
        }
        Ok(())
    }

    pub fn get_last_source_poll(&self) -> Result<Option<String>, rusqlite::Error> {
        self.db.get_most_recent_poll()
    }

    pub fn get_source_states(&self) -> Result<Vec<crate::db::SourceState>, rusqlite::Error> {
        self.db.get_all_source_states()
    }
}
