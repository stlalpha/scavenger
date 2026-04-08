use scavenger::models::{AlertPriority, KeywordEntry, Profile};
use scavenger::plugins::facebook::*;

fn test_profile() -> Profile {
    Profile {
        id: "test".into(),
        name: "Test".into(),
        keywords: vec![
            KeywordEntry::Single("guitar".into()),
            KeywordEntry::Variants(vec!["fender".into(), "gibson".into()]),
        ],
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
fn url_cleaning_relative() {
    let url = clean_fb_url("/marketplace/item/12345/?ref=search&tracking=abc");
    assert_eq!(url, "https://www.facebook.com/marketplace/item/12345/");
}

#[test]
fn url_cleaning_absolute() {
    let url = clean_fb_url("https://www.facebook.com/marketplace/item/999?ref=x");
    assert_eq!(url, "https://www.facebook.com/marketplace/item/999");
}

#[test]
fn price_extraction() {
    assert_eq!(extract_price("$1,500.00"), Some(1500.0));
    assert_eq!(extract_price("$25"), Some(25.0));
    assert_eq!(extract_price("Price not listed"), None);
}

#[test]
fn login_detection_title() {
    assert!(is_login_page("Log in to Facebook", "https://www.facebook.com/"));
    assert!(is_login_page("Facebook - Sign In", "https://www.facebook.com/"));
}

#[test]
fn login_detection_url() {
    assert!(is_login_page("Marketplace", "https://www.facebook.com/login?next=/marketplace"));
}

#[test]
fn login_detection_negative() {
    assert!(!is_login_page(
        "Marketplace - Buy and Sell",
        "https://www.facebook.com/marketplace/search/?query=test"
    ));
}

#[test]
fn keywords_join() {
    let profile = test_profile();
    let kw = FacebookPlugin::build_keywords(&profile);
    assert_eq!(kw, "guitar fender");
}

#[test]
fn card_parsing_full() {
    let text = "$350\nVintage Fender\nPortland, OR\n5 miles";
    let (price, title, loc) = parse_card_text(text);
    assert_eq!(price, Some(350.0));
    assert_eq!(title, "Vintage Fender");
    assert_eq!(loc.as_deref(), Some("Portland, OR"));
}

#[test]
fn parse_cards_filters_non_marketplace() {
    let profile = test_profile();
    let cards = vec![(
        "/some/other/link".into(),
        "$100\nItem".into(),
        None,
    )];
    assert!(FacebookPlugin::parse_cards(&cards, &profile).is_empty());
}
