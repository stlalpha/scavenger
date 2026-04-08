use scavenger::ai::prompts::{build_batch_prompt, build_escalation_prompt, build_prompt};
use scavenger::models::{KeywordEntry, Listing, Profile};

fn test_profile() -> Profile {
    Profile {
        id: "p1".to_string(),
        name: "vintage synthesizers".to_string(),
        keywords: vec![
            KeywordEntry::Single("moog".to_string()),
            KeywordEntry::Group(vec!["minimoog".to_string(), "model d".to_string()]),
        ],
        negative_keywords: vec!["broken".to_string(), "parts only".to_string()],
        sources: vec!["ebay".to_string()],
        price_min: Some(500.0),
        price_max: Some(5000.0),
        poll_interval_sec: 3600,
        alert_priority: "normal".to_string(),
        enabled: true,
        tags: vec![],
        escalation_keywords: vec!["rare".to_string(), "mint".to_string()],
        location_radius_mi: None,
    }
}

fn test_listing() -> Listing {
    Listing {
        id: "abc123".to_string(),
        profile_id: "p1".to_string(),
        source_id: "ebay-12345".to_string(),
        title: "Vintage Minimoog Model D Synthesizer".to_string(),
        description: "Original 1972 Minimoog in great condition".to_string(),
        price: Some(3500.0),
        currency: "USD".to_string(),
        condition: Some("used".to_string()),
        url: "https://ebay.com/item/12345".to_string(),
        image_urls: vec![],
        location: None,
        first_seen: "2026-01-01T00:00:00Z".to_string(),
        last_seen: "2026-01-01T00:00:00Z".to_string(),
        relevance_score: 85.0,
        status: "new".to_string(),
        ai_evaluation: None,
    }
}

#[test]
fn build_prompt_includes_keywords() {
    let (system, _user) = build_prompt(&test_profile(), &test_listing());
    assert!(system.contains("moog"));
    assert!(system.contains("minimoog or model d"));
}

#[test]
fn build_prompt_includes_price_range() {
    let (system, _user) = build_prompt(&test_profile(), &test_listing());
    assert!(system.contains("$500"));
    assert!(system.contains("$5000"));
}

#[test]
fn build_prompt_includes_negative_keywords() {
    let (system, _user) = build_prompt(&test_profile(), &test_listing());
    assert!(system.contains("broken"));
    assert!(system.contains("parts only"));
}

#[test]
fn build_prompt_user_has_listing_details() {
    let (_system, user) = build_prompt(&test_profile(), &test_listing());
    assert!(user.contains("Vintage Minimoog Model D Synthesizer"));
    assert!(user.contains("$3500.00"));
    assert!(user.contains("Original 1972 Minimoog"));
}

#[test]
fn build_prompt_no_price() {
    let mut listing = test_listing();
    listing.price = None;
    let (_system, user) = build_prompt(&test_profile(), &listing);
    assert!(user.contains("price not listed"));
}

#[test]
fn build_prompt_no_description() {
    let mut listing = test_listing();
    listing.description = String::new();
    let (_system, user) = build_prompt(&test_profile(), &listing);
    assert!(user.contains("(no description)"));
}

#[test]
fn build_batch_prompt_includes_all_ids() {
    let listings = vec![
        test_listing(),
        {
            let mut l = test_listing();
            l.id = "def456".to_string();
            l.title = "Another synth".to_string();
            l
        },
    ];
    let (_system, user) = build_batch_prompt(&test_profile(), &listings);
    assert!(user.contains("[abc123]"));
    assert!(user.contains("[def456]"));
}

#[test]
fn build_batch_prompt_system_has_keywords() {
    let (system, _user) = build_batch_prompt(&test_profile(), &[test_listing()]);
    assert!(system.contains("moog"));
    assert!(system.contains("minimoog or model d"));
    assert!(system.contains("JSON array"));
}

#[test]
fn build_escalation_prompt_includes_triggered_keywords() {
    let triggered = vec!["rare".to_string(), "mint".to_string()];
    let (system, _user) = build_escalation_prompt(&test_profile(), &test_listing(), &triggered);
    assert!(system.contains("\"rare\""));
    assert!(system.contains("\"mint\""));
}

#[test]
fn build_escalation_prompt_includes_source() {
    let triggered = vec!["rare".to_string()];
    let (_system, user) = build_escalation_prompt(&test_profile(), &test_listing(), &triggered);
    assert!(user.contains("Source: ebay-12345"));
}

#[test]
fn build_escalation_prompt_deep_analysis() {
    let triggered = vec!["rare".to_string()];
    let (system, _user) = build_escalation_prompt(&test_profile(), &test_listing(), &triggered);
    assert!(system.contains("INSIDER KNOWLEDGE"));
    assert!(system.contains("MARKET CONTEXT"));
    assert!(system.contains("CREDIBILITY CHECK"));
}

#[test]
fn price_range_up_to_max_only() {
    let mut profile = test_profile();
    profile.price_min = None;
    profile.price_max = Some(1000.0);
    let (system, _user) = build_prompt(&profile, &test_listing());
    assert!(system.contains("up to $1000"));
}

#[test]
fn price_range_min_and_above() {
    let mut profile = test_profile();
    profile.price_min = Some(200.0);
    profile.price_max = None;
    let (system, _user) = build_prompt(&profile, &test_listing());
    assert!(system.contains("$200 and above"));
}

#[test]
fn price_range_any_price() {
    let mut profile = test_profile();
    profile.price_min = None;
    profile.price_max = None;
    let (system, _user) = build_prompt(&profile, &test_listing());
    assert!(system.contains("any price"));
}
