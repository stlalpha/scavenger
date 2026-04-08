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

/// Normalize a URL by stripping tracking parameters and fragments.
pub fn normalize_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return raw.to_string();
    };

    let filtered: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| {
            let key = k.as_ref();
            !STRIP_PARAMS.contains(&key) && !key.starts_with("utm_")
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    parsed.set_query(None);
    if !filtered.is_empty() {
        let qs: Vec<String> = filtered
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        parsed.set_query(Some(&qs.join("&")));
    }
    parsed.set_fragment(None);

    parsed.to_string()
}

/// SHA-256 hex digest of the normalized URL. Used as listing primary key.
pub fn content_hash(url: &str) -> String {
    let normalized = normalize_url(url);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tracking_params() {
        let url = "https://example.com/item?id=1&utm_source=google&ref=abc";
        let normalized = normalize_url(url);
        assert!(normalized.contains("id=1"));
        assert!(!normalized.contains("utm_source"));
        assert!(!normalized.contains("ref="));
    }

    #[test]
    fn same_url_same_hash() {
        let a = content_hash("https://example.com/item?id=1&utm_source=x");
        let b = content_hash("https://example.com/item?id=1&utm_campaign=y");
        assert_eq!(a, b);
    }
}
