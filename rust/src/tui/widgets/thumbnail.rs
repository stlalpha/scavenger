use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

const MEM_CACHE_SIZE: usize = 128;
const DEFAULT_MAX_AGE_DAYS: u64 = 30;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Extract file extension from a URL, stripping query params.
pub fn ext_from_url(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    match path.rfind('.') {
        Some(i) => {
            let ext = &path[i..];
            // Only accept short, reasonable extensions
            if ext.len() <= 5 && ext.chars().all(|c| c == '.' || c.is_ascii_alphanumeric()) {
                ext
            } else {
                ".jpg"
            }
        }
        None => ".jpg",
    }
}

/// Hash a URL to produce a cache filename.
fn url_hash(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Downloads and caches listing images on disk with an in-memory LRU layer.
pub struct ThumbnailCache {
    cache_dir: PathBuf,
    max_age: Duration,
    client: reqwest::Client,
    resolved: HashMap<String, PathBuf>,
    /// Insertion-ordered keys for LRU eviction.
    insertion_order: Vec<String>,
}

impl ThumbnailCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        fs::create_dir_all(&cache_dir).ok();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            cache_dir,
            max_age: Duration::from_secs(DEFAULT_MAX_AGE_DAYS * 86400),
            client,
            resolved: HashMap::new(),
            insertion_order: Vec::new(),
        }
    }

    pub fn default_cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("scavenger")
            .join("images")
    }

    pub fn with_default_dir() -> Self {
        Self::new(Self::default_cache_dir())
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        let hash = url_hash(url);
        let ext = ext_from_url(url);
        self.cache_dir.join(format!("{hash}{ext}"))
    }

    /// Remove cached images older than max_age. Returns count removed.
    pub fn evict_stale(&mut self) -> usize {
        let cutoff = SystemTime::now()
            .checked_sub(self.max_age)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut removed = 0;
        if let Ok(entries) = fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let modified = path
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < cutoff {
                    if fs::remove_file(&path).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
        if removed > 0 {
            self.resolved.clear();
            self.insertion_order.clear();
        }
        removed
    }

    /// Look up a cached path for the URL. Returns None if not cached.
    pub fn get_cached(&self, url: &str) -> Option<&PathBuf> {
        self.resolved.get(url)
    }

    /// Check memory cache, then disk. Returns the cached path or None.
    pub fn check(&mut self, url: &str) -> Option<PathBuf> {
        if let Some(path) = self.resolved.get(url) {
            return Some(path.clone());
        }
        let path = self.cache_path(url);
        if path.exists() {
            self.insert_mem(url.to_string(), path.clone());
            return Some(path);
        }
        None
    }

    /// Download an image and cache it. Returns the path on success.
    pub async fn download(&mut self, url: &str) -> Option<PathBuf> {
        let dest = self.cache_path(url);
        match self.do_download(url, &dest).await {
            Ok(()) => {
                self.insert_mem(url.to_string(), dest.clone());
                Some(dest)
            }
            Err(_) => None,
        }
    }

    /// Get from memory/disk cache, or download. Returns path or None.
    pub async fn get(&mut self, url: &str) -> Option<PathBuf> {
        if let Some(path) = self.check(url) {
            return Some(path);
        }
        self.download(url).await
    }

    async fn do_download(&self, url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        let tmp = dest.with_extension("tmp");
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn insert_mem(&mut self, url: String, path: PathBuf) {
        if !self.resolved.contains_key(&url) {
            self.insertion_order.push(url.clone());
        }
        self.resolved.insert(url, path);
        self.evict_mem_if_full();
    }

    fn evict_mem_if_full(&mut self) {
        if self.resolved.len() > MEM_CACHE_SIZE {
            let drop_count = MEM_CACHE_SIZE / 4;
            let to_drop: Vec<String> = self.insertion_order.drain(..drop_count).collect();
            for key in &to_drop {
                self.resolved.remove(key);
            }
        }
    }

    pub fn mem_cache_len(&self) -> usize {
        self.resolved.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ext_from_url() {
        assert_eq!(ext_from_url("https://example.com/photo.jpg"), ".jpg");
        assert_eq!(ext_from_url("https://example.com/photo.png?w=100"), ".png");
        assert_eq!(ext_from_url("https://example.com/photo"), ".jpg");
        assert_eq!(
            ext_from_url("https://example.com/img.webp?quality=80"),
            ".webp"
        );
    }

    #[test]
    fn test_cache_path_uses_hash() {
        let dir = TempDir::new().unwrap();
        let cache = ThumbnailCache::new(dir.path().to_path_buf());
        let path = cache.cache_path("https://example.com/photo.png");
        assert!(path.to_string_lossy().ends_with(".png"));
        assert!(path.to_string_lossy().contains(&url_hash("https://example.com/photo.png")[..8]));
    }

    #[test]
    fn test_lru_eviction_at_128() {
        let dir = TempDir::new().unwrap();
        let mut cache = ThumbnailCache::new(dir.path().to_path_buf());

        // Insert 129 entries — should trigger eviction
        for i in 0..=MEM_CACHE_SIZE {
            let url = format!("https://example.com/img{}.jpg", i);
            let path = dir.path().join(format!("img{}.jpg", i));
            fs::write(&path, b"fake").unwrap();
            cache.insert_mem(url, path);
        }

        // After eviction of oldest quarter (32), we should have 129 - 32 = 97
        assert!(cache.mem_cache_len() <= MEM_CACHE_SIZE);
        // The earliest entries should have been evicted
        assert!(!cache.resolved.contains_key("https://example.com/img0.jpg"));
        // Later entries should still be present
        let last_url = format!("https://example.com/img{}.jpg", MEM_CACHE_SIZE);
        assert!(cache.resolved.contains_key(&last_url));
    }

    #[test]
    fn test_check_disk_hit() {
        let dir = TempDir::new().unwrap();
        let mut cache = ThumbnailCache::new(dir.path().to_path_buf());
        let url = "https://example.com/test.png";

        // Place a file on disk where cache expects it
        let expected_path = cache.cache_path(url);
        fs::write(&expected_path, b"image data").unwrap();

        // check() should find it on disk and populate memory cache
        let result = cache.check(url);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), expected_path);
        assert!(cache.resolved.contains_key(url));
    }

    #[test]
    fn test_check_miss() {
        let dir = TempDir::new().unwrap();
        let mut cache = ThumbnailCache::new(dir.path().to_path_buf());
        assert!(cache.check("https://example.com/nope.jpg").is_none());
    }
}
