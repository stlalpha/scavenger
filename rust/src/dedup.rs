use sha2::{Digest, Sha256};
use url::Url;

const STRIP_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "ssPageName",
    "_trkparms",
    "_trktoken",
    "hash",
    "ref",
    "mkevt",
    "mkcid",
    "mkrid",
    "campid",
    "toolid",
];

/// Parse URL, strip tracking params, remove fragment, rebuild.
pub fn normalize_url(raw: &str) -> String {
    let parsed = match Url::parse(raw) {
        Ok(u) => u,
        Err(_) => return raw.to_string(),
    };

    let filtered: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            !STRIP_PARAMS.contains(&k.as_ref()) && !k.starts_with("utm_")
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let mut out = parsed.clone();
    out.set_fragment(None);

    if filtered.is_empty() {
        out.set_query(None);
    } else {
        let qs = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&filtered)
            .finish();
        out.set_query(Some(&qs));
    }

    out.to_string()
}

/// SHA-256 hex digest of the normalized URL.
pub fn content_hash(raw: &str) -> String {
    let normalized = normalize_url(raw);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_utm_params() {
        let url = "https://example.com/item?id=1&utm_source=google&utm_medium=cpc";
        let norm = normalize_url(url);
        assert!(norm.contains("id=1"));
        assert!(!norm.contains("utm_source"));
        assert!(!norm.contains("utm_medium"));
    }

    #[test]
    fn strips_ebay_tracking_params() {
        let url = "https://ebay.com/itm/123?ssPageName=foo&_trkparms=bar&mkevt=1";
        let norm = normalize_url(url);
        assert!(!norm.contains("ssPageName"));
        assert!(!norm.contains("_trkparms"));
        assert!(!norm.contains("mkevt"));
    }

    #[test]
    fn strips_fragment() {
        let url = "https://example.com/item?id=1#section";
        let norm = normalize_url(url);
        assert!(!norm.contains('#'));
    }

    #[test]
    fn preserves_non_tracking_params() {
        let url = "https://example.com/search?q=test&page=2&utm_campaign=spring";
        let norm = normalize_url(url);
        assert!(norm.contains("q=test"));
        assert!(norm.contains("page=2"));
        assert!(!norm.contains("utm_campaign"));
    }

    #[test]
    fn content_hash_stable() {
        let url = "https://example.com/item?id=42";
        let h1 = content_hash(url);
        let h2 = content_hash(url);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn same_url_different_tracking_same_hash() {
        let a = "https://example.com/item?id=42&utm_source=email";
        let b = "https://example.com/item?id=42&utm_source=twitter";
        assert_eq!(content_hash(a), content_hash(b));
    }
}
