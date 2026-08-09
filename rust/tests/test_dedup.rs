use scavenger::dedup::{content_hash, normalize_url};

#[test]
fn normalize_strips_utm_params() {
    let url = "https://example.com/item?id=1&utm_source=google&utm_medium=cpc&utm_campaign=summer";
    let norm = normalize_url(url);
    assert!(norm.contains("id=1"));
    assert!(!norm.contains("utm_"));
}

#[test]
fn normalize_strips_ebay_tracking() {
    let url = "https://www.ebay.com/itm/123456?ssPageName=STRK&_trkparms=algo&mkevt=1&mkcid=1&mkrid=abc&campid=5&toolid=10";
    let norm = normalize_url(url);
    for param in &["ssPageName", "_trkparms", "mkevt", "mkcid", "mkrid", "campid", "toolid"] {
        assert!(!norm.contains(param), "should have stripped {}", param);
    }
}

#[test]
fn normalize_removes_fragment() {
    let url = "https://example.com/page?q=test#section2";
    let norm = normalize_url(url);
    assert!(!norm.contains('#'));
    assert!(norm.contains("q=test"));
}

#[test]
fn content_hash_produces_stable_hex() {
    let url = "https://example.com/item?id=42";
    let h1 = content_hash(url);
    let h2 = content_hash(url);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn same_url_different_tracking_same_hash() {
    let a = "https://example.com/item?id=42&utm_source=email&ref=homepage";
    let b = "https://example.com/item?id=42&utm_source=twitter&ref=sidebar";
    assert_eq!(content_hash(a), content_hash(b));
}

#[test]
fn different_urls_different_hash() {
    let a = "https://example.com/item?id=1";
    let b = "https://example.com/item?id=2";
    assert_ne!(content_hash(a), content_hash(b));
}
