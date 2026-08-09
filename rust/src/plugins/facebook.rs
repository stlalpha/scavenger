use chrono::Utc;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use tracing::{debug, info, warn};

use crate::models::{content_hash, KeywordEntry, Listing, ListingStatus, Profile};
use crate::plugins::browser::{new_page, PageGuard};
use crate::plugins::PluginError;

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

/// Parse a marketplace card's aria-label into (price, title, location).
///
/// Facebook renders a structured label on every result anchor:
/// `Title, $price[, reduced from $orig][, Location], listing <id>`
/// (location is empty for shipped items). This is far more reliable than
/// the card's visible text, which carries badges ("Just listed") and
/// strikethrough prices that poison positional line parsing.
pub fn parse_aria_label(aria: &str) -> Option<(Option<f64>, String, Option<String>)> {
    static LISTING_TAIL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r",\s*listing \d+$").unwrap());
    static PRICE_SEG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\$[0-9,]+(?:\.\d{2})?$").unwrap());
    static REDUCED_SEG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^reduced from \$[0-9,]+(?:\.\d{2})?$").unwrap());

    let core = LISTING_TAIL_RE.replace(aria.trim(), "");
    let segs: Vec<&str> = core.split(", ").collect();
    let price_idx = segs.iter().position(|s| PRICE_SEG_RE.is_match(s))?;
    if price_idx == 0 {
        return None; // no title before the price segment
    }

    let title = segs[..price_idx].join(", ");
    let price = extract_price(segs[price_idx]);

    let mut rest = &segs[price_idx + 1..];
    if let Some(first) = rest.first() {
        if REDUCED_SEG_RE.is_match(first) {
            rest = &rest[1..];
        }
    }
    let location = {
        let loc = rest.join(", ").trim().trim_matches(',').trim().to_string();
        if loc.is_empty() { None } else { Some(loc) }
    };

    Some((price, title, location))
}

/// Parse card text lines into (price, title, location) — fallback for
/// cards without an aria-label.
///
/// Cards often lead with badge lines ("Just listed") and may show two
/// price lines (current + strikethrough original), so the title is taken
/// as the first non-price line *after* the first price line when one
/// exists, and only otherwise as the first non-price line.
pub fn parse_card_text(text: &str) -> (Option<f64>, String, Option<String>) {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let first_price_idx = lines.iter().position(|l| PRICE_RE.is_match(l));
    let price = first_price_idx.and_then(|i| extract_price(lines[i]));

    let title_idx = match first_price_idx {
        Some(pi) => lines[pi + 1..]
            .iter()
            .position(|l| !PRICE_RE.is_match(l))
            .map(|off| pi + 1 + off),
        None => lines.iter().position(|l| !PRICE_RE.is_match(l)),
    };
    let title = title_idx.map(|i| lines[i].to_string()).unwrap_or_default();

    // Prefer a line that looks like a place ("Portland, OR" / "Ships to
    // you"); fall back to the first non-price line after the title.
    static LOC_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r",\s*[A-Z]{2}$|^Ships to you$").unwrap());
    let mut location: Option<String> = None;
    if let Some(ti) = title_idx {
        let after = &lines[ti + 1..];
        location = after
            .iter()
            .find(|l| LOC_RE.is_match(l))
            .or_else(|| after.iter().find(|l| !PRICE_RE.is_match(l)))
            .map(|l| l.to_string());
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
    /// The aria-label is the primary source (structured, badge-immune);
    /// visible card text is the fallback. Pure logic, kept separate from
    /// the browser interaction loop so it can be tested against captured
    /// real-page fixtures.
    pub fn parse_cards(cards: &[FbRawCard], profile: &Profile) -> Vec<Listing> {
        let now = Utc::now();
        let mut seen_urls = HashSet::new();
        let mut listings = Vec::new();

        for card in cards {
            let FbRawCard { href, text, img_src: image_src, aria, .. } = card;
            if !href.contains("/marketplace/item/") {
                continue;
            }

            let item_url = clean_fb_url(href);
            if !seen_urls.insert(item_url.clone()) {
                continue;
            }

            let (price, title, location) = aria
                .as_deref()
                .and_then(parse_aria_label)
                .unwrap_or_else(|| parse_card_text(text));
            if title.is_empty() {
                debug!("Facebook: unparseable card at {item_url}: {:?}", &text.get(..60));
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

    /// Scrape Facebook Marketplace via CDP.
    async fn scrape(
        &self,
        keywords: &str,
        profile: &Profile,
    ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let page = new_page()
            .await
            .map_err(|e| PluginError::Browser(e.to_string()))?;
        let guard = PageGuard::new(page);

        let url = build_search_url(keywords, profile);
        guard
            .page()?
            .goto(&url)
            .await
            .map_err(|e| PluginError::Navigation(e.to_string()))?;

        // Wait for page to settle after navigation
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let title: String = guard
            .page()?
            .evaluate("document.title")
            .await
            .map_err(|e| PluginError::Browser(e.to_string()))?
            .into_value()
            .map_err(|e| PluginError::Browser(e.to_string()))?;

        let current_url: String = guard
            .page()?
            .evaluate("window.location.href")
            .await
            .map_err(|e| PluginError::Browser(e.to_string()))?
            .into_value()
            .map_err(|e| PluginError::Browser(e.to_string()))?;

        debug!("Facebook: page loaded, title={title:?}, url={current_url:?}");

        if is_login_page(&title, &current_url) {
            let _ = guard.close().await;
            return Err(Box::new(PluginError::BotDetected {
                plugin_id: "facebook".into(),
                url: "https://www.facebook.com/login".into(),
                message: "Facebook not logged in".into(),
            }));
        }

        // Detect redirect away from marketplace — retry once
        if !current_url.contains("/marketplace/") {
            warn!("Facebook: redirected away from marketplace (url: {current_url})");
            guard
                .page()?
                .goto(&url)
                .await
                .map_err(|e| PluginError::Navigation(e.to_string()))?;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            let retry_url: String = guard
                .page()?
                .evaluate("window.location.href")
                .await
                .map_err(|e| PluginError::Browser(e.to_string()))?
                .into_value()
                .map_err(|e| PluginError::Browser(e.to_string()))?;

            if !retry_url.contains("/marketplace/") {
                warn!("Facebook: marketplace redirect failed twice");
                let _ = guard.close().await;
                return Ok(vec![]);
            }
        }

        // Scroll down to load more results
        for _ in 0..3 {
            if let Ok(p) = guard.page() {
                let _ = p.evaluate("window.scrollBy(0, window.innerHeight)").await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }

        // Extract card data via JS
        let item_sels = serde_json::to_string(&ITEM_SELECTORS).unwrap();
        let js = format!(
            r#"
            (() => {{
                const sels = {item_sels};
                let items = [];
                for (const sel of sels) {{
                    items = document.querySelectorAll(sel);
                    if (items.length > 0) break;
                }}
                const results = [];
                for (const item of items) {{
                    const href = item.getAttribute("href") || "";
                    const text = item.innerText || "";
                    const img = item.querySelector("img");
                    const imgSrc = img ? img.getAttribute("src") : null;
                    const aria = item.getAttribute("aria-label");
                    const imgAlt = img ? img.getAttribute("alt") : null;
                    results.push({{ href, text, imgSrc, aria, imgAlt }});
                }}
                return results;
            }})()
            "#
        );

        let page_ref = match guard.page() {
            Ok(p) => p,
            Err(e) => {
                warn!("Facebook: {e}");
                let _ = guard.close().await;
                return Ok(vec![]);
            }
        };
        let raw_cards: Vec<FbRawCard> = match page_ref.evaluate(js).await {
            Ok(val) => match val.into_value() {
                Ok(cards) => cards,
                Err(e) => {
                    warn!("Facebook: failed to parse cards: {e}");
                    let _ = guard.close().await;
                    return Ok(vec![]);
                }
            },
            Err(e) => {
                warn!("Facebook: JS eval failed: {e}");
                let _ = guard.close().await;
                return Ok(vec![]);
            }
        };

        if raw_cards.is_empty() {
            warn!("Facebook: no listings found (title: {title:?})");
            let _ = guard.close().await;
            return Ok(vec![]);
        }

        let listings = Self::parse_cards(&raw_cards, profile);
        info!(
            "Facebook: {} cards -> {} listings for '{keywords}'",
            raw_cards.len(),
            listings.len()
        );

        let _ = guard.close().await;
        Ok(listings)
    }
}

/// Raw card data from Facebook page JS evaluation.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FbRawCard {
    pub href: String,
    pub text: String,
    pub img_src: Option<String>,
    #[serde(default)]
    pub aria: Option<String>,
    #[serde(default)]
    pub img_alt: Option<String>,
}

#[async_trait::async_trait]
impl crate::plugins::Plugin for FacebookPlugin {
    fn plugin_id(&self) -> &str {
        "facebook"
    }

    async fn fetch(&self, profile: &Profile) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
        let keywords = Self::build_keywords(profile);
        // 90s timeout — Facebook is slow (scroll + network idle waits)
        let scrape_result = tokio::time::timeout(
            std::time::Duration::from_secs(90),
            self.scrape(&keywords, profile),
        )
        .await
        .unwrap_or_else(|_| {
            warn!("Facebook scrape timed out after 90s");
            Ok(vec![])
        });
        match scrape_result {
            Ok(listings) => Ok(listings),
            Err(e) => {
                if let Some(pe) = e.downcast_ref::<PluginError>() {
                    if matches!(pe, PluginError::BotDetected { .. }) {
                        return Err(e);
                    }
                }
                warn!("Facebook fetch failed: {e}");
                Ok(vec![])
            }
        }
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

    fn raw_card(href: &str, text: &str, img_src: Option<&str>) -> FbRawCard {
        FbRawCard {
            href: href.into(),
            text: text.into(),
            img_src: img_src.map(str::to_string),
            aria: None,
            img_alt: None,
        }
    }

    #[test]
    fn parse_cards_deduplicates() {
        let profile = test_profile();
        let cards = vec![
            raw_card("/marketplace/item/123/?ref=search", "$100\nItem One\nCity", None),
            raw_card("/marketplace/item/123/?ref=other", "$100\nItem One\nCity", None),
        ];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings.len(), 1);
    }

    #[test]
    fn parse_cards_skips_data_urls() {
        let profile = test_profile();
        let cards = vec![raw_card(
            "/marketplace/item/456",
            "$200\nSomething",
            Some("data:image/gif;base64,R0lGODlh"),
        )];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings.len(), 1);
        assert!(listings[0].image_urls.is_empty());
    }

    #[test]
    fn parse_cards_includes_real_image() {
        let profile = test_profile();
        let cards = vec![raw_card(
            "/marketplace/item/789",
            "$300\nNice Guitar",
            Some("https://scontent.xx.fbcdn.net/image.jpg"),
        )];
        let listings = FacebookPlugin::parse_cards(&cards, &profile);
        assert_eq!(listings[0].image_urls.len(), 1);
    }

    #[test]
    fn parse_cards_prefers_aria_over_badge_poisoned_text() {
        let profile = test_profile();
        let mut card = raw_card(
            "/marketplace/item/555",
            "Just listed\n$140\nLenovo ThinkPad T450s\nSt Peters, MO",
            None,
        );
        card.aria =
            Some("Lenovo ThinkPad T450s, $140, St Peters, MO, listing 555".into());
        let listings = FacebookPlugin::parse_cards(&[card], &profile);
        assert_eq!(listings[0].title, "Lenovo ThinkPad T450s");
        assert_eq!(listings[0].price, Some(140.0));
        assert_eq!(listings[0].location.as_deref(), Some("St Peters, MO"));
    }

    #[test]
    fn parse_aria_label_typical() {
        let (price, title, location) =
            parse_aria_label("Thinkpad X380 Computer, $200, reduced from $400, O'Fallon, MO, listing 1187064949900306")
                .unwrap();
        assert_eq!(title, "Thinkpad X380 Computer");
        assert_eq!(price, Some(200.0));
        assert_eq!(location.as_deref(), Some("O'Fallon, MO"));
    }

    #[test]
    fn parse_aria_label_shipped_item_has_no_location() {
        let (price, title, location) =
            parse_aria_label("Lenovo ThinkPad E550, $150, , listing 1260366362772611").unwrap();
        assert_eq!(title, "Lenovo ThinkPad E550");
        assert_eq!(price, Some(150.0));
        assert_eq!(location, None);
    }

    #[test]
    fn parse_aria_label_title_with_comma() {
        let (price, title, location) =
            parse_aria_label("ThinkPad T14, 32GB RAM, $350, Chicago, IL, listing 42").unwrap();
        assert_eq!(title, "ThinkPad T14, 32GB RAM");
        assert_eq!(price, Some(350.0));
        assert_eq!(location.as_deref(), Some("Chicago, IL"));
    }

    #[test]
    fn parse_aria_label_rejects_priceless_or_titleless() {
        assert!(parse_aria_label("$200, Somewhere, MO, listing 1").is_none());
        assert!(parse_aria_label("no structure here at all").is_none());
    }

    #[test]
    fn parse_card_text_skips_badge_and_strikethrough() {
        let (price, title, location) =
            parse_card_text("Just listed\n$140\nLenovo ThinkPad T450s\nSt Peters, MO");
        assert_eq!(price, Some(140.0));
        assert_eq!(title, "Lenovo ThinkPad T450s");
        assert_eq!(location.as_deref(), Some("St Peters, MO"));

        let (price, title, _) =
            parse_card_text("$200\n$400\nThinkpad X380 Computer\nO'Fallon, MO");
        assert_eq!(price, Some(200.0));
        assert_eq!(title, "Thinkpad X380 Computer");
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
