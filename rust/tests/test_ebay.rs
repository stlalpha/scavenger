use scavenger::models::KeywordEntry;
use scavenger::plugins::ebay::{build_keywords, build_url, extract_price, strip_title_noise};

#[test]
fn url_basic_keywords() {
    let url = build_url("porsche 911", None, None);
    assert!(url.contains("_nkw=porsche%20911"));
    assert!(url.contains("_sop=10"));
    assert!(url.contains("_ipg=50"));
}

#[test]
fn url_with_price_range() {
    let url = build_url("monitor", Some(200.0), Some(800.0));
    assert!(url.contains("_udlo=200"));
    assert!(url.contains("_udhi=800"));
}

#[test]
fn url_min_price_only() {
    let url = build_url("laptop", Some(500.0), None);
    assert!(url.contains("_udlo=500"));
    assert!(!url.contains("_udhi"));
}

#[test]
fn url_max_price_only() {
    let url = build_url("laptop", None, Some(1500.0));
    assert!(!url.contains("_udlo"));
    assert!(url.contains("_udhi=1500"));
}

#[test]
fn price_usd() {
    assert_eq!(extract_price("$1,234.56"), Some(1234.56));
}

#[test]
fn price_gbp() {
    assert_eq!(extract_price("£500.00"), Some(500.0));
}

#[test]
fn price_euro() {
    assert_eq!(extract_price("€99.99"), Some(99.99));
}

#[test]
fn price_no_decimals() {
    assert_eq!(extract_price("$500"), Some(500.0));
}

#[test]
fn price_with_comma_thousands() {
    assert_eq!(extract_price("$12,500.00"), Some(12500.0));
}

#[test]
fn price_missing() {
    assert_eq!(extract_price("Free shipping"), None);
    assert_eq!(extract_price(""), None);
    assert_eq!(extract_price("Best Offer"), None);
}

#[test]
fn title_noise_stripped() {
    assert_eq!(
        strip_title_noise("NEW LISTING Porsche 911 Opens in a new window or tab"),
        "Porsche 911"
    );
}

#[test]
fn title_clean_passthrough() {
    assert_eq!(strip_title_noise("Porsche 911 Turbo"), "Porsche 911 Turbo");
}

#[test]
fn title_only_new_listing() {
    assert_eq!(
        strip_title_noise("NEW LISTING Great Widget"),
        "Great Widget"
    );
}

#[test]
fn keywords_simple() {
    let kw = vec![
        KeywordEntry::Single("porsche".into()),
        KeywordEntry::Single("911".into()),
    ];
    assert_eq!(build_keywords(&kw), "porsche 911");
}

#[test]
fn keywords_or_group() {
    let kw = vec![
        KeywordEntry::Single("porsche".into()),
        KeywordEntry::OrGroup(vec!["911".into(), "992".into()]),
    ];
    assert_eq!(build_keywords(&kw), "porsche (911,992)");
}

#[test]
fn keywords_single_variant_or_group() {
    let kw = vec![
        KeywordEntry::Single("bmw".into()),
        KeywordEntry::OrGroup(vec!["m3".into()]),
    ];
    assert_eq!(build_keywords(&kw), "bmw m3");
}

#[test]
fn keywords_multiple_or_groups() {
    let kw = vec![
        KeywordEntry::OrGroup(vec!["ford".into(), "chevy".into()]),
        KeywordEntry::OrGroup(vec!["truck".into(), "pickup".into()]),
    ];
    assert_eq!(build_keywords(&kw), "(ford,chevy) (truck,pickup)");
}
