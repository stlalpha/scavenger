use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use thiserror::Error;

use crate::models::{Listing, ListingStatus, PricePoint, SourceState};

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS listings (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT DEFAULT '',
    price REAL,
    currency TEXT DEFAULT 'USD',
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

CREATE TABLE IF NOT EXISTS price_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    listing_id TEXT NOT NULL REFERENCES listings(id),
    price REAL NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS image_cache (
    url_hash TEXT PRIMARY KEY,
    local_path TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
    plugin_id TEXT PRIMARY KEY,
    last_polled TEXT,
    consecutive_errors INTEGER DEFAULT 0,
    rate_limit_until TEXT
);
"#;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid status: {0}")]
    InvalidStatus(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("datetime parse error: {0}")]
    DateTimeParse(#[from] chrono::ParseError),
}

pub type Result<T> = std::result::Result<T, DbError>;

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn parse_dt(s: &str) -> std::result::Result<DateTime<Utc>, chrono::ParseError> {
    // Try RFC 3339 first, then fall back to other ISO-8601 variants.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Python's isoformat with +00:00 should parse above; try naive UTC as last resort.
    s.parse::<DateTime<Utc>>()
}

fn parse_dt_opt(val: Option<String>) -> std::result::Result<Option<DateTime<Utc>>, chrono::ParseError> {
    val.map(|s| parse_dt(&s)).transpose()
}

fn row_to_source_state(row: &Row<'_>) -> rusqlite::Result<SourceState> {
    let last_polled: Option<String> = row.get("last_polled")?;
    let rate_limit_until: Option<String> = row.get("rate_limit_until")?;
    Ok(SourceState {
        plugin_id: row.get("plugin_id")?,
        last_polled: parse_dt_opt(last_polled).unwrap_or(None),
        consecutive_errors: row.get("consecutive_errors")?,
        rate_limit_until: parse_dt_opt(rate_limit_until).unwrap_or(None),
    })
}

fn row_to_listing(row: &Row<'_>) -> rusqlite::Result<Listing> {
    let image_urls_json: String = row.get("image_urls")?;
    let image_urls: Vec<String> =
        serde_json::from_str(&image_urls_json).unwrap_or_default();

    let first_seen_str: String = row.get("first_seen")?;
    let last_seen_str: String = row.get("last_seen")?;
    let status_str: String = row.get("status")?;

    let first_seen = parse_dt(&first_seen_str)
        .unwrap_or_else(|_| Utc::now());
    let last_seen = parse_dt(&last_seen_str)
        .unwrap_or_else(|_| Utc::now());
    let status = ListingStatus::from_str_checked(&status_str)
        .unwrap_or(ListingStatus::New);

    Ok(Listing {
        id: row.get("id")?,
        profile_id: row.get("profile_id")?,
        source_id: row.get("source_id")?,
        title: row.get("title")?,
        description: row.get::<_, Option<String>>("description")?.unwrap_or_default(),
        price: row.get("price")?,
        currency: row.get::<_, Option<String>>("currency")?.unwrap_or_else(|| "USD".to_string()),
        condition: row.get("condition")?,
        url: row.get("url")?,
        image_urls,
        location: row.get("location")?,
        first_seen,
        last_seen,
        relevance_score: row.get("relevance_score")?,
        status,
        ai_evaluation: row.get("ai_evaluation")?,
    })
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open a database at the given path. Use ":memory:" for in-memory databases.
    pub fn open(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            let p = Path::new(path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            Connection::open(p)?
        };
        Ok(Self { conn })
    }

    /// Create tables and set pragmas.
    pub fn init(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        self.conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
        Ok(())
    }

    /// Idempotently add columns that may be missing from older schemas.
    pub fn migrate(&self) -> Result<()> {
        let columns: HashSet<String> = self
            .conn
            .prepare("PRAGMA table_info(listings)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        if !columns.contains("ai_evaluation") {
            self.conn
                .execute_batch("ALTER TABLE listings ADD COLUMN ai_evaluation TEXT")?;
        }
        if !columns.contains("snoozed_until") {
            self.conn
                .execute_batch("ALTER TABLE listings ADD COLUMN snoozed_until TEXT")?;
        }
        Ok(())
    }

    /// Close the connection explicitly. Also happens on drop.
    pub fn close(self) -> Result<()> {
        self.conn
            .close()
            .map_err(|(_, e)| DbError::Sqlite(e))
    }

    /// Insert a listing if it doesn't exist. Returns true if the listing was new.
    /// Tracks price history on insert and on price change for existing listings.
    pub fn upsert_listing(&self, listing: &Listing) -> Result<bool> {
        let image_urls_json = serde_json::to_string(&listing.image_urls)?;
        let first_seen = listing.first_seen.to_rfc3339();
        let last_seen = listing.last_seen.to_rfc3339();
        let status = listing.status.as_str();

        let rows = self.conn.execute(
            "INSERT OR IGNORE INTO listings \
             (id, profile_id, source_id, title, description, price, currency, \
              condition, url, image_urls, location, first_seen, last_seen, \
              relevance_score, status, ai_evaluation, snoozed_until) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, NULL)",
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
                image_urls_json,
                listing.location,
                first_seen,
                last_seen,
                listing.relevance_score,
                status,
                listing.ai_evaluation,
            ],
        )?;

        let is_new = rows == 1;
        if is_new {
            if let Some(price) = listing.price {
                self.conn.execute(
                    "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?1, ?2, ?3)",
                    params![listing.id, price, now_iso()],
                )?;
            }
        } else {
            // Update last_seen
            self.conn.execute(
                "UPDATE listings SET last_seen=?1 WHERE id=?2",
                params![last_seen, listing.id],
            )?;
            // Check for price change
            let existing_price: Option<f64> = self
                .conn
                .query_row(
                    "SELECT price FROM listings WHERE id=?1",
                    params![listing.id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();

            if let Some(new_price) = listing.price {
                if existing_price != Some(new_price) {
                    self.conn.execute(
                        "INSERT INTO price_history (listing_id, price, observed_at) VALUES (?1, ?2, ?3)",
                        params![listing.id, new_price, now_iso()],
                    )?;
                }
            }
        }
        Ok(is_new)
    }

    pub fn get_listing(&self, listing_id: &str) -> Result<Option<Listing>> {
        let result = self
            .conn
            .query_row(
                "SELECT * FROM listings WHERE id=?1",
                params![listing_id],
                row_to_listing,
            )
            .optional()?;
        Ok(result)
    }

    pub fn get_listings(
        &self,
        status: Option<&str>,
        profile_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Listing>> {
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(s) = status {
            conditions.push("status=?");
            param_values.push(Box::new(s.to_string()));
        }
        if let Some(p) = profile_id {
            conditions.push("profile_id=?");
            param_values.push(Box::new(p.to_string()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        param_values.push(Box::new(limit));

        let sql = format!(
            "SELECT * FROM listings{} ORDER BY first_seen DESC LIMIT ?",
            where_clause
        );

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), row_to_listing)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn update_listing_status(&self, listing_id: &str, status: &str) -> Result<()> {
        // Validate status
        ListingStatus::from_str_checked(status)
            .map_err(DbError::InvalidStatus)?;
        self.conn.execute(
            "UPDATE listings SET status=?1 WHERE id=?2",
            params![status, listing_id],
        )?;
        Ok(())
    }

    /// Delete all listings and their price history for a profile. Returns count of deleted listings.
    pub fn delete_profile_listings(&self, profile_id: &str) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM price_history WHERE listing_id IN \
             (SELECT id FROM listings WHERE profile_id=?1)",
            params![profile_id],
        )?;
        let count = self.conn.execute(
            "DELETE FROM listings WHERE profile_id=?1",
            params![profile_id],
        )?;
        Ok(count)
    }

    pub fn snooze_listing(&self, listing_id: &str, until: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE listings SET status='snoozed', snoozed_until=?1 WHERE id=?2",
            params![until.to_rfc3339(), listing_id],
        )?;
        Ok(())
    }

    /// Un-snooze listings whose snooze period has expired. Returns count of affected rows.
    pub fn unsnooze_expired(&self) -> Result<usize> {
        let count = self.conn.execute(
            "UPDATE listings SET status='seen', snoozed_until=NULL \
             WHERE status='snoozed' AND snoozed_until IS NOT NULL AND snoozed_until <= ?1",
            params![now_iso()],
        )?;
        Ok(count)
    }

    /// Get listings excluding dismissed and actively snoozed.
    pub fn get_active_listings(
        &self,
        profile_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Listing>> {
        let now = now_iso();
        let mut conditions = vec![
            "(status != 'dismissed' AND (status != 'snoozed' OR snoozed_until <= ?))".to_string(),
        ];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];

        if let Some(p) = profile_id {
            conditions.push("profile_id=?".to_string());
            param_values.push(Box::new(p.to_string()));
        }
        param_values.push(Box::new(limit));

        let sql = format!(
            "SELECT * FROM listings WHERE {} ORDER BY first_seen DESC LIMIT ?",
            conditions.join(" AND ")
        );

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), row_to_listing)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn get_price_history(&self, listing_id: &str) -> Result<Vec<PricePoint>> {
        let mut stmt = self.conn.prepare(
            "SELECT price, observed_at FROM price_history WHERE listing_id=?1 ORDER BY observed_at",
        )?;
        let rows = stmt
            .query_map(params![listing_id], |row| {
                let observed_at_str: String = row.get(1)?;
                let observed_at =
                    parse_dt(&observed_at_str).unwrap_or_else(|_| Utc::now());
                Ok(PricePoint {
                    price: row.get(0)?,
                    observed_at,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn get_source_state(&self, plugin_id: &str) -> Result<Option<SourceState>> {
        let result = self
            .conn
            .query_row(
                "SELECT * FROM sources WHERE plugin_id=?1",
                params![plugin_id],
                row_to_source_state,
            )
            .optional()?;
        Ok(result)
    }

    /// Return the subset of listing_ids that already exist in the database.
    /// Chunks by 500 to avoid SQLite parameter limits.
    pub fn get_existing_ids(&self, listing_ids: &[String]) -> Result<HashSet<String>> {
        if listing_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let mut result = HashSet::new();
        for chunk in listing_ids.chunks(500) {
            let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("SELECT id FROM listings WHERE id IN ({})", placeholders);
            let mut stmt = self.conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> =
                chunk.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;
            for row in rows {
                if let Ok(id) = row {
                    result.insert(id);
                }
            }
        }
        Ok(result)
    }

    /// Return {profile_id: count} for listings with status='new'.
    pub fn count_new_by_profile(&self) -> Result<HashMap<String, usize>> {
        let mut stmt = self.conn.prepare(
            "SELECT profile_id, COUNT(*) FROM listings WHERE status='new' GROUP BY profile_id",
        )?;
        let mut map = HashMap::new();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })?;
        for row in rows {
            if let Ok((pid, count)) = row {
                map.insert(pid, count);
            }
        }
        Ok(map)
    }

    pub fn get_all_source_states(&self) -> Result<Vec<SourceState>> {
        let mut stmt = self.conn.prepare(
            "SELECT plugin_id, last_polled, consecutive_errors, rate_limit_until FROM sources",
        )?;
        let rows = stmt
            .query_map([], row_to_source_state)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Return the most recent last_polled timestamp across all sources.
    pub fn get_most_recent_poll(&self) -> Result<Option<DateTime<Utc>>> {
        let result: Option<String> = self
            .conn
            .query_row(
                "SELECT MAX(last_polled) FROM sources WHERE last_polled IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        match result {
            Some(s) => Ok(Some(parse_dt(&s)?)),
            None => Ok(None),
        }
    }

    pub fn update_source_state(
        &self,
        plugin_id: &str,
        last_polled: Option<DateTime<Utc>>,
        consecutive_errors: i64,
        rate_limit_until: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sources (plugin_id, last_polled, consecutive_errors, rate_limit_until) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(plugin_id) DO UPDATE SET \
               last_polled=excluded.last_polled, \
               consecutive_errors=excluded.consecutive_errors, \
               rate_limit_until=excluded.rate_limit_until",
            params![
                plugin_id,
                last_polled.map(|d| d.to_rfc3339()),
                consecutive_errors,
                rate_limit_until.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }
}
