use std::collections::HashMap;

use chrono::{Duration, Utc};

use crate::db::{self, Database};
use crate::models::{Listing, ListingStatus};

const SNOOZE_HOURS: i64 = 24;

pub struct DataLayer<'a> {
    db: &'a Database,
}

impl<'a> DataLayer<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_listings(
        &self,
        profile_id: Option<&str>,
        limit: u32,
    ) -> db::Result<Vec<Listing>> {
        let _ = self.db.unsnooze_expired();
        self.db.get_active_listings(profile_id, limit)
    }

    pub fn get_profile_stats(&self) -> db::Result<HashMap<String, usize>> {
        self.db.count_new_by_profile()
    }

    pub fn mark_status(&self, listing_id: &str, status: &str) -> db::Result<()> {
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

    pub fn mark_seen(&self, listing_id: &str) -> db::Result<()> {
        let listing = self.db.get_listing(listing_id)?;
        if let Some(l) = listing {
            if l.status == ListingStatus::New {
                self.db.update_listing_status(listing_id, "seen")?;
            }
        }
        Ok(())
    }

    pub fn get_last_source_poll(&self) -> db::Result<Option<String>> {
        self.db.get_most_recent_poll().map(|opt| opt.map(|dt| dt.to_rfc3339()))
    }

    pub fn get_source_states(&self) -> db::Result<Vec<crate::db::SourceState>> {
        self.db.get_all_source_states()
    }
}
