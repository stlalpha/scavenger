use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqlResult};

use crate::models::{Listing, ListingStatus};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    pub fn migrate(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS listings (
                id TEXT PRIMARY KEY,
                profile_id TEXT NOT NULL,
                source_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                price REAL,
                currency TEXT NOT NULL DEFAULT 'USD',
                condition TEXT,
                url TEXT NOT NULL,
                image_urls TEXT NOT NULL DEFAULT '[]',
                location TEXT,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                relevance_score REAL NOT NULL DEFAULT 0.0,
                status TEXT NOT NULL DEFAULT 'new',
                ai_evaluation TEXT,
                snoozed_until TEXT
            );

            CREATE TABLE IF NOT EXISTS source_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                plugin_id TEXT NOT NULL,
                profile_id TEXT NOT NULL,
                last_polled TEXT,
                consecutive_errors INTEGER NOT NULL DEFAULT 0,
                UNIQUE(plugin_id, profile_id)
            );",
        )?;
        Ok(())
    }

    pub fn insert_listing(&self, listing: &Listing) -> SqlResult<()> {
        let image_urls = serde_json::to_string(&listing.image_urls).unwrap_or_default();
        self.conn.execute(
            "INSERT OR REPLACE INTO listings
             (id, profile_id, source_id, title, description, price, currency,
              condition, url, image_urls, location, first_seen, last_seen,
              relevance_score, status, ai_evaluation)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                listing.id,
                listing.profile_id,
                listing.source_id,
                listing.title,
                listing.description,
                listing.price,
                listing.currency,
                listing.condition,
                listing.url,
                image_urls,
                listing.location,
                listing.first_seen.to_rfc3339(),
                listing.last_seen.to_rfc3339(),
                listing.relevance_score,
                listing.status.to_string(),
                listing.ai_evaluation,
            ],
        )?;
        Ok(())
    }

    pub fn get_listing(&self, id: &str) -> SqlResult<Option<Listing>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM listings WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], row_to_listing)?;
        match rows.next() {
            Some(Ok(l)) => Ok(Some(l)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Active listings: not dismissed, not currently snoozed.
    pub fn get_active_listings(
        &self,
        profile_id: Option<&str>,
        limit: u32,
    ) -> SqlResult<Vec<Listing>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt;
        let rows = if let Some(pid) = profile_id {
            stmt = self.conn.prepare(
                "SELECT * FROM listings
                 WHERE profile_id = ?1
                   AND status NOT IN ('dismissed')
                   AND (status != 'snoozed' OR snoozed_until IS NULL OR snoozed_until <= ?2)
                 ORDER BY first_seen DESC
                 LIMIT ?3",
            )?;
            stmt.query_map(params![pid, now, limit], row_to_listing)?
        } else {
            stmt = self.conn.prepare(
                "SELECT * FROM listings
                 WHERE status NOT IN ('dismissed')
                   AND (status != 'snoozed' OR snoozed_until IS NULL OR snoozed_until <= ?1)
                 ORDER BY first_seen DESC
                 LIMIT ?2",
            )?;
            stmt.query_map(params![now, limit], row_to_listing)?
        };
        rows.collect()
    }

    pub fn update_listing_status(&self, id: &str, status: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE listings SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn snooze_listing(&self, id: &str, until: DateTime<Utc>) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE listings SET status = 'snoozed', snoozed_until = ?1 WHERE id = ?2",
            params![until.to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn unsnooze_expired(&self) -> SqlResult<usize> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE listings SET status = 'new', snoozed_until = NULL
             WHERE status = 'snoozed' AND snoozed_until <= ?1",
            params![now],
        )
    }

    pub fn count_new_by_profile(&self) -> SqlResult<HashMap<String, u32>> {
        let mut stmt = self.conn.prepare(
            "SELECT profile_id, COUNT(*) FROM listings WHERE status = 'new' GROUP BY profile_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        let mut map = HashMap::new();
        for r in rows {
            let (pid, count) = r?;
            map.insert(pid, count);
        }
        Ok(map)
    }

    pub fn get_most_recent_poll(&self) -> SqlResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT MAX(last_polled) FROM source_state")?;
        let result: Option<String> = stmt.query_row([], |row| row.get(0))?;
        Ok(result)
    }

    pub fn get_all_source_states(&self) -> SqlResult<Vec<SourceState>> {
        let mut stmt = self
            .conn
            .prepare("SELECT plugin_id, profile_id, last_polled, consecutive_errors FROM source_state")?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceState {
                plugin_id: row.get(0)?,
                profile_id: row.get(1)?,
                last_polled: row.get(2)?,
                consecutive_errors: row.get(3)?,
            })
        })?;
        rows.collect()
    }
}

#[derive(Debug, Clone)]
pub struct SourceState {
    pub plugin_id: String,
    pub profile_id: String,
    pub last_polled: Option<String>,
    pub consecutive_errors: u32,
}

fn row_to_listing(row: &rusqlite::Row) -> SqlResult<Listing> {
    let status_str: String = row.get("status")?;
    let status: ListingStatus = status_str
        .parse()
        .unwrap_or(ListingStatus::New);
    let image_urls_str: String = row.get("image_urls")?;
    let image_urls: Vec<String> =
        serde_json::from_str(&image_urls_str).unwrap_or_default();
    let first_seen_str: String = row.get("first_seen")?;
    let last_seen_str: String = row.get("last_seen")?;
    Ok(Listing {
        id: row.get("id")?,
        profile_id: row.get("profile_id")?,
        source_id: row.get("source_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        price: row.get("price")?,
        currency: row.get("currency")?,
        condition: row.get("condition")?,
        url: row.get("url")?,
        image_urls,
        location: row.get("location")?,
        first_seen: DateTime::parse_from_rfc3339(&first_seen_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_seen: DateTime::parse_from_rfc3339(&last_seen_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        relevance_score: row.get("relevance_score")?,
        status,
        ai_evaluation: row.get("ai_evaluation")?,
    })
}
