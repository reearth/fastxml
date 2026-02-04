//! File-based caching fetcher wrapper.
//!
//! Wraps any `SchemaFetcher` or `AsyncSchemaFetcher` with a temporary-file cache
//! so each URL is fetched at most once, while keeping memory usage low by storing
//! fetched content on disk instead of in a `DashMap`.

use std::path::{Path, PathBuf};

use dashmap::DashMap;
use xxhash_rust::xxh64;

use crate::error::Result;

use super::result::FetchResult;
use super::traits::SchemaFetcher;

/// Generates a cache file name from a URL using xxhash64.
fn cache_filename(url: &str) -> String {
    let hash = xxh64::xxh64(url.as_bytes(), 0);
    format!("{:016x}.xsd", hash)
}

/// A fetcher wrapper that caches fetch results as files on disk.
///
/// When a URL is requested:
/// 1. Check the index — if present, read the cached file.
/// 2. Otherwise delegate to the inner fetcher.
/// 3. Write the result to a file and register it in the index
///    (under both the requested URL and the final URL if a redirect occurred).
///
/// # Cache directory lifecycle
///
/// - [`FileCachingFetcher::new`] creates a temporary directory that is
///   automatically deleted when the fetcher is dropped.
/// - [`FileCachingFetcher::with_dir`] uses an existing directory and does
///   **not** clean it up on drop (persistent cache).
/// - [`FileCachingFetcher::with_temp_dir`] creates a temporary directory
///   inside the given parent and cleans it up on drop.
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::fetcher::{FileCachingFetcher, DefaultFetcher};
///
/// let fetcher = FileCachingFetcher::new(DefaultFetcher::new())?;
/// let result = fetcher.fetch("http://example.com/schema.xsd")?;
/// // Second call reads from the file cache
/// let cached = fetcher.fetch("http://example.com/schema.xsd")?;
/// ```
pub struct FileCachingFetcher<F: SchemaFetcher> {
    inner: F,
    cache_dir: PathBuf,
    /// `Some` → temp dir is deleted on drop; `None` → persistent directory.
    _temp_dir: Option<tempfile::TempDir>,
    index: DashMap<String, PathBuf>,
}

impl<F: SchemaFetcher> FileCachingFetcher<F> {
    /// Creates a new file-caching fetcher with an auto-created temporary directory.
    ///
    /// The temporary directory is deleted when this fetcher is dropped.
    pub fn new(inner: F) -> Result<Self> {
        let temp_dir = tempfile::TempDir::new()?;
        let cache_dir = temp_dir.path().to_path_buf();
        Ok(Self {
            inner,
            cache_dir,
            _temp_dir: Some(temp_dir),
            index: DashMap::new(),
        })
    }

    /// Creates a file-caching fetcher using an existing directory.
    ///
    /// The directory is **not** cleaned up when the fetcher is dropped,
    /// allowing it to serve as a persistent cache across runs.
    pub fn with_dir(inner: F, dir: impl AsRef<Path>) -> Self {
        Self {
            inner,
            cache_dir: dir.as_ref().to_path_buf(),
            _temp_dir: None,
            index: DashMap::new(),
        }
    }

    /// Creates a file-caching fetcher with a temporary directory inside `dir`.
    ///
    /// The temporary sub-directory is deleted when the fetcher is dropped.
    pub fn with_temp_dir(inner: F, dir: impl AsRef<Path>) -> Result<Self> {
        let temp_dir = tempfile::TempDir::new_in(dir)?;
        let cache_dir = temp_dir.path().to_path_buf();
        Ok(Self {
            inner,
            cache_dir,
            _temp_dir: Some(temp_dir),
            index: DashMap::new(),
        })
    }

    /// Pre-seeds the cache with content for a given URL.
    pub fn seed(&self, url: &str, content: Vec<u8>) -> Result<()> {
        let filename = cache_filename(url);
        let path = self.cache_dir.join(&filename);
        std::fs::write(&path, &content)?;
        self.index.insert(url.to_string(), path);
        Ok(())
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns a reference to the inner fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Returns the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Writes content to a cache file and registers it in the index for the given URL.
    fn write_cache(&self, url: &str, content: &[u8]) -> Result<PathBuf> {
        let filename = cache_filename(url);
        let path = self.cache_dir.join(&filename);
        std::fs::write(&path, content)?;
        self.index.insert(url.to_string(), path.clone());
        Ok(path)
    }
}

impl<F: SchemaFetcher> SchemaFetcher for FileCachingFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check index — read from file cache
        if let Some(entry) = self.index.get(url) {
            let content = std::fs::read(entry.value())?;
            return Ok(FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            });
        }

        // Delegate to inner
        let result = self.inner.fetch(url)?;

        // Write to file cache
        let path = self.write_cache(url, &result.content)?;

        // Also register under the final URL if a redirect occurred
        if result.final_url != url {
            self.index.insert(result.final_url.clone(), path);
        }

        Ok(result)
    }
}

/// Async version of [`FileCachingFetcher`].
///
/// Uses `tokio::fs` for file I/O so the cache operations don't block the
/// async runtime.
#[cfg(feature = "tokio")]
pub struct AsyncFileCachingFetcher<F: super::traits::AsyncSchemaFetcher> {
    inner: F,
    cache_dir: PathBuf,
    _temp_dir: Option<tempfile::TempDir>,
    index: DashMap<String, PathBuf>,
}

#[cfg(feature = "tokio")]
impl<F: super::traits::AsyncSchemaFetcher> AsyncFileCachingFetcher<F> {
    /// Creates a new async file-caching fetcher with an auto-created temporary directory.
    pub fn new(inner: F) -> Result<Self> {
        let temp_dir = tempfile::TempDir::new()?;
        let cache_dir = temp_dir.path().to_path_buf();
        Ok(Self {
            inner,
            cache_dir,
            _temp_dir: Some(temp_dir),
            index: DashMap::new(),
        })
    }

    /// Creates an async file-caching fetcher using an existing directory (persistent cache).
    pub fn with_dir(inner: F, dir: impl AsRef<Path>) -> Self {
        Self {
            inner,
            cache_dir: dir.as_ref().to_path_buf(),
            _temp_dir: None,
            index: DashMap::new(),
        }
    }

    /// Creates an async file-caching fetcher with a temporary directory inside `dir`.
    pub fn with_temp_dir(inner: F, dir: impl AsRef<Path>) -> Result<Self> {
        let temp_dir = tempfile::TempDir::new_in(dir)?;
        let cache_dir = temp_dir.path().to_path_buf();
        Ok(Self {
            inner,
            cache_dir,
            _temp_dir: Some(temp_dir),
            index: DashMap::new(),
        })
    }

    /// Pre-seeds the cache with content for a given URL.
    pub async fn seed(&self, url: &str, content: Vec<u8>) -> Result<()> {
        let filename = cache_filename(url);
        let path = self.cache_dir.join(&filename);
        tokio::fs::write(&path, &content).await?;
        self.index.insert(url.to_string(), path);
        Ok(())
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns a reference to the inner fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Returns the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl<F: super::traits::AsyncSchemaFetcher> super::traits::AsyncSchemaFetcher
    for AsyncFileCachingFetcher<F>
{
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check index — read from file cache
        if let Some(entry) = self.index.get(url) {
            let content = tokio::fs::read(entry.value()).await?;
            return Ok(FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            });
        }

        // Delegate to inner
        let result = self.inner.fetch(url).await?;

        // Write to file cache
        let filename = cache_filename(url);
        let path = self.cache_dir.join(&filename);
        tokio::fs::write(&path, &result.content).await?;
        self.index.insert(url.to_string(), path.clone());

        // Also register under the final URL if a redirect occurred
        if result.final_url != url {
            self.index.insert(result.final_url.clone(), path);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::NoopFetcher;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A mock fetcher that tracks fetch calls.
    struct TrackingFetcher {
        responses: HashMap<String, Vec<u8>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl TrackingFetcher {
        fn new(responses: HashMap<String, Vec<u8>>) -> Self {
            Self {
                responses,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl SchemaFetcher for TrackingFetcher {
        fn fetch(&self, url: &str) -> Result<FetchResult> {
            self.calls.lock().unwrap().push(url.to_string());
            match self.responses.get(url) {
                Some(content) => Ok(FetchResult {
                    content: content.clone(),
                    final_url: url.to_string(),
                    redirected: false,
                }),
                None => Err(crate::schema::fetch_error::FetchError::RequestFailed {
                    url: url.to_string(),
                    message: "Not found".to_string(),
                }
                .into()),
            }
        }
    }

    /// A mock fetcher that simulates redirects.
    struct RedirectFetcher {
        content: Vec<u8>,
        final_url: String,
    }

    impl SchemaFetcher for RedirectFetcher {
        fn fetch(&self, _url: &str) -> Result<FetchResult> {
            Ok(FetchResult {
                content: self.content.clone(),
                final_url: self.final_url.clone(),
                redirected: true,
            })
        }
    }

    #[test]
    fn test_file_caching_fetcher_caches_result() {
        let mut responses = HashMap::new();
        responses.insert(
            "http://example.com/a.xsd".to_string(),
            b"<schema/>".to_vec(),
        );
        let inner = TrackingFetcher::new(responses);

        let fetcher = FileCachingFetcher::new(inner).unwrap();

        // First fetch
        let r1 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r1.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1);

        // Second fetch should come from file cache
        let r2 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r2.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1); // still 1
    }

    #[test]
    fn test_file_caching_fetcher_seed() {
        let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
        fetcher
            .seed("http://example.com/test.xsd", b"<seeded/>".to_vec())
            .unwrap();

        let result = fetcher.fetch("http://example.com/test.xsd").unwrap();
        assert_eq!(result.content, b"<seeded/>");
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_file_caching_fetcher_len_is_empty() {
        let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
        assert!(fetcher.is_empty());
        assert_eq!(fetcher.len(), 0);

        fetcher
            .seed("http://example.com/a.xsd", b"a".to_vec())
            .unwrap();
        assert!(!fetcher.is_empty());
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_file_caching_fetcher_with_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let fetcher = FileCachingFetcher::with_dir(NoopFetcher, dir.path());
        assert_eq!(fetcher.cache_dir(), dir.path());
    }

    #[test]
    fn test_file_caching_fetcher_with_temp_dir() {
        let parent = tempfile::TempDir::new().unwrap();
        let fetcher = FileCachingFetcher::with_temp_dir(NoopFetcher, parent.path()).unwrap();
        assert!(fetcher.cache_dir().starts_with(parent.path()));
    }

    #[test]
    fn test_file_caching_fetcher_redirect_caches_both_urls() {
        let inner = RedirectFetcher {
            content: b"<redirected/>".to_vec(),
            final_url: "http://example.com/final.xsd".to_string(),
        };
        let fetcher = FileCachingFetcher::new(inner).unwrap();

        let r = fetcher.fetch("http://example.com/original.xsd").unwrap();
        assert_eq!(r.content, b"<redirected/>");

        // Both URLs should be in the index
        assert_eq!(fetcher.len(), 2);

        // Fetching by final URL should hit the cache
        let r2 = fetcher.fetch("http://example.com/final.xsd").unwrap();
        assert_eq!(r2.content, b"<redirected/>");
    }

    #[test]
    fn test_file_caching_fetcher_temp_dir_cleanup() {
        let cache_dir;
        {
            let fetcher = FileCachingFetcher::new(NoopFetcher).unwrap();
            fetcher
                .seed("http://example.com/a.xsd", b"data".to_vec())
                .unwrap();
            cache_dir = fetcher.cache_dir().to_path_buf();
            assert!(cache_dir.exists());
        }
        // After drop, the temp dir should be cleaned up
        assert!(!cache_dir.exists());
    }

    #[test]
    fn test_file_caching_fetcher_persistent_dir_not_cleaned() {
        let dir = tempfile::TempDir::new().unwrap();
        let dir_path = dir.path().to_path_buf();
        {
            let fetcher = FileCachingFetcher::with_dir(NoopFetcher, &dir_path);
            fetcher
                .seed("http://example.com/a.xsd", b"data".to_vec())
                .unwrap();
        }
        // After drop, the persistent dir should still exist
        assert!(dir_path.exists());
    }

    #[test]
    fn test_cache_filename_deterministic() {
        let a = cache_filename("http://example.com/schema.xsd");
        let b = cache_filename("http://example.com/schema.xsd");
        assert_eq!(a, b);
        assert!(a.ends_with(".xsd"));
    }

    #[test]
    fn test_cache_filename_different_urls() {
        let a = cache_filename("http://example.com/a.xsd");
        let b = cache_filename("http://example.com/b.xsd");
        assert_ne!(a, b);
    }
}
