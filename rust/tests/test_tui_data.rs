mod common {
    use scavenger::db::Database;
    use scavenger::models::{Listing, ListingStatus};

    pub fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory db");
        db.migrate().expect("migrate");
        db
    }

    pub fn make_listing(id: &str, profile_id: &str, status: ListingStatus) -> Listing {
        let now = chrono::Utc::now();
        Listing {
            id: id.to_string(),
            profile_id: profile_id.to_string(),
            source_id: "ebay".to_string(),
            title: format!("Test listing {id}"),
            description: String::new(),
            price: Some(99.0),
            currency: "USD".to_string(),
            condition: None,
            url: format!("https://example.com/{id}"),
            image_urls: vec![],
            location: None,
            first_seen: now,
            last_seen: now,
            relevance_score: 50.0,
            status,
            ai_evaluation: None,
        }
    }
}

#[test]
fn test_get_listings_returns_active() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("a1", "p1", ListingStatus::New))
        .unwrap();
    db.insert_listing(&common::make_listing("a2", "p1", ListingStatus::Seen))
        .unwrap();
    db.insert_listing(&common::make_listing("a3", "p1", ListingStatus::Dismissed))
        .unwrap();

    let dl = DataLayer::new(&db);
    let listings = dl.get_listings(Some("p1"), 100).unwrap();

    // Dismissed listings should be excluded
    assert_eq!(listings.len(), 2);
    let ids: Vec<&str> = listings.iter().map(|l| l.id.as_str()).collect();
    assert!(ids.contains(&"a1"));
    assert!(ids.contains(&"a2"));
    assert!(!ids.contains(&"a3"));
}

#[test]
fn test_get_listings_filters_by_profile() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("b1", "p1", ListingStatus::New))
        .unwrap();
    db.insert_listing(&common::make_listing("b2", "p2", ListingStatus::New))
        .unwrap();

    let dl = DataLayer::new(&db);
    let listings = dl.get_listings(Some("p1"), 100).unwrap();
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].id, "b1");
}

#[test]
fn test_profile_stats() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("c1", "p1", ListingStatus::New))
        .unwrap();
    db.insert_listing(&common::make_listing("c2", "p1", ListingStatus::New))
        .unwrap();
    db.insert_listing(&common::make_listing("c3", "p1", ListingStatus::Seen))
        .unwrap();
    db.insert_listing(&common::make_listing("c4", "p2", ListingStatus::New))
        .unwrap();

    let dl = DataLayer::new(&db);
    let stats = dl.get_profile_stats().unwrap();
    assert_eq!(stats.get("p1"), Some(&2));
    assert_eq!(stats.get("p2"), Some(&1));
}

#[test]
fn test_mark_status_save() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("d1", "p1", ListingStatus::New))
        .unwrap();

    let dl = DataLayer::new(&db);
    dl.mark_status("d1", "saved").unwrap();

    let listing = db.get_listing("d1").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Saved);
}

#[test]
fn test_mark_seen_only_from_new() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("e1", "p1", ListingStatus::New))
        .unwrap();
    db.insert_listing(&common::make_listing("e2", "p1", ListingStatus::Saved))
        .unwrap();

    let dl = DataLayer::new(&db);
    dl.mark_seen("e1").unwrap();
    dl.mark_seen("e2").unwrap();

    let l1 = db.get_listing("e1").unwrap().unwrap();
    let l2 = db.get_listing("e2").unwrap().unwrap();
    assert_eq!(l1.status, ListingStatus::Seen);
    // Saved should not be downgraded to seen
    assert_eq!(l2.status, ListingStatus::Saved);
}

#[test]
fn test_mark_status_snooze() {
    use scavenger::models::ListingStatus;
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    db.insert_listing(&common::make_listing("f1", "p1", ListingStatus::New))
        .unwrap();

    let dl = DataLayer::new(&db);
    dl.mark_status("f1", "snoozed").unwrap();

    let listing = db.get_listing("f1").unwrap().unwrap();
    assert_eq!(listing.status, ListingStatus::Snoozed);
}

#[test]
fn test_mark_nonexistent_listing() {
    use scavenger::tui::data::DataLayer;

    let db = common::setup_db();
    let dl = DataLayer::new(&db);
    // Should not error on nonexistent ID
    dl.mark_status("nonexistent", "saved").unwrap();
    dl.mark_seen("nonexistent").unwrap();
}
