use std::fs;

use scavenger::tui::widgets::thumbnail::{ext_from_url, ThumbnailCache};

#[test]
fn test_ext_from_url_jpg() {
    assert_eq!(ext_from_url("https://example.com/photo.jpg"), ".jpg");
}

#[test]
fn test_ext_from_url_png_with_query() {
    assert_eq!(
        ext_from_url("https://example.com/photo.png?w=100&h=100"),
        ".png"
    );
}

#[test]
fn test_ext_from_url_no_extension() {
    assert_eq!(ext_from_url("https://example.com/photo"), ".jpg");
}

#[test]
fn test_ext_from_url_webp() {
    assert_eq!(
        ext_from_url("https://example.com/img.webp?quality=80"),
        ".webp"
    );
}

#[test]
fn test_cache_hit_disk() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut cache = ThumbnailCache::new(dir.path().to_path_buf());
    let url = "https://example.com/test-image.png";

    // Initially nothing cached
    assert!(cache.check(url).is_none());

    // Place a file where the cache expects it
    let expected = {
        // We need the actual cache path, so use the same hash logic
        let path = cache.check(url); // still None
        drop(path);
        // Manually construct the expected path using the same hash
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(url.as_bytes());
            format!("{:x}", hasher.finalize())
        };
        dir.path().join(format!("{}.png", hash))
    };
    fs::write(&expected, b"fake image data").unwrap();

    // Now check should find it
    let result = cache.check(url);
    assert!(result.is_some());
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_cache_miss() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut cache = ThumbnailCache::new(dir.path().to_path_buf());
    assert!(cache.check("https://example.com/nonexistent.jpg").is_none());
}

#[test]
fn test_lru_eviction_at_128() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut cache = ThumbnailCache::new(dir.path().to_path_buf());

    // Insert 129 entries by placing files on disk and checking them
    for i in 0..=128 {
        let url = format!("https://example.com/img{}.jpg", i);
        // Create the expected cache file
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(url.as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let path = dir.path().join(format!("{}.jpg", hash));
        fs::write(&path, format!("data-{}", i)).unwrap();
        cache.check(&url);
    }

    // After 129 insertions exceeding the 128 limit, eviction drops oldest quarter (32)
    assert!(cache.mem_cache_len() <= 128);
    // The first entry should have been evicted
    assert!(cache.get_cached("https://example.com/img0.jpg").is_none());
    // The last entry should still be present
    assert!(cache.get_cached("https://example.com/img128.jpg").is_some());
}
