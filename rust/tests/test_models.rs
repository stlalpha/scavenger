use chrono::Utc;
use scavenger::ai::models::AIEvaluation;
use scavenger::models::*;

#[test]
fn listing_serde_roundtrip() {
    let now = Utc::now();
    let listing = Listing {
        id: "abc123".into(),
        profile_id: "p1".into(),
        source_id: "ebay".into(),
        title: "Test Item".into(),
        description: "A thing".into(),
        price: Some(42.50),
        currency: "USD".into(),
        condition: Some("used".into()),
        url: "https://example.com/item".into(),
        image_urls: vec!["https://example.com/img.jpg".into()],
        location: Some("Portland, OR".into()),
        first_seen: now,
        last_seen: now,
        relevance_score: 85.0,
        status: ListingStatus::New,
        ai_evaluation: None,
    };

    let json = serde_json::to_string(&listing).unwrap();
    let roundtripped: Listing = serde_json::from_str(&json).unwrap();

    assert_eq!(roundtripped.id, listing.id);
    assert_eq!(roundtripped.title, listing.title);
    assert_eq!(roundtripped.price, listing.price);
    assert_eq!(roundtripped.status, ListingStatus::New);
    assert_eq!(roundtripped.image_urls.len(), 1);
}

#[test]
fn listing_status_serializes_lowercase() {
    let json = serde_json::to_string(&ListingStatus::Saved).unwrap();
    assert_eq!(json, "\"saved\"");

    let json = serde_json::to_string(&ListingStatus::Snoozed).unwrap();
    assert_eq!(json, "\"snoozed\"");

    let json = serde_json::to_string(&ListingStatus::Dismissed).unwrap();
    assert_eq!(json, "\"dismissed\"");
}

#[test]
fn keyword_group_deserialize_string() {
    let kw: KeywordGroup = serde_json::from_str("\"thing\"").unwrap();
    assert_eq!(kw, KeywordGroup::Single("thing".into()));
}

#[test]
fn keyword_group_deserialize_array() {
    let kw: KeywordGroup = serde_json::from_str("[\"variant1\", \"variant2\"]").unwrap();
    assert_eq!(
        kw,
        KeywordGroup::Any(vec!["variant1".into(), "variant2".into()])
    );
}

#[test]
fn profile_serde_roundtrip_toml() {
    let toml_str = r#"
id = "p1"
name = "Test Profile"
keywords = ["keyboard", ["mechanical", "mech"]]
sources = ["ebay"]
poll_interval_sec = 600
"#;

    let profile: Profile = toml::from_str(toml_str).unwrap();
    assert_eq!(profile.id, "p1");
    assert_eq!(profile.keywords.len(), 2);
    assert_eq!(profile.keywords[0], KeywordGroup::Single("keyboard".into()));
    assert_eq!(
        profile.keywords[1],
        KeywordGroup::Any(vec!["mechanical".into(), "mech".into()])
    );
    assert_eq!(profile.poll_interval_sec, 600);
    assert!(profile.enabled);
    assert_eq!(profile.alert_priority, AlertPriority::Normal);
}

#[test]
fn profile_validation_poll_interval_too_low() {
    let profile = Profile {
        id: "p1".into(),
        name: "Bad".into(),
        keywords: vec![],
        negative_keywords: vec![],
        sources: vec!["ebay".into()],
        price_min: None,
        price_max: None,
        poll_interval_sec: 10,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    };

    let result = profile.validate();
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("poll_interval_sec must be >= 30"));
}

#[test]
fn ai_evaluation_passthrough_defaults() {
    let eval = AIEvaluation::passthrough();
    assert!(eval.relevant);
    assert_eq!(eval.reason, "");
    assert!(eval.notable.is_none());
    assert!(!eval.escalate);
}
