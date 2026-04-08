use chrono::Utc;
use log::info;
use regex::Regex;
use std::sync::LazyLock;
use tokio::sync::Semaphore;

use crate::dedup::content_hash;
use crate::models::{KeywordEntry, Listing, ListingStatus, Profile};
use crate::plugins::craigslist_cities::{cities_for_zip_default, NATIONAL_METROS};
use crate::plugins::Plugin;

const MAX_CONCURRENT_CITIES: usize = 3;

static PRICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$([0-9,]+(?:\.[0-9]{2})?)").unwrap());

/// Extract a price from text like "$1,234.56" or "$50".
pub fn extract_price(text: &str) -> Option<f64> {
    PRICE_RE
        .captures(text)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().replace(',', "").parse::<f64>().ok())
}

/// Build the Craigslist search query string from profile keywords.
///
/// Craigslist doesn't support OR syntax, so we take the first variant
/// from each keyword group and join with spaces.
pub fn build_keywords(keywords: &[KeywordEntry]) -> String {
    keywords
        .iter()
        .filter_map(|entry| match entry {
            KeywordEntry::Single(s) => Some(s.as_str()),
            KeywordEntry::Variants(v) => v.first().map(|s| s.as_str()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the search URL for a given city and keyword query.
pub fn build_search_url(city: &str, keywords: &str) -> String {
    let encoded = urlencoding::encode(keywords);
    format!(
        "https://{}.craigslist.org/search/sss?query={}&sort=date&hasPic=1",
        city, encoded
    )
}

pub struct CraigslistPlugin {
    explicit_cities: Option<Vec<String>>,
    home_zip: Option<String>,
}

impl CraigslistPlugin {
    pub fn new(cities: Option<Vec<String>>, home_zip: Option<String>) -> Self {
        Self {
            explicit_cities: cities,
            home_zip,
        }
    }

    async fn get_cities(&self) -> Vec<String> {
        if let Some(ref cities) = self.explicit_cities {
            return cities.clone();
        }
        if let Some(ref zip) = self.home_zip {
            return cities_for_zip_default(zip).await;
        }
        NATIONAL_METROS.iter().map(|s| s.to_string()).collect()
    }

    /// Fetch listings from a single city. In the real implementation this
    /// drives Chrome via CDP; here we define the interface and URL construction.
    /// The actual DOM scraping requires a browser integration layer.
    async fn fetch_city(
        &self,
        city: &str,
        keywords: &str,
        profile: &Profile,
    ) -> Vec<Listing> {
        let url = build_search_url(city, keywords);
        info!("Craigslist {}: fetching {}", city, url);

        // Actual DOM scraping requires a browser/CDP module (not part of this unit).
        let _ = (city, keywords, profile, url);
        Vec::new()
    }
}

impl Plugin for CraigslistPlugin {
    fn plugin_id(&self) -> &str {
        "craigslist"
    }

    async fn fetch(
        &self,
        profile: &Profile,
    ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let keywords = build_keywords(&profile.keywords);
        let cities = self.get_cities().await;
        let sem = Semaphore::new(MAX_CONCURRENT_CITIES);

        let mut handles = Vec::new();

        // We can't move &self into spawned tasks, so collect city+keyword pairs
        // and use the semaphore for concurrency control.
        // In a real implementation each task would hold an Arc<BrowserPool>.
        for city in &cities {
            let permit = sem.acquire().await.unwrap();
            let listings = self.fetch_city(city, &keywords, profile).await;
            drop(permit);
            handles.push(listings);
        }

        Ok(handles.into_iter().flatten().collect())
    }

    fn supports_geo(&self) -> bool {
        true
    }
}

/// Parse a Listing from raw scraped fields. Used by the browser integration
/// layer to construct listings from extracted DOM data.
pub fn listing_from_scraped(
    item_url: &str,
    city: &str,
    title: &str,
    price_text: &str,
    image_urls: Vec<String>,
    profile_id: &str,
) -> Listing {
    let now = Utc::now();
    let url = if item_url.starts_with("http") {
        item_url.to_string()
    } else {
        format!("https://{}.craigslist.org{}", city, item_url)
    };

    Listing {
        id: content_hash(&url),
        profile_id: profile_id.to_string(),
        source_id: "craigslist".to_string(),
        title: title.to_string(),
        description: String::new(),
        price: extract_price(price_text),
        currency: "USD".to_string(),
        condition: None,
        url,
        image_urls,
        location: Some(city.to_string()),
        first_seen: now,
        last_seen: now,
        relevance_score: 0.0,
        status: ListingStatus::New,
        ai_evaluation: None,
    }
}

/// Extract a Craigslist image URL from the data-ids gallery attribute.
/// Format: "3:xxxxx_yyyyy,3:aaaaa_bbbbb,..."
/// Returns the first image as a 300x300 thumbnail URL.
pub fn image_from_data_ids(data_ids: &str) -> Option<String> {
    let first_id = data_ids
        .split(',')
        .next()?
        .split(':')
        .last()?
        .trim();
    if first_id.is_empty() {
        return None;
    }
    Some(format!("https://images.craigslist.org/{}_300x300.jpg", first_id))
}

/// Extract an image URL from a CSS background-image style attribute.
pub fn image_from_bg_style(style: &str) -> Option<String> {
    static BG_URL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"url\(["']?(https?://[^"')\s]+)"#).unwrap());
    BG_URL_RE
        .captures(style)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_extraction() {
        assert_eq!(extract_price("$1,234.56"), Some(1234.56));
        assert_eq!(extract_price("$50"), Some(50.0));
        assert_eq!(extract_price("free"), None);
        assert_eq!(extract_price("$0"), Some(0.0));
        assert_eq!(extract_price("asking $999 obo"), Some(999.0));
    }

    #[test]
    fn keyword_construction() {
        let keywords = vec![
            KeywordEntry::Single("guitar".to_string()),
            KeywordEntry::Variants(vec!["fender".to_string(), "gibson".to_string()]),
            KeywordEntry::Single("vintage".to_string()),
        ];
        assert_eq!(build_keywords(&keywords), "guitar fender vintage");
    }

    #[test]
    fn keyword_empty() {
        let keywords: Vec<KeywordEntry> = vec![];
        assert_eq!(build_keywords(&keywords), "");
    }

    #[test]
    fn search_url_construction() {
        let url = build_search_url("sfbay", "guitar fender");
        assert_eq!(
            url,
            "https://sfbay.craigslist.org/search/sss?query=guitar%20fender&sort=date&hasPic=1"
        );
    }

    #[test]
    fn data_ids_image_extraction() {
        assert_eq!(
            image_from_data_ids("3:00Z0Z_abc123,3:00Z0Z_def456"),
            Some("https://images.craigslist.org/00Z0Z_abc123_300x300.jpg".to_string())
        );
    }

    #[test]
    fn bg_style_image_extraction() {
        let style = "background-image: url('https://images.craigslist.org/abc.jpg');";
        assert_eq!(
            image_from_bg_style(style),
            Some("https://images.craigslist.org/abc.jpg".to_string())
        );
    }

    #[test]
    fn relative_url_gets_city_prefix() {
        let listing = listing_from_scraped(
            "/item/123.html",
            "sfbay",
            "Test Item",
            "$100",
            vec![],
            "profile1",
        );
        assert!(listing.url.starts_with("https://sfbay.craigslist.org/"));
        assert_eq!(listing.price, Some(100.0));
        assert_eq!(listing.source_id, "craigslist");
    }

    #[test]
    fn absolute_url_unchanged() {
        let listing = listing_from_scraped(
            "https://sfbay.craigslist.org/item/123.html",
            "sfbay",
            "Test",
            "",
            vec![],
            "p1",
        );
        assert_eq!(listing.url, "https://sfbay.craigslist.org/item/123.html");
        assert_eq!(listing.price, None);
    }
}
