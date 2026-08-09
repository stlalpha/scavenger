//! `scavenger reset` — wipe search data while preserving everything that
//! identifies the user: config.toml profiles, sops-encrypted secrets, and
//! the Chrome profile (marketplace logins) are never touched.

use std::fs;
use std::path::Path;

use crate::config::AppConfig;
use crate::error::{Result, ScavengerError};

pub struct ResetOptions {
    /// Delete listings + price history only, preserving per-source poll
    /// state so the next daemon start does not re-poll every profile at
    /// once (and re-provoke marketplace bot detection).
    pub listings_only: bool,
}

pub struct ResetPlan {
    pub db_path: std::path::PathBuf,
    pub db_bytes: u64,
    pub listing_count: Option<u64>,
    pub image_cache_path: std::path::PathBuf,
    pub image_count: usize,
    pub listings_only: bool,
}

pub struct ResetOutcome {
    pub listings_deleted: u64,
    pub db_removed: bool,
    pub images_removed: usize,
}

fn count_listings(db_path: &Path) -> Option<u64> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    conn.query_row("SELECT count(*) FROM listings", [], |r| r.get(0))
        .ok()
}

fn image_files(dir: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default()
}

/// Describe what a reset would remove, without touching anything.
pub fn plan(config: &AppConfig, opts: &ResetOptions) -> ResetPlan {
    let db_path = config.db_path();
    let image_cache_path = crate::config::expand_tilde(&config.global_config.image_cache_path);
    ResetPlan {
        db_bytes: fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0),
        listing_count: count_listings(&db_path),
        image_count: image_files(&image_cache_path).len(),
        db_path,
        image_cache_path,
        listings_only: opts.listings_only,
    }
}

/// Execute the reset. The daemon must not be running (callers check the
/// socket first); the TUI must also be closed — both hold the DB open.
pub fn run(config: &AppConfig, opts: &ResetOptions) -> Result<ResetOutcome> {
    let db_path = config.db_path();

    if opts.listings_only {
        let mut deleted = 0u64;
        if db_path.exists() {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ScavengerError::Database(e.to_string()))?;
            // price_history references listings — child rows go first.
            conn.execute("DELETE FROM price_history", [])
                .map_err(|e| ScavengerError::Database(e.to_string()))?;
            deleted = conn
                .execute("DELETE FROM listings", [])
                .map_err(|e| ScavengerError::Database(e.to_string()))?
                as u64;
            conn.execute_batch("VACUUM")
                .map_err(|e| ScavengerError::Database(e.to_string()))?;
        }
        return Ok(ResetOutcome {
            listings_deleted: deleted,
            db_removed: false,
            images_removed: 0,
        });
    }

    let listings = count_listings(&db_path).unwrap_or(0);
    let mut db_removed = false;
    for suffix in ["", "-wal", "-shm"] {
        let p = std::path::PathBuf::from(format!("{}{suffix}", db_path.display()));
        if p.exists() {
            fs::remove_file(&p).map_err(|e| {
                ScavengerError::Database(format!("failed to remove {}: {e}", p.display()))
            })?;
            db_removed = true;
        }
    }

    let image_cache = crate::config::expand_tilde(&config.global_config.image_cache_path);
    let images = image_files(&image_cache);
    let mut images_removed = 0;
    for img in &images {
        if fs::remove_file(img).is_ok() {
            images_removed += 1;
        }
    }

    Ok(ResetOutcome {
        listings_deleted: listings,
        db_removed,
        images_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, GlobalConfig};
    use crate::db::Database;
    use crate::models::{Listing, ListingStatus};
    use chrono::Utc;

    fn test_config(dir: &Path) -> AppConfig {
        AppConfig {
            global_config: GlobalConfig {
                db_path: dir.join("test.db").display().to_string(),
                socket_path: dir.join("test.sock").display().to_string(),
                image_cache_path: dir.join("images").display().to_string(),
                ..GlobalConfig::default()
            },
            profiles: vec![],
        }
    }

    fn seed(config: &AppConfig) {
        let db = Database::open(config.db_path().to_str().unwrap()).unwrap();
        db.init().unwrap();
        db.migrate().unwrap();
        let listing = Listing {
            id: "reset-test-1".into(),
            profile_id: "p1".into(),
            source_id: "ebay".into(),
            title: "Test".into(),
            description: String::new(),
            price: Some(10.0),
            currency: "USD".into(),
            condition: None,
            url: "https://example.com/1".into(),
            image_urls: vec![],
            location: None,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            relevance_score: 80.0,
            status: ListingStatus::New,
            ai_evaluation: None,
        };
        db.upsert_listing(&listing).unwrap();
        db.update_source_state("ebay", Some(Utc::now()), 0, None)
            .unwrap();
    }

    #[test]
    fn full_reset_removes_db_and_images() {
        let dir = std::env::temp_dir().join(format!("scav-reset-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("images")).unwrap();
        std::fs::write(dir.join("images/x.jpg"), b"img").unwrap();
        let config = test_config(&dir);
        seed(&config);

        let p = plan(&config, &ResetOptions { listings_only: false });
        assert_eq!(p.listing_count, Some(1));
        assert_eq!(p.image_count, 1);

        let out = run(&config, &ResetOptions { listings_only: false }).unwrap();
        assert!(out.db_removed);
        assert_eq!(out.listings_deleted, 1);
        assert_eq!(out.images_removed, 1);
        assert!(!config.db_path().exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listings_only_reset_preserves_source_state() {
        let dir = std::env::temp_dir().join(format!("scav-reset-lo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = test_config(&dir);
        seed(&config);

        let out = run(&config, &ResetOptions { listings_only: true }).unwrap();
        assert_eq!(out.listings_deleted, 1);
        assert!(!out.db_removed);
        assert!(config.db_path().exists());

        let db = Database::open(config.db_path().to_str().unwrap()).unwrap();
        assert_eq!(db.get_active_listings(None, 100).unwrap().len(), 0);
        // Poll state survives, so the next daemon start is not a scrape storm.
        assert!(db.get_source_state("ebay").unwrap().is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
