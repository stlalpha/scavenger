use scavenger::models::KeywordEntry;
use scavenger::plugins::craigslist::{build_keywords, extract_price, listing_from_scraped};
use scavenger::plugins::craigslist_cities::{haversine, CL_CITIES, NATIONAL_METROS};

#[test]
fn haversine_ny_to_la() {
    let dist = haversine(40.7128, -74.0060, 34.0522, -118.2437);
    assert!(
        (dist - 2451.0).abs() < 20.0,
        "NY to LA should be ~2451 miles, got {}",
        dist
    );
}

#[test]
fn haversine_seattle_to_sfbay() {
    let dist = haversine(47.6062, -122.3321, 37.7749, -122.4194);
    assert!(
        (dist - 679.0).abs() < 20.0,
        "Seattle to SF should be ~679 miles, got {}",
        dist
    );
}

#[test]
fn zip_near_stlouis_gets_stlouis_first() {
    // St. Louis coords: 38.6270, -90.1994
    // Find the nearest city to those coords manually
    let stl_lat = 38.6270;
    let stl_lon = -90.1994;

    let mut ranked: Vec<(&str, f64)> = CL_CITIES
        .iter()
        .map(|c| (c.subdomain, haversine(stl_lat, stl_lon, c.lat, c.lon)))
        .collect();
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    assert_eq!(ranked[0].0, "stlouis");
    assert!(ranked[0].1 < 1.0, "distance to stlouis should be ~0");
}

#[test]
fn price_regex_various_formats() {
    assert_eq!(extract_price("$1,234"), Some(1234.0));
    assert_eq!(extract_price("$50.00"), Some(50.0));
    assert_eq!(extract_price("$0"), Some(0.0));
    assert_eq!(extract_price("free stuff"), None);
    assert_eq!(extract_price("asking $2,500 obo"), Some(2500.0));
    assert_eq!(extract_price(""), None);
}

#[test]
fn keyword_construction_mixed() {
    let kw = vec![
        KeywordEntry::Single("bicycle".to_string()),
        KeywordEntry::Variants(vec!["trek".to_string(), "specialized".to_string()]),
    ];
    assert_eq!(build_keywords(&kw), "bicycle trek");
}

#[test]
fn keyword_single_only() {
    let kw = vec![
        KeywordEntry::Single("laptop".to_string()),
        KeywordEntry::Single("thinkpad".to_string()),
    ];
    assert_eq!(build_keywords(&kw), "laptop thinkpad");
}

#[test]
fn national_metros_fallback() {
    assert_eq!(NATIONAL_METROS.len(), 6);
    assert!(NATIONAL_METROS.contains(&"newyork"));
    assert!(NATIONAL_METROS.contains(&"sfbay"));
}

#[test]
fn listing_construction_sets_hash_id() {
    let a = listing_from_scraped(
        "https://sfbay.craigslist.org/item/1",
        "sfbay",
        "Test",
        "$100",
        vec![],
        "p1",
    );
    let b = listing_from_scraped(
        "https://sfbay.craigslist.org/item/1",
        "sfbay",
        "Different Title",
        "$200",
        vec![],
        "p2",
    );
    // Same URL should produce the same content hash ID
    assert_eq!(a.id, b.id);
    assert!(!a.id.is_empty());
}

#[test]
fn listing_different_urls_different_ids() {
    let a = listing_from_scraped(
        "https://sfbay.craigslist.org/item/1",
        "sfbay",
        "A",
        "",
        vec![],
        "p1",
    );
    let b = listing_from_scraped(
        "https://sfbay.craigslist.org/item/2",
        "sfbay",
        "B",
        "",
        vec![],
        "p1",
    );
    assert_ne!(a.id, b.id);
}
