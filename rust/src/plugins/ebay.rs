use chrono::Utc;
use chromiumoxide::Page;
use regex::Regex;
use std::sync::LazyLock;
use tracing::{debug, info, warn};

use crate::dedup::content_hash;
use crate::models::{KeywordEntry, Listing, ListingStatus, Profile};
use crate::plugins::{Plugin, PluginError};

const SEARCH_URL: &str = "https://www.ebay.com/sch/i.html";

static PRICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\$£€]([0-9]+(?:\.[0-9]{2})?)").unwrap());

/// CSS selectors tried in order — eBay restructures periodically.
const ITEM_SELECTORS: &[&str] = &[".s-card", ".s-item", "li.s-item", "li[data-viewport]"];

const TITLE_SELECTORS: &[&str] = &[".s-card__title", ".s-item__title"];
const LINK_SELECTORS: &[&str] = &[".s-card__link", ".s-item__link", "a[href*='/itm/']"];
const PRICE_SELECTORS: &[&str] = &[".s-card__price", ".s-item__price"];
const IMAGE_SELECTORS: &[&str] = &[
    ".s-card__image img",
    "img[src*=ebayimg]",
    ".s-item__image-img",
];
const LOCATION_SELECTORS: &[&str] = &[".s-card__location", ".s-item__location"];

/// Noise strings stripped from listing titles.
const TITLE_NOISE: &[&str] = &["NEW LISTING", "Opens in a new window or tab"];

/// Extract a price from text like "$1,234.56" or "£500.00".
/// Strips commas before matching.
pub fn extract_price(text: &str) -> Option<f64> {
    let cleaned = text.replace(',', "");
    PRICE_RE
        .captures(&cleaned)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok())
}

/// Strip decorative noise from an eBay listing title.
pub fn strip_title_noise(title: &str) -> String {
    let mut result = title.to_string();
    for noise in TITLE_NOISE {
        result = result.replace(noise, "");
    }
    // Collapse whitespace left behind.
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Build eBay keyword string from profile keywords.
/// Single strings pass through; OR-groups become `(a,b,c)`.
pub fn build_keywords(keywords: &[KeywordEntry]) -> String {
    keywords
        .iter()
        .map(|entry| match entry {
            KeywordEntry::Single(s) => s.clone(),
            KeywordEntry::Any(variants) => {
                if variants.len() == 1 {
                    variants[0].clone()
                } else {
                    format!("({})", variants.join(","))
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the full eBay search URL from keywords and optional price filters.
pub fn build_url(keywords: &str, price_min: Option<f64>, price_max: Option<f64>) -> String {
    let encoded = urlencoding::encode(keywords);
    let mut url = format!("{SEARCH_URL}?_nkw={encoded}&_sop=10&_ipg=50");
    if let Some(min) = price_min {
        url.push_str(&format!("&_udlo={:.0}", min));
    }
    if let Some(max) = price_max {
        url.push_str(&format!("&_udhi={:.0}", max));
    }
    url
}

pub struct EbayPlugin;

#[async_trait::async_trait]
impl Plugin for EbayPlugin {
    fn plugin_id(&self) -> &str {
        "ebay"
    }

    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let keywords = build_keywords(&profile.keywords);
        match self.scrape(&keywords, profile).await {
            Ok(listings) => Ok(listings),
            Err(e) => {
                // Check if it's a bot detection — propagate those
                if let Some(pe) = e.downcast_ref::<PluginError>() {
                    if matches!(pe, PluginError::BotDetected { .. }) {
                        return Err(e);
                    }
                }
                warn!("eBay fetch failed: {e}");
                Ok(vec![])
            }
        }
    }

    async fn supports_geo(&self) -> bool {
        false
    }
}

impl EbayPlugin {
    /// Scrape eBay search results using a CDP page.
    async fn scrape(
        &self,
        keywords: &str,
        profile: &Profile,
    ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        // In production, `page` comes from browser.rs; here we define the logic
        // that operates on a Page handle. The actual browser connection is
        // managed externally.
        let _ = (keywords, profile);
        Err(Box::new(PluginError::Browser(
            "browser integration requires runtime CDP connection".into(),
        )))
    }

    /// Core scraping logic operating on a live CDP page.
    /// Separated from `scrape` so browser wiring can inject the page.
    pub async fn scrape_with_page(
        &self,
        page: &Page,
        keywords: &str,
        profile: &Profile,
    ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let url = build_url(keywords, profile.price_min, profile.price_max);

        page.goto(&url)
            .await
            .map_err(|e| PluginError::Navigation(e.to_string()))?;

        let title: String = page
            .evaluate("document.title")
            .await
            .map_err(|e| PluginError::Browser(e.to_string()))?
            .into_value()
            .map_err(|e| PluginError::Browser(e.to_string()))?;
        debug!("eBay: page loaded, title={title:?}");

        if title.contains("Pardon Our Interruption") {
            return Err(Box::new(PluginError::BotDetected {
                plugin_id: "ebay".into(),
                url: "https://www.ebay.com".into(),
                message: "eBay bot detection -- needs manual CAPTCHA".into(),
            }));
        }

        // Find the first working item selector.
        let mut item_selector: Option<&str> = None;
        for sel in ITEM_SELECTORS {
            let found: bool = page
                .evaluate(format!(
                    "document.querySelector({}) !== null",
                    serde_json::to_string(sel).unwrap()
                ))
                .await
                .ok()
                .and_then(|r| r.into_value().ok())
                .unwrap_or(false);
            if found {
                item_selector = Some(sel);
                break;
            }
        }

        let Some(item_sel) = item_selector else {
            warn!("eBay: no listing elements found on page (title: {title:?})");
            return Ok(vec![]);
        };

        // Extract listing data in a single JS evaluation to minimize round-trips.
        let js = format!(
            r#"
            (() => {{
                const items = document.querySelectorAll({item_sel});
                const titleSels = {title_sels};
                const linkSels = {link_sels};
                const priceSels = {price_sels};
                const imgSels = {img_sels};
                const locSels = {loc_sels};

                function qFirst(parent, sels) {{
                    for (const s of sels) {{
                        const el = parent.querySelector(s);
                        if (el) return el;
                    }}
                    return null;
                }}

                const results = [];
                for (const item of items) {{
                    const titleEl = qFirst(item, titleSels);
                    const linkEl = qFirst(item, linkSels);
                    const priceEl = qFirst(item, priceSels);
                    const imgEl = qFirst(item, imgSels);
                    const locEl = qFirst(item, locSels);

                    if (!titleEl || !linkEl) continue;

                    const href = linkEl.getAttribute("href") || "";
                    let imgUrl = null;
                    if (imgEl) {{
                        for (const attr of ["src", "data-src", "srcset"]) {{
                            const raw = imgEl.getAttribute(attr);
                            if (!raw || raw.startsWith("data:")) continue;
                            const candidate = raw.split(",")[0].split(" ")[0].trim();
                            if (candidate.startsWith("http")) {{
                                imgUrl = candidate;
                                break;
                            }}
                        }}
                    }}

                    results.push({{
                        title: titleEl.innerText.trim(),
                        href: href.split("?")[0],
                        price: priceEl ? priceEl.innerText : "",
                        image: imgUrl,
                        location: locEl ? locEl.innerText.trim() : null,
                    }});
                }}
                return results;
            }})()
            "#,
            item_sel = serde_json::to_string(item_sel).unwrap(),
            title_sels = serde_json::to_string(&TITLE_SELECTORS).unwrap(),
            link_sels = serde_json::to_string(&LINK_SELECTORS).unwrap(),
            price_sels = serde_json::to_string(&PRICE_SELECTORS).unwrap(),
            img_sels = serde_json::to_string(&IMAGE_SELECTORS).unwrap(),
            loc_sels = serde_json::to_string(&LOCATION_SELECTORS).unwrap(),
        );

        let raw_items: Vec<RawItem> = page
            .evaluate(js)
            .await
            .map_err(|e| PluginError::Browser(e.to_string()))?
            .into_value()
            .map_err(|e| PluginError::Browser(e.to_string()))?;

        let now = Utc::now();
        let mut listings = Vec::new();

        for raw in raw_items {
            let title = strip_title_noise(&raw.title);
            if title.contains("Shop on eBay") || raw.href.is_empty() {
                continue;
            }

            let price = extract_price(&raw.price);
            let image_urls = raw.image.into_iter().collect();

            listings.push(Listing {
                id: content_hash(&raw.href),
                profile_id: profile.id.clone(),
                source_id: "ebay".into(),
                title,
                description: String::new(),
                price,
                currency: "USD".into(),
                condition: None,
                url: raw.href,
                image_urls,
                location: raw.location,
                first_seen: now,
                last_seen: now,
                relevance_score: 0.0,
                status: ListingStatus::New,
                ai_evaluation: None,
            });
        }

        info!("eBay: found {} listings for '{keywords}'", listings.len());
        Ok(listings)
    }
}

/// Raw item extracted from page JS evaluation.
#[derive(Debug, serde::Deserialize)]
struct RawItem {
    title: String,
    href: String,
    #[serde(default)]
    price: String,
    image: Option<String>,
    location: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_price_usd() {
        assert_eq!(extract_price("$1,234.56"), Some(1234.56));
    }

    #[test]
    fn test_extract_price_gbp() {
        assert_eq!(extract_price("£500.00"), Some(500.0));
    }

    #[test]
    fn test_extract_price_euro() {
        assert_eq!(extract_price("€99.99"), Some(99.99));
    }

    #[test]
    fn test_extract_price_no_cents() {
        assert_eq!(extract_price("$500"), Some(500.0));
    }

    #[test]
    fn test_extract_price_none() {
        assert_eq!(extract_price("Free shipping"), None);
        assert_eq!(extract_price(""), None);
    }

    #[test]
    fn test_strip_title_noise() {
        assert_eq!(
            strip_title_noise("NEW LISTING Porsche 911 Opens in a new window or tab"),
            "Porsche 911"
        );
    }

    #[test]
    fn test_strip_title_noise_clean() {
        assert_eq!(strip_title_noise("Porsche 911 Turbo"), "Porsche 911 Turbo");
    }

    #[test]
    fn test_build_keywords_simple() {
        let kw = vec![
            KeywordEntry::Single("porsche".into()),
            KeywordEntry::Single("911".into()),
        ];
        assert_eq!(build_keywords(&kw), "porsche 911");
    }

    #[test]
    fn test_build_keywords_or_group() {
        let kw = vec![
            KeywordEntry::Single("porsche".into()),
            KeywordEntry::Any(vec!["911".into(), "992".into()]),
        ];
        assert_eq!(build_keywords(&kw), "porsche (911,992)");
    }

    #[test]
    fn test_build_keywords_single_item_or_group() {
        let kw = vec![
            KeywordEntry::Single("porsche".into()),
            KeywordEntry::Any(vec!["911".into()]),
        ];
        assert_eq!(build_keywords(&kw), "porsche 911");
    }

    #[test]
    fn test_build_url_basic() {
        let url = build_url("porsche 911", None, None);
        assert!(url.starts_with(SEARCH_URL));
        assert!(url.contains("_nkw=porsche%20911"));
        assert!(url.contains("_sop=10"));
        assert!(url.contains("_ipg=50"));
        assert!(!url.contains("_udlo"));
        assert!(!url.contains("_udhi"));
    }

    #[test]
    fn test_build_url_with_price_range() {
        let url = build_url("thing", Some(100.0), Some(5000.0));
        assert!(url.contains("_udlo=100"));
        assert!(url.contains("_udhi=5000"));
    }

    #[test]
    fn test_build_url_with_min_only() {
        let url = build_url("thing", Some(50.0), None);
        assert!(url.contains("_udlo=50"));
        assert!(!url.contains("_udhi"));
    }

    #[test]
    fn test_plugin_id() {
        let plugin = EbayPlugin;
        assert_eq!(plugin.plugin_id(), "ebay");
    }

    #[tokio::test]
    async fn test_supports_geo() {
        let plugin = EbayPlugin;
        assert!(!plugin.supports_geo().await);
    }
}
