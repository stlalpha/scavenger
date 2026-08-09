use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use chrono::Utc;
use regex::Regex;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::dedup::content_hash;
use crate::models::{KeywordEntry, Listing, ListingStatus, Profile};
use crate::plugins::browser::{new_page, PageGuard};
use crate::plugins::craigslist_cities::{cities_for_zip_default, NATIONAL_METROS};
use crate::plugins::{Plugin, PluginError};

// Craigslist blocks per-city-subdomain but shares one IP reputation; two
// city fetches in flight is a gentler footprint than three while still
// finishing a fan-out promptly.
const MAX_CONCURRENT_CITIES: usize = 2;

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
            KeywordEntry::Any(v) => v.first().map(|s| s.as_str()),
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

/// Heuristic detection of a Craigslist block/captcha page, checked against
/// the page title and a snippet of body text after navigation.
pub fn is_blocked_page(title: &str, body_text: &str) -> bool {
    let title = title.to_lowercase();
    let body = body_text.to_lowercase();
    title.contains("blocked")
        || title.contains("403 forbidden")
        || title.contains("access denied")
        || body.contains("your access to craigslist has been blocked")
        || body.contains("unusual activity")
        || body.contains("network administrator")
        || body.contains("please complete the captcha")
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

    /// Fetch listings from a single city via CDP.
    ///
    /// Generic failures (page creation, navigation, selector misses) are
    /// swallowed and return `Ok(vec![])` so one bad city never aborts the
    /// others. `PluginError::BotDetected` propagates as `Err` so the caller
    /// can surface it distinctly.
    async fn fetch_city(
        city: &str,
        keywords: &str,
        profile: &Profile,
    ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let url = build_search_url(city, keywords);
        info!("Craigslist {city}: fetching {url}");

        let page = match new_page().await {
            Ok(p) => p,
            Err(e) => {
                warn!("Craigslist {city}: failed to open page: {e}");
                return Ok(Vec::new());
            }
        };
        let guard = PageGuard::new(page);

        let cur_page = match guard.page() {
            Ok(p) => p,
            Err(e) => {
                warn!("Craigslist {city}: {e}");
                let _ = guard.close().await;
                return Ok(Vec::new());
            }
        };

        if let Err(e) = cur_page.goto(&url).await {
            warn!("Craigslist {city}: navigation failed: {e}");
            let _ = guard.close().await;
            return Ok(Vec::new());
        }

        // Wait for page to settle after navigation before probing selectors.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let title: String = cur_page
            .evaluate("document.title")
            .await
            .ok()
            .and_then(|r| r.into_value().ok())
            .unwrap_or_default();
        let body_snippet: String = cur_page
            .evaluate("document.body ? document.body.innerText.slice(0, 500) : ''")
            .await
            .ok()
            .and_then(|r| r.into_value().ok())
            .unwrap_or_default();
        debug!("Craigslist {city}: page loaded, title={title:?}");

        if is_blocked_page(&title, &body_snippet) {
            warn!("Craigslist {city}: bot detection triggered (title={title:?})");
            let _ = guard.close().await;
            return Err(Box::new(PluginError::BotDetected {
                plugin_id: "craigslist".into(),
                url,
                message: format!("Craigslist blocked request for {city}"),
            }));
        }

        // Craigslist has two result formats depending on city/view
        let item_selectors = [".cl-search-result", ".result-row", "li.result-row"];
        let mut item_sel: Option<&str> = None;
        for sel in &item_selectors {
            let found: bool = cur_page
                .evaluate(format!(
                    "document.querySelector({}) !== null",
                    serde_json::to_string(sel).unwrap()
                ))
                .await
                .ok()
                .and_then(|r| r.into_value().ok())
                .unwrap_or(false);
            if found {
                item_sel = Some(sel);
                break;
            }
        }

        let Some(item_sel) = item_sel else {
            warn!("Craigslist {city}: no results found");
            let _ = guard.close().await;
            return Ok(Vec::new());
        };

        // Extract all listing data in a single JS call
        let js = format!(
            r#"
            (() => {{
                const items = document.querySelectorAll({item_sel});
                const results = [];
                for (const item of items) {{
                    // Title
                    const titleEl = item.querySelector(".posting-title .label")
                        || item.querySelector(".result-title")
                        || item.querySelector("a.titlestring");
                    // Link
                    const linkEl = item.querySelector("a.posting-title")
                        || item.querySelector("a.result-title")
                        || item.querySelector("a.titlestring");
                    if (!titleEl || !linkEl) continue;

                    const title = titleEl.innerText.trim();
                    const href = linkEl.getAttribute("href") || "";

                    // Price
                    const priceEl = item.querySelector(".priceinfo")
                        || item.querySelector(".result-price");
                    const priceText = priceEl ? priceEl.innerText : "";

                    // Image: try img src/data-src, then data-ids gallery, then bg style
                    let imageUrl = null;
                    for (const imgSel of ["img", ".swipe img", ".gallery img"]) {{
                        const img = item.querySelector(imgSel);
                        if (img) {{
                            for (const attr of ["src", "data-src"]) {{
                                const val = img.getAttribute(attr);
                                if (val && val.startsWith("http") && !val.includes("data:")) {{
                                    imageUrl = val;
                                    break;
                                }}
                            }}
                            if (imageUrl) break;
                        }}
                    }}
                    if (!imageUrl) {{
                        const gallery = item.querySelector("[data-ids]");
                        if (gallery) {{
                            const ids = gallery.getAttribute("data-ids") || "";
                            if (ids) {{
                                imageUrl = "__DATA_IDS__:" + ids;
                            }}
                        }}
                    }}
                    if (!imageUrl) {{
                        for (const bgSel of [".swipe", ".gallery", "[style*=background]"]) {{
                            const el = item.querySelector(bgSel);
                            if (el) {{
                                const style = el.getAttribute("style") || "";
                                imageUrl = "__BG_STYLE__:" + style;
                                break;
                            }}
                        }}
                    }}

                    results.push({{ title, href, priceText, imageUrl }});
                }}
                return results;
            }})()
            "#,
            item_sel = serde_json::to_string(item_sel).unwrap(),
        );

        let raw_items: Vec<CraigslistRawItem> = match cur_page.evaluate(js).await {
            Ok(val) => match val.into_value() {
                Ok(items) => items,
                Err(e) => {
                    warn!("Craigslist {city}: failed to parse items: {e}");
                    let _ = guard.close().await;
                    return Ok(Vec::new());
                }
            },
            Err(e) => {
                warn!("Craigslist {city}: JS eval failed: {e}");
                let _ = guard.close().await;
                return Ok(Vec::new());
            }
        };

        let mut listings = Vec::new();
        for raw in raw_items {
            // Resolve image URL from data-ids or bg-style markers
            let image_urls = if let Some(ref img) = raw.image_url {
                if let Some(ids) = img.strip_prefix("__DATA_IDS__:") {
                    image_from_data_ids(ids).into_iter().collect()
                } else if let Some(style) = img.strip_prefix("__BG_STYLE__:") {
                    image_from_bg_style(style).into_iter().collect()
                } else {
                    vec![img.clone()]
                }
            } else {
                vec![]
            };

            listings.push(listing_from_scraped(
                &raw.href,
                city,
                &raw.title,
                &raw.price_text,
                image_urls,
                &profile.id,
            ));
        }

        info!("Craigslist {city}: found {} listings for '{keywords}'", listings.len());
        let _ = guard.close().await;
        Ok(listings)
    }
}

/// Raw item extracted from Craigslist page JS evaluation.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CraigslistRawItem {
    title: String,
    href: String,
    #[serde(default)]
    price_text: String,
    image_url: Option<String>,
}

#[async_trait::async_trait]
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
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_CITIES));
        // Set once a city reports bot detection so queued-not-started cities
        // skip opening a tab against a site that just blocked us.
        let cancelled = Arc::new(AtomicBool::new(false));

        let mut handles = Vec::new();
        for city in cities {
            let sem = sem.clone();
            let kw = keywords.clone();
            let prof = profile.clone();
            let cancelled = cancelled.clone();
            handles.push(tokio::spawn(async move {
                if cancelled.load(Ordering::Relaxed) {
                    return Ok(Vec::new());
                }
                let _permit = sem.acquire().await.unwrap();
                if cancelled.load(Ordering::Relaxed) {
                    return Ok(Vec::new());
                }
                // 60s timeout per city
                tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    Self::fetch_city(&city, &kw, &prof),
                )
                .await
                .unwrap_or_else(|_| {
                    warn!("Craigslist {city}: timed out after 60s");
                    Ok(Vec::new())
                })
            }));
        }

        let mut per_city_results = Vec::with_capacity(handles.len());
        let mut remaining = handles.into_iter();
        let mut bot_detected = false;
        for handle in remaining.by_ref() {
            let city_result = match handle.await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Craigslist city task panicked: {e}");
                    Ok(Vec::new())
                }
            };
            let is_bot = matches!(
                &city_result,
                Err(e) if e
                    .downcast_ref::<PluginError>()
                    .is_some_and(|pe| matches!(pe, PluginError::BotDetected { .. }))
            );
            per_city_results.push(city_result);
            if is_bot {
                bot_detected = true;
                break;
            }
        }

        if bot_detected {
            cancelled.store(true, Ordering::Relaxed);
            // Abort every remaining city task so none of them open a tab
            // against a site that just blocked us; await so PageGuard drops
            // (page-close spawns) run before we return.
            for handle in remaining {
                handle.abort();
                let _ = handle.await;
            }
        }

        aggregate_city_results(per_city_results)
    }

    async fn supports_geo(&self) -> bool {
        true
    }
}

/// Aggregate per-city fetch results into a single listing set.
///
/// Generic errors are logged and their city's contribution dropped; any
/// `PluginError::BotDetected` short-circuits the whole aggregation to `Err`
/// so the caller can surface it distinctly.
fn aggregate_city_results(
    results: Vec<Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>>,
) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
    let mut all_listings = Vec::new();
    for result in results {
        match result {
            Ok(listings) => all_listings.extend(listings),
            Err(e) => {
                if let Some(pe) = e.downcast_ref::<PluginError>() {
                    if matches!(pe, PluginError::BotDetected { .. }) {
                        return Err(e);
                    }
                }
                warn!("Craigslist city task failed: {e}");
            }
        }
    }
    Ok(all_listings)
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
        .next_back()?
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
            KeywordEntry::Any(vec!["fender".to_string(), "gibson".to_string()]),
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
    fn blocked_page_detected_by_title() {
        assert!(is_blocked_page("Blocked", ""));
        assert!(is_blocked_page("403 Forbidden", ""));
        assert!(is_blocked_page("Access Denied", ""));
    }

    #[test]
    fn blocked_page_detected_by_body_text() {
        assert!(is_blocked_page(
            "craigslist",
            "Your access to craigslist has been blocked"
        ));
        assert!(is_blocked_page("craigslist", "unusual activity detected"));
        assert!(is_blocked_page(
            "craigslist",
            "please complete the captcha to continue"
        ));
    }

    #[test]
    fn normal_page_not_blocked() {
        assert!(!is_blocked_page(
            "guitar for sale - craigslist",
            "Fender Stratocaster, great condition, $500"
        ));
        assert!(!is_blocked_page("", ""));
    }

    fn dummy_listing(url: &str) -> Listing {
        listing_from_scraped(url, "sfbay", "Test", "$10", vec![], "p1")
    }

    #[test]
    fn aggregate_collects_oks_and_logs_generic_errors() {
        let results: Vec<Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>> = vec![
            Ok(vec![dummy_listing("https://sfbay.craigslist.org/a")]),
            Err(Box::new(PluginError::Other("navigation failed".into()))),
            Ok(vec![dummy_listing("https://sfbay.craigslist.org/b")]),
        ];
        let aggregated = aggregate_city_results(results).unwrap();
        assert_eq!(aggregated.len(), 2);
    }

    #[test]
    fn aggregate_short_circuits_on_bot_detected() {
        let results: Vec<Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>> = vec![
            Ok(vec![dummy_listing("https://sfbay.craigslist.org/a")]),
            Err(Box::new(PluginError::BotDetected {
                plugin_id: "craigslist".into(),
                url: "https://sfbay.craigslist.org/search/sss".into(),
                message: "blocked".into(),
            })),
            Ok(vec![dummy_listing("https://sfbay.craigslist.org/c")]),
        ];
        let err = aggregate_city_results(results).unwrap_err();
        assert!(err.downcast_ref::<PluginError>().is_some_and(|pe| matches!(
            pe,
            PluginError::BotDetected { .. }
        )));
    }

    #[test]
    fn aggregate_all_ok_no_errors() {
        let results: Vec<Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>>> = vec![
            Ok(vec![dummy_listing("https://sfbay.craigslist.org/a")]),
            Ok(Vec::new()),
        ];
        let aggregated = aggregate_city_results(results).unwrap();
        assert_eq!(aggregated.len(), 1);
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
