use scavenger::plugins::images::*;

#[test]
fn ebay_regex_matches_and_normalizes() {
    let html = r#"{"url":"https://i.ebayimg.com/images/g/AbCdEf/s-l1600.jpg"}"#;
    let images = extract_ebay(html);
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0],
        "https://i.ebayimg.com/images/g/AbCdEf/s-l500.jpg"
    );
}

#[test]
fn ebay_deduplicates_same_image_different_sizes() {
    let html = r#"
        "https://i.ebayimg.com/images/g/SAME/s-l1600.jpg"
        "https://i.ebayimg.com/images/g/SAME/s-l300.jpg"
        "https://i.ebayimg.com/images/g/SAME/s-l96.jpg"
    "#;
    let images = extract_ebay(html);
    assert_eq!(images.len(), 1);
}

#[test]
fn craigslist_regex_matches() {
    let html = "https://images.craigslist.org/ab12_300x300.jpg more text";
    let images = extract_craigslist(html);
    assert_eq!(images.len(), 1);
    assert!(images[0].ends_with("_600x450.jpg"));
}

#[test]
fn craigslist_normalizes_resolution() {
    let html = "https://images.craigslist.org/img_50x50.jpg";
    let images = extract_craigslist(html);
    assert_eq!(images[0], "https://images.craigslist.org/img_600x450.jpg");
}

#[test]
fn facebook_scontent_regex() {
    let html = r#""https://scontent.fseattle1-1.fna.fbcdn.net/v/photo.jpg""#;
    let images = extract_facebook(html);
    assert_eq!(images.len(), 1);
    assert!(images[0].starts_with("https://scontent"));
}

#[test]
fn facebook_avatar_filtering() {
    let html = r#"
        "https://scontent.xx.fbcdn.net/v/p50x50/avatar.jpg"
        "https://scontent.xx.fbcdn.net/v/p36x36/mini.jpg"
        "https://scontent.xx.fbcdn.net/v/full_size_photo.jpg"
    "#;
    let images = extract_facebook(html);
    assert_eq!(images.len(), 1);
    assert!(images[0].contains("full_size_photo"));
}

#[test]
fn facebook_emoji_filtered() {
    let html = r#""https://scontent.xx.fbcdn.net/emoji/123.png""#;
    let images = extract_facebook(html);
    assert!(images.is_empty());
}

#[test]
fn facebook_max_10_images() {
    let mut html = String::new();
    for i in 0..15 {
        html.push_str(&format!(
            r#""https://scontent.xx.fbcdn.net/v/photo_{i}.jpg" "#
        ));
    }
    let images = extract_facebook(&html);
    assert_eq!(images.len(), 10);
}

#[test]
fn no_match_returns_empty() {
    let html = "just some random text with no URLs";
    assert!(extract_ebay(html).is_empty());
    assert!(extract_craigslist(html).is_empty());
    assert!(extract_facebook(html).is_empty());
}
