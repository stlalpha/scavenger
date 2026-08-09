use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::sync::{LazyLock, OnceLock};

// eBay: gallery images embedded as JSON in page source
static EBAY_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""(https://i\.ebayimg\.com/images/g/[^"]+/s-l\d+\.\w+)""#).unwrap()
});

// Craigslist: image IDs in a JS array
static CL_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https://images\.craigslist\.org/[a-zA-Z0-9_]+_\d+x\d+\.jpg").unwrap()
});

// Facebook: scontent CDN URLs
static FB_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""(https://scontent[^"]+)""#).unwrap()
});

// Normalization patterns
static EBAY_SIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/s-l\d+\.").unwrap());
static CL_SIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"_\d+x\d+\.jpg").unwrap());

const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

/// Global persistent HTTP client with connection pooling.
static CLIENT: OnceLock<Client> = OnceLock::new();

fn get_client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Fetch a listing page and extract all product image URLs.
pub async fn fetch_listing_images(
    url: &str,
    source_id: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    if url.is_empty() || !url.starts_with("http") {
        return Ok(vec![]);
    }

    // Facebook item pages are login-gated and JS-rendered; a plain
    // cookieless GET only reaches the logged-out wall, whose regex scrape
    // yields Facebook's own UI images (logo/chrome), not the listing
    // photos — and those junk images then replace the working card image
    // in the detail panel. There is no useful gallery to fetch this way,
    // so skip it and keep the card image. (A real FB gallery needs the
    // logged-in CDP browser — see extract_facebook, still used if a page's
    // HTML is ever provided through that path.)
    if source_id == "facebook" {
        return Ok(vec![]);
    }

    let resp = get_client().get(url).send().await?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let html = resp.text().await?;

    Ok(match source_id {
        "ebay" => extract_ebay(&html),
        "craigslist" => extract_craigslist(&html),
        "facebook" => extract_facebook(&html),
        _ => vec![],
    })
}

/// Extract eBay gallery images, normalized to s-l500.
pub fn extract_ebay(html: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut images = Vec::new();
    for cap in EBAY_IMG_RE.captures_iter(html) {
        let raw = &cap[1];
        let normalized = EBAY_SIZE_RE.replace(raw, "/s-l500.").to_string();
        if seen.insert(normalized.clone()) {
            images.push(normalized);
        }
    }
    images
}

/// Extract Craigslist gallery images, normalized to 600x450.
pub fn extract_craigslist(html: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut images = Vec::new();
    for m in CL_IMG_RE.find_iter(html) {
        let normalized = CL_SIZE_RE.replace(m.as_str(), "_600x450.jpg").to_string();
        if seen.insert(normalized.clone()) {
            images.push(normalized);
        }
    }
    images
}

/// Extract Facebook marketplace images, filtering avatars, capped at 10.
pub fn extract_facebook(html: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut images = Vec::new();
    for cap in FB_IMG_RE.captures_iter(html) {
        let url = &cap[1];
        if url.contains("emoji") || url.contains("p50x50") || url.contains("p36x36") {
            continue;
        }
        if seen.insert(url.to_string()) {
            images.push(url.to_string());
        }
        if images.len() >= 10 {
            break;
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ebay_image_extraction() {
        let html = r#"
            "https://i.ebayimg.com/images/g/ABC123/s-l1600.jpg"
            "https://i.ebayimg.com/images/g/ABC123/s-l300.jpg"
            "https://i.ebayimg.com/images/g/DEF456/s-l1600.png"
        "#;
        let images = extract_ebay(html);
        assert_eq!(images.len(), 2);
        assert!(images[0].contains("/s-l500."));
        assert!(images[1].contains("/s-l500."));
    }

    #[test]
    fn craigslist_image_extraction() {
        let html = r#"
            https://images.craigslist.org/abc_123_300x300.jpg
            https://images.craigslist.org/abc_123_50x50.jpg
            https://images.craigslist.org/def_456_300x300.jpg
        "#;
        let images = extract_craigslist(html);
        assert_eq!(images.len(), 2);
        assert!(images[0].ends_with("_600x450.jpg"));
        assert!(images[1].ends_with("_600x450.jpg"));
    }

    #[tokio::test]
    async fn facebook_gallery_fetch_is_skipped_no_network() {
        // Must return empty without any HTTP request — the cookieless GET
        // only reaches FB's login wall and its scrape is junk.
        let out = fetch_listing_images(
            "https://www.facebook.com/marketplace/item/123",
            "facebook",
        )
        .await
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn facebook_image_extraction() {
        let html = r#"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/big_photo.jpg"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/another.jpg"
        "#;
        let images = extract_facebook(html);
        assert_eq!(images.len(), 2);
        assert!(images[0].starts_with("https://scontent"));
    }

    #[test]
    fn facebook_filters_avatars() {
        let html = r#"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/p50x50/avatar.jpg"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/p36x36/tiny.jpg"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/emoji/smile.png"
            "https://scontent.fxxx1-1.fna.fbcdn.net/v/real_photo.jpg"
        "#;
        let images = extract_facebook(html);
        assert_eq!(images.len(), 1);
        assert!(images[0].contains("real_photo"));
    }

    #[test]
    fn facebook_caps_at_10() {
        let mut html = String::new();
        for i in 0..20 {
            html.push_str(&format!(
                "\"https://scontent.fxxx1-1.fna.fbcdn.net/v/photo_{i}.jpg\"\n"
            ));
        }
        let images = extract_facebook(&html);
        assert_eq!(images.len(), 10);
    }

    #[test]
    fn empty_url_returns_empty() {
        assert!(extract_ebay("").is_empty());
        assert!(extract_craigslist("").is_empty());
        assert!(extract_facebook("").is_empty());
    }

    #[test]
    fn ebay_normalizes_to_l500() {
        let html = r#""https://i.ebayimg.com/images/g/XYZ/s-l96.jpg""#;
        let images = extract_ebay(html);
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0],
            "https://i.ebayimg.com/images/g/XYZ/s-l500.jpg"
        );
    }
}
