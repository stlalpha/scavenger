use chrono::{Duration, Utc};
use scavenger::db::Database;
use scavenger::models::{Listing, ListingStatus};

fn test_db() -> Database {
    let db = Database::open(":memory:").unwrap();
    db.init().unwrap();
    db
}

fn make_listing(id: &str, profile_id: &str) -> Listing {
    let now = Utc::now();
    Listing {
        id: id.to_string(),
        profile_id: profile_id.to_string(),
        source_id: "ebay".to_string(),
        title: "Test Item".to_string(),
        description: "A test listing".to_string(),
        price: Some(29.99),
        currency: "USD".to_string(),
        condition: Some("used".to_string()),
        url: "https://example.com/item/1".to_string(),
        image_urls: vec!["https://img.example.com/1.jpg".to_string()],
        location: Some("Portland, OR".to_string()),
        first_seen: now,
        last_seen: now,
        relevance_score: 75.0,
        status: ListingStatus::New,
        ai_evaluation: None,
    }
}

#[test]
fn test_init_and_migrate() {
    let db = test_db();
    // migrate should be idempotent on a fresh schema (columns already exist)
    db.migrate().unwrap();
    db.migrate().unwrap();
}

#[test]
fn test_upsert_new_returns_true() {
    let db = test_db();
    let listing = make_listing("abc123", "guitars");
    assert!(db.upsert_listing(&listing).unwrap());
}

#[test]
fn test_upsert_duplicate_returns_false() {
    let db = test_db();
    let listing = make_listing("abc123", "guitars");
    assert!(db.upsert_listing(&listing).unwrap());
    assert!(!db.upsert_listing(&listing).unwrap());
}

#[test]
fn test_get_listing() {
    let db = test_db();
    let listing = make_listing("abc123", "guitars");
    db.upsert_listing(&listing).unwrap();

    let fetched = db.get_listing("abc123").unwrap().unwrap();
    assert_eq!(fetched.id, "abc123");
    assert_eq!(fetched.title, "Test Item");
    assert_eq!(fetched.image_urls, vec!["https://img.example.com/1.jpg"]);
    assert_eq!(fetched.price, Some(29.99));

    assert!(db.get_listing("nonexistent").unwrap().is_none());
}

#[test]
fn test_get_listings_with_filters() {
    let db = test_db();
    db.upsert_listing(&make_listing("a1", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a2", "guitars")).unwrap();
    db.upsert_listing(&make_listing("b1", "amps")).unwrap();

    // All listings
    let all = db.get_listings(None, None, 100).unwrap();
    assert_eq!(all.len(), 3);

    // Filter by profile
    let guitars = db.get_listings(None, Some("guitars"), 100).unwrap();
    assert_eq!(guitars.len(), 2);

    // Filter by status
    let new_ones = db.get_listings(Some("new"), None, 100).unwrap();
    assert_eq!(new_ones.len(), 3);

    // Limit
    let limited = db.get_listings(None, None, 2).unwrap();
    assert_eq!(limited.len(), 2);
}

#[test]
fn test_update_listing_status() {
    let db = test_db();
    db.upsert_listing(&make_listing("abc123", "guitars")).unwrap();

    db.update_listing_status("abc123", "seen").unwrap();
    let listing = db.get_listing("abc123").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Seen);

    // Invalid status should error
    assert!(db.update_listing_status("abc123", "bogus").is_err());
}

#[test]
fn test_price_history_on_insert() {
    let db = test_db();
    let listing = make_listing("abc123", "guitars");
    db.upsert_listing(&listing).unwrap();

    let history = db.get_price_history("abc123").unwrap();
    assert_eq!(history.len(), 1);
    assert!((history[0].price - 29.99).abs() < f64::EPSILON);
}

#[test]
fn test_price_history_on_change() {
    let db = test_db();
    let mut listing = make_listing("abc123", "guitars");
    db.upsert_listing(&listing).unwrap();

    // Re-insert with different price
    listing.price = Some(19.99);
    db.upsert_listing(&listing).unwrap();

    let history = db.get_price_history("abc123").unwrap();
    assert_eq!(history.len(), 2);
    assert!((history[0].price - 29.99).abs() < f64::EPSILON);
    assert!((history[1].price - 19.99).abs() < f64::EPSILON);
}

#[test]
fn test_price_history_no_change() {
    let db = test_db();
    let listing = make_listing("abc123", "guitars");
    db.upsert_listing(&listing).unwrap();
    // Same price, should not add another history entry
    db.upsert_listing(&listing).unwrap();

    let history = db.get_price_history("abc123").unwrap();
    assert_eq!(history.len(), 1);
}

#[test]
fn test_snooze_and_unsnooze() {
    let db = test_db();
    db.upsert_listing(&make_listing("abc123", "guitars")).unwrap();

    // Snooze until the past (so it unsnoozes immediately)
    let past = Utc::now() - Duration::hours(1);
    db.snooze_listing("abc123", past).unwrap();

    let listing = db.get_listing("abc123").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Snoozed);

    let count = db.unsnooze_expired().unwrap();
    assert_eq!(count, 1);

    let listing = db.get_listing("abc123").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Seen);
}

#[test]
fn test_snooze_future_not_unsnoozed() {
    let db = test_db();
    db.upsert_listing(&make_listing("abc123", "guitars")).unwrap();

    let future = Utc::now() + Duration::hours(24);
    db.snooze_listing("abc123", future).unwrap();

    let count = db.unsnooze_expired().unwrap();
    assert_eq!(count, 0);

    let listing = db.get_listing("abc123").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Snoozed);
}

#[test]
fn test_get_existing_ids() {
    let db = test_db();
    db.upsert_listing(&make_listing("a1", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a2", "guitars")).unwrap();

    let ids: Vec<String> = vec!["a1".into(), "a2".into(), "a3".into()];
    let existing = db.get_existing_ids(&ids).unwrap();
    assert_eq!(existing.len(), 2);
    assert!(existing.contains("a1"));
    assert!(existing.contains("a2"));
    assert!(!existing.contains("a3"));
}

#[test]
fn test_get_existing_ids_empty() {
    let db = test_db();
    let existing = db.get_existing_ids(&[]).unwrap();
    assert!(existing.is_empty());
}

#[test]
fn test_count_new_by_profile() {
    let db = test_db();
    db.upsert_listing(&make_listing("a1", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a2", "guitars")).unwrap();
    db.upsert_listing(&make_listing("b1", "amps")).unwrap();

    // Mark one as seen
    db.update_listing_status("a1", "seen").unwrap();

    let counts = db.count_new_by_profile().unwrap();
    assert_eq!(counts.get("guitars"), Some(&1));
    assert_eq!(counts.get("amps"), Some(&1));
}

#[test]
fn test_delete_profile_listings() {
    let db = test_db();
    db.upsert_listing(&make_listing("a1", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a2", "guitars")).unwrap();
    db.upsert_listing(&make_listing("b1", "amps")).unwrap();

    let deleted = db.delete_profile_listings("guitars").unwrap();
    assert_eq!(deleted, 2);

    let remaining = db.get_listings(None, None, 100).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].profile_id, "amps");

    // Price history should also be gone
    assert!(db.get_price_history("a1").unwrap().is_empty());
}

#[test]
fn test_source_state() {
    let db = test_db();
    let now = Utc::now();

    db.update_source_state("ebay", Some(now), 0, None).unwrap();

    let state = db.get_source_state("ebay").unwrap().unwrap();
    assert_eq!(state.plugin_id, "ebay");
    assert_eq!(state.consecutive_errors, 0);
    assert!(state.last_polled.is_some());

    // Update with errors
    db.update_source_state("ebay", Some(now), 3, None).unwrap();
    let state = db.get_source_state("ebay").unwrap().unwrap();
    assert_eq!(state.consecutive_errors, 3);

    assert!(db.get_source_state("nonexistent").unwrap().is_none());
}

#[test]
fn test_get_all_source_states() {
    let db = test_db();
    let now = Utc::now();

    db.update_source_state("ebay", Some(now), 0, None).unwrap();
    db.update_source_state("craigslist", Some(now), 1, None).unwrap();

    let states = db.get_all_source_states().unwrap();
    assert_eq!(states.len(), 2);
}

#[test]
fn test_get_most_recent_poll() {
    let db = test_db();
    assert!(db.get_most_recent_poll().unwrap().is_none());

    let t1 = Utc::now() - Duration::hours(2);
    let t2 = Utc::now();
    db.update_source_state("ebay", Some(t1), 0, None).unwrap();
    db.update_source_state("craigslist", Some(t2), 0, None).unwrap();

    let most_recent = db.get_most_recent_poll().unwrap().unwrap();
    // Should be close to t2
    let diff = (most_recent - t2).num_seconds().abs();
    assert!(diff < 2);
}

#[test]
fn test_get_active_listings() {
    let db = test_db();
    db.upsert_listing(&make_listing("a1", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a2", "guitars")).unwrap();
    db.upsert_listing(&make_listing("a3", "guitars")).unwrap();

    // Dismiss one
    db.update_listing_status("a1", "dismissed").unwrap();

    // Snooze one into the future
    let future = Utc::now() + Duration::hours(24);
    db.snooze_listing("a2", future).unwrap();

    let active = db.get_active_listings(Some("guitars"), 100).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "a3");
}
