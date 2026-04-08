use sha2::{Digest, Sha256};
use url::Url;

/// Query params stripped during URL normalization (tracking junk).
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

fn should_strip(key: &str) -> bool {
    STRIP_PARAMS.contains(&key) || key.starts_with("utm_")
}

/// Normalize a URL by stripping tracking params and the fragment.
pub fn normalize_url(raw: &str) -> String {
    let Ok(mut parsed) = Url::parse(raw) else {
        return raw.to_string();
    };

    let filtered: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !should_strip(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    parsed.set_query(None);
    if !filtered.is_empty() {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &filtered {
            ser.append_pair(k, v);
        }
        parsed.set_query(Some(&ser.finish()));
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
        let url = "https://www.ebay.com/itm/123?utm_source=foo&_trkparms=bar&real=yes";
        let norm = normalize_url(url);
        assert!(norm.contains("real=yes"));
        assert!(!norm.contains("utm_source"));
        assert!(!norm.contains("_trkparms"));
    }

    #[test]
    fn strips_fragment() {
        let url = "https://example.com/page#section";
        let norm = normalize_url(url);
        assert!(!norm.contains('#'));
    }

    #[test]
    fn content_hash_stable() {
        let h1 = content_hash("https://www.ebay.com/itm/123");
        let h2 = content_hash("https://www.ebay.com/itm/123#section");
        assert_eq!(h1, h2);
    }
}
