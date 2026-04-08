use chrono::Utc;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::models::{content_hash, KeywordEntry, Listing, ListingStatus, Profile};

static PRICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$([0-9,]+(?:\.\d{2})?)").unwrap());

const BASE_URL: &str = "https://www.facebook.com/marketplace";

/// Selectors used to find item links in the marketplace DOM.
pub const ITEM_SELECTORS: &[&str] = &[
    "div[class] > a[href*='/marketplace/item/']",
    "a[href*='/marketplace/item/']",
];

/// Extract the first price from a text string.
pub fn extract_price(text: &str) -> Option<f64> {
    let cleaned = text.replace(',', "");
    PRICE_RE
        .captures(&cleaned)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok())
}

/// Normalize a Facebook marketplace item URL: strip query params, ensure absolute.
pub fn clean_fb_url(href: &str) -> String {
    let path = match href.find('?') {
        Some(idx) => &href[..idx],
        None => href,
    };
    if path.starts_with("http") {
        path.to_string()
    } else {
        format!("https://www.facebook.com{path}")
    }
}

/// Build the search URL for Facebook Marketplace.
pub fn build_search_url(keywords: &str, profile: &Profile) -> String {
    let encoded = urlencoding::encode(keywords);
    let mut url = format!(
        "{BASE_URL}/search/?query={encoded}&sortBy=creation_time_descend&exact=false"
    );
    if let Some(min) = profile.price_min {
        url.push_str(&format!("&minPrice={min:.0}"));
    }
    if let Some(max) = profile.price_max {
        url.push_str(&format!("&maxPrice={max:.0}"));
    }
    url
}

/// Check whether a page title or URL indicates a login wall.
pub fn is_login_page(title: &str, current_url: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("log in") || lower.contains("sign in") || current_url.contains("/login")
}

/// Parse card text lines into (price, title, location).
///
/// Facebook card layout is typically: price, title, location, distance.
/// Order varies. We find the first price-like line, treat the first
/// non-price line as the title, and look for a location in the rest.
pub fn parse_card_text(text: &str) -> (Option<f64>, String, Option<String>) {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut price: Option<f64> = None;
    let mut title = String::new();
    let mut location: Option<String> = None;

    for line in &lines {
        if PRICE_RE.is_match(line) && price.is_none() {
            price = extract_price(line);
        } else if title.is_empty() && !PRICE_RE.is_match(line) {
            title = line.to_string();
        }
    }

    if lines.len() > 2 {
        for line in &lines[2..] {
            if !PRICE_RE.is_match(line) && *line != title {
                location = Some(line.to_string());
                break;
            }
        }
    }

    (price, title, location)
}

/// Facebook Marketplace scraper plugin.
///
/// In the full port this drives a CDP browser session. Here we define
/// the data extraction logic that operates on already-fetched page data.
pub struct FacebookPlugin {
    pub location_id: Option<String>,
    pub radius_km: u32,
}

impl FacebookPlugin {
    pub fn new(location_id: Option<String>, radius_km: Option<u32>) -> Self {
        Self {
            location_id,
            radius_km: radius_km.unwrap_or(80),
        }
    }

    /// Join the first variant of each keyword group into a search string.
    /// Facebook has no OR syntax, so we pick the first variant per group.
    pub fn build_keywords(profile: &Profile) -> String {
        profile
            .keywords
            .iter()
            .map(|entry| match entry {
                KeywordEntry::Single(s) => s.as_str(),
                KeywordEntry::Any(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
            })
            .collect::<Vec<&str>>()
            .join(" ")
    }

    /// Build listings from raw scraped card data.
    ///
    /// Each card is (href, inner_text, optional image_src). This is the
    /// pure logic extracted from the browser interaction loop.
    pub fn parse_cards(
        cards: &[(String, String, Option<String>)],
        profile: &Profile,
    ) -> Vec<Listing> {
        let now = Utc::now();
        let mut seen_urls = HashSet::new();
        let mut listings = Vec::new();

        for (href, text, image_src) in cards {
            if !href.contains("/marketplace/item/") {
                continue;
            }

            let item_url = clean_fb_url(href);
            if !seen_urls.insert(item_url.clone()) {
                continue;
            }

            let (price, title, location) = parse_card_text(text);
            if title.is_empty() {
                continue;
            }

            let image_urls = match image_src {
                Some(src) if !src.starts_with("data:") => vec![src.clone()],
                _ => vec![],
            };

            listings.push(Listing {
                id: content_hash(&item_url),
                profile_id: profile.id.clone(),
                source_id: "facebook".to_string(),
                title,
                description: String::new(),
                price,
                currency: "USD".to_string(),
                condition: None,
                url: item_url,
                image_urls,
                location,
                first_seen: now,
                last_seen: now,
                relevance_score: 0.0,
                status: ListingStatus::New,
                ai_evaluation: None,
            });
        }

        listings
    }
}

#[async_trait::async_trait]
impl crate::plugins::Plugin for FacebookPlugin {
    fn plugin_id(&self) -> &str {
        "facebook"
    }

    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        // CDP-based scraping — requires browser connection.
        // For now, return empty. Full implementation needs chromiumoxide page lifecycle.
        tracing::warn!("Facebook plugin: CDP scraping not yet wired — returning empty");
        Ok(vec![])
    }

    async fn supports_geo(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AlertPriority, KeywordEntry, Profile};

    fn test_profile() -> Profile {
        Profile {
            id: "test".into(),
            name: "Test".into(),
            keywords: vec![
                KeywordEntry::Single("guitar".into()),
                KeywordEntry::Any(vec!["fender".into(), "gibson".into()]),
            ],
            negative_keywords: vec![],
            sources: vec!["facebook".into()],
            price_min: Some(100.0),
            price_max: Some(500.0),
            poll_interval_sec: 3600,
            alert_priority: AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        }
    }

    #[test]
    fn clean_url_strips_params() {
        assert_eq!(
            clean_fb_url("/marketplace/item/123456789/?ref=search"),
            "https://www.facebook.com/marketplace/item/123456789/"
        );
    }

    #[test]
    fn clean_url_absolute() {
        assert_eq!(
            clean_fb_url("https://www.facebook.com/marketplace/item/999"),
            "https://www.facebook.com/marketplace/item/999"
        );
    }

    #[test]
    fn clean_url_no_params() {
        assert_eq!(
            clean_fb_url("/marketplace/item/42"),
            "https://www.facebook.com/marketplace/item/42"
        );
    }

    #[test]
    fn extract_price_basic() {
        assert_eq!(extract_price("$1,299.00"), Some(1299.0));
        assert_eq!(extract_price("$50"), Some(50.0));
        assert_eq!(extract_price("Free"), None);
        assert_eq!(extract_price("$0"), Some(0.0));
    }

    #[test]
    fn login_detection() {
        assert!(is_login_page("Log in to Facebook", "https://www.facebook.com/login"));
        assert!(is_login_page("Sign In", "https://www.facebook.com/marketplace"));
        assert!(is_login_page("Marketplace", "https://www.facebook.com/login?next=..."));
        assert!(!is_login_page("Marketplace - Facebook", "https://www.facebook.com/marketplace/search"));
    }

    #[test]
    fn build_keywords_picks_first_variant() {
        let profile = test_profile();
        assert_eq!(FacebookPlugin::build_keywords(&profile), "guitar fender");
    }

    #[test]
    fn parse_card_text_typical() {
        let text = "$250\nFender Stratocaster\nPortland, OR\n15 miles away";
        let (price, title, location) = parse_card_text(text);
        assert_eq!(price, Some(250.0));
        assert_eq!(title, "Fender Stratocaster");
        assert_eq!(location.as_deref(), Some("Portland, OR"));
    }

    #[test]
    fn parse_card_text_no_price() {
        let text = "Free\nVintage Amp\nSeattle, WA";
        let (price, title, location) = parse_card_text(text);
        assert_eq!(price, None);
        assert_eq!(title, "Free");
        assert_eq!(location.as_deref(), Some("Seattle, WA"));
    }

    #[test]
    fn parse_cards_deduplicates() {
        let profile = test_profile();
        let cards = vec![
            (
                "/marketplace/item/123/?ref=search".into(),
                "$100\nItem One\nCity".into(),
                None,
            ),
            (
                "/marketplace/item/123/?ref=other".into(),
                "$100\nItem One\nCity".into(),
                None,
            ),
        ];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings.len(), 1);
    }

    #[test]
    fn parse_cards_skips_data_urls() {
        let profile = test_profile();
        let cards = vec![(
            "/marketplace/item/456".into(),
            "$200\nSomething".into(),
            Some("data:image/gif;base64,R0lGODlh".into()),
        )];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings.len(), 1);
        assert!(listings[0].image_urls.is_empty());
    }

    #[test]
    fn parse_cards_includes_real_image() {
        let profile = test_profile();
        let cards = vec![(
            "/marketplace/item/789".into(),
            "$300\nNice Guitar".into(),
            Some("https://scontent.xx.fbcdn.net/image.jpg".into()),
        )];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings[0].image_urls.len(), 1);
    }

    #[test]
    fn build_search_url_includes_price_bounds() {
        let profile = test_profile();
        let url = build_search_url("guitar fender", &profile);
        assert!(url.contains("minPrice=100"));
        assert!(url.contains("maxPrice=500"));
        assert!(url.contains("sortBy=creation_time_descend"));
    }
}
