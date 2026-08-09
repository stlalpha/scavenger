//! Facebook card parsing against real Marketplace pages captured 2026-08-08
//! from a logged-in session: `fb_cards_newest.json` (sortBy=creation_time_descend,
//! the daemon's actual search URL — heavy "Just listed" badge coverage) and
//! `fb_cards_relevance.json` (default sort — discounted double-price and
//! ships-to-you cards).

use scavenger::models::{AlertPriority, KeywordEntry, Profile};
use scavenger::plugins::facebook::{FacebookPlugin, FbRawCard};

fn load(name: &str) -> Vec<FbRawCard> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn thinkpad_profile() -> Profile {
    Profile {
        id: "fixture".into(),
        name: "Fixture".into(),
        keywords: vec![KeywordEntry::Single("thinkpad".into())],
        negative_keywords: vec![],
        sources: vec!["facebook".into()],
        price_min: None,
        price_max: None,
        poll_interval_sec: 3600,
        alert_priority: AlertPriority::Normal,
        enabled: true,
        tags: vec![],
        escalation_keywords: vec![],
        location_radius_mi: None,
    }
}

#[test]
fn every_captured_card_parses_with_a_real_title() {
    for fixture in ["fb_cards_newest.json", "fb_cards_relevance.json"] {
        let cards = load(fixture);
        let listings = FacebookPlugin::parse_cards(&cards, &thinkpad_profile());
        // Every card in both captures has an aria-label, so none may be lost
        // to parsing (dedup by URL is the only permitted reduction).
        let unique_urls: std::collections::HashSet<_> =
            cards.iter().map(|c| c.href.split('?').next().unwrap()).collect();
        assert_eq!(listings.len(), unique_urls.len(), "{fixture}: lost cards to parsing");
        for l in &listings {
            assert!(!l.title.is_empty(), "{fixture}: empty title for {}", l.url);
            assert_ne!(l.title, "Just listed", "{fixture}: badge leaked into title");
            assert!(!l.title.starts_with('$'), "{fixture}: price leaked into title: {}", l.title);
        }
    }
}

#[test]
fn badge_card_parses_to_its_real_title() {
    let listings = FacebookPlugin::parse_cards(&load("fb_cards_newest.json"), &thinkpad_profile());
    let t450s = listings
        .iter()
        .find(|l| l.title.contains("ThinkPad T450s"))
        .expect("T450s card missing");
    assert_eq!(t450s.price, Some(140.0));
    assert_eq!(t450s.location.as_deref(), Some("St Peters, MO"));
}

#[test]
fn discounted_card_takes_current_price_not_strikethrough() {
    let listings =
        FacebookPlugin::parse_cards(&load("fb_cards_relevance.json"), &thinkpad_profile());
    let x380 = listings
        .iter()
        .find(|l| l.title.contains("Thinkpad X380"))
        .expect("X380 card missing");
    assert_eq!(x380.price, Some(200.0));
    assert_eq!(x380.location.as_deref(), Some("O'Fallon, MO"));
}

#[test]
fn shipped_card_has_no_location() {
    let listings =
        FacebookPlugin::parse_cards(&load("fb_cards_relevance.json"), &thinkpad_profile());
    let shipped = listings
        .iter()
        .find(|l| l.title.contains("X1 Carbon (Gen 9)"))
        .expect("shipped X1 Carbon missing");
    assert_eq!(shipped.price, Some(500.0));
    assert_eq!(shipped.location, None);
}
