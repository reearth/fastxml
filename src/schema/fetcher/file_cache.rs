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

/// The name of the record kept beside a cache file: the URL the content was
/// requested as and the URL it was finally fetched from, one per line.
///
/// The record is written after the content, so its presence marks a complete
/// entry, and its first line guards against two URLs sharing a hash.
fn record_filename(url: &str) -> String {
    let hash = xxh64::xxh64(url.as_bytes(), 0);
    format!("{:016x}.url", hash)
}

fn record_text(url: &str, final_url: &str) -> String {
    format!("{url}\n{final_url}\n")
}

/// The final URL a record names, if it was written for `url`.
fn parse_record(text: &str, url: &str) -> Option<String> {
    let mut lines = text.lines();
    (lines.next()? == url).then(|| lines.next().map(str::to_string))?
}

/// A path to write to before renaming over `path`, unique to this process
/// and call, so a reader never sees a half-written file.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{n}.tmp", std::process::id()));
    path.with_file_name(name)
}

fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let temp = temp_path(path);
    std::fs::write(&temp, content)?;
    std::fs::rename(&temp, path)
}

/// A cached document: where its content is and the URL it was finally
/// fetched from.
#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    final_url: String,
}

impl Entry {
    fn result(&self, url: &str, content: Vec<u8>) -> FetchResult {
        FetchResult {
            content,
            final_url: self.final_url.clone(),
            redirected: self.final_url != url,
        }
    }
}

/// Writes the content and then the record of `url` into `dir`.
fn store(dir: &Path, url: &str, final_url: &str, content: &[u8]) -> Result<Entry> {
    let path = dir.join(cache_filename(url));
    write_atomic(&path, content)?;
    write_atomic(
        &dir.join(record_filename(url)),
        record_text(url, final_url).as_bytes(),
    )?;
    Ok(Entry {
        path,
        final_url: final_url.to_string(),
    })
}

/// The complete entry for `url` a previous fetcher left in `dir`, if any.
fn load(dir: &Path, url: &str) -> Option<Entry> {
    let text = std::fs::read_to_string(dir.join(record_filename(url))).ok()?;
    let final_url = parse_record(&text, url)?;
    let path = dir.join(cache_filename(url));
    path.is_file().then_some(Entry { path, final_url })
}

/// A fetcher wrapper that caches fetch results as files on disk.
///
/// When a URL is requested:
/// 1. Check the index — if present, read the cached file.
/// 2. Otherwise look for an entry an earlier fetcher left in the cache
///    directory, so a persistent directory is reused across runs.
/// 3. Otherwise delegate to the inner fetcher, write the result to a file
///    and register it (under both the requested URL and the final URL if a
///    redirect occurred).
///
/// A cache hit reports the URL the document was finally fetched from, so
/// relative imports inside it resolve as they did on the first fetch.
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
    index: DashMap<String, Entry>,
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
        let entry = store(&self.cache_dir, url, url, &content)?;
        self.index.insert(url.to_string(), entry);
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

impl<F: SchemaFetcher> SchemaFetcher for FileCachingFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let cached = match self.index.get(url) {
            Some(entry) => Some(entry.clone()),
            None => load(&self.cache_dir, url),
        };
        if let Some(entry) = cached {
            let content = std::fs::read(&entry.path)?;
            let result = entry.result(url, content);
            self.index.insert(url.to_string(), entry);
            return Ok(result);
        }

        let result = self.inner.fetch(url)?;
        let entry = store(&self.cache_dir, url, &result.final_url, &result.content)?;
        self.index.insert(url.to_string(), entry);
        // Also cache it under the final URL if a redirect occurred.
        if result.final_url != url {
            let target = store(
                &self.cache_dir,
                &result.final_url,
                &result.final_url,
                &result.content,
            )?;
            self.index.insert(result.final_url.clone(), target);
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
    index: DashMap<String, Entry>,
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
        let entry = self.store(url, url, content).await?;
        self.index.insert(url.to_string(), entry);
        Ok(())
    }

    /// [`store`] on a blocking thread, so file I/O does not block the
    /// runtime.
    async fn store(&self, url: &str, final_url: &str, content: Vec<u8>) -> Result<Entry> {
        let (dir, url, final_url) = (
            self.cache_dir.clone(),
            url.to_string(),
            final_url.to_string(),
        );
        tokio::task::spawn_blocking(move || store(&dir, &url, &final_url, &content))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
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
        let cached = match self.index.get(url) {
            Some(entry) => Some(entry.clone()),
            None => {
                let (dir, key) = (self.cache_dir.clone(), url.to_string());
                tokio::task::spawn_blocking(move || load(&dir, &key))
                    .await
                    .map_err(|e| std::io::Error::other(e.to_string()))?
            }
        };
        if let Some(entry) = cached {
            let content = tokio::fs::read(&entry.path).await?;
            let result = entry.result(url, content);
            self.index.insert(url.to_string(), entry);
            return Ok(result);
        }

        let result = self.inner.fetch(url).await?;
        let entry = self
            .store(url, &result.final_url, result.content.clone())
            .await?;
        self.index.insert(url.to_string(), entry);
        // Also cache it under the final URL if a redirect occurred.
        if result.final_url != url {
            let target = self
                .store(&result.final_url, &result.final_url, result.content.clone())
                .await?;
            self.index.insert(result.final_url.clone(), target);
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
                None => Err(crate::schema::fetcher::error::FetchError::RequestFailed {
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

    #[test]
    fn test_file_caching_fetcher_persistent_dir_is_reused_by_a_new_fetcher() {
        let dir = tempfile::TempDir::new().unwrap();
        let url = "http://example.com/a.xsd";
        let responses = HashMap::from([(url.to_string(), b"<schema/>".to_vec())]);

        let first =
            FileCachingFetcher::with_dir(TrackingFetcher::new(responses.clone()), dir.path());
        assert_eq!(first.fetch(url).unwrap().content, b"<schema/>");
        assert_eq!(first.inner().call_count(), 1);
        drop(first);

        // A new fetcher (as in another process) reads the files instead of
        // fetching again.
        let second = FileCachingFetcher::with_dir(TrackingFetcher::new(responses), dir.path());
        let r = second.fetch(url).unwrap();
        assert_eq!(r.content, b"<schema/>");
        assert_eq!(r.final_url, url);
        assert!(!r.redirected);
        assert_eq!(second.inner().call_count(), 0);
    }

    #[test]
    fn test_file_caching_fetcher_cache_hit_keeps_the_redirect_target() {
        let dir = tempfile::TempDir::new().unwrap();
        let original = "http://example.com/original.xsd";
        let final_url = "http://example.com/dir/final.xsd";
        let first = FileCachingFetcher::with_dir(
            RedirectFetcher {
                content: b"<redirected/>".to_vec(),
                final_url: final_url.to_string(),
            },
            dir.path(),
        );
        first.fetch(original).unwrap();

        // Relative imports resolve against the final URL, so a hit must
        // report it, from memory and from disk alike.
        let from_memory = first.fetch(original).unwrap();
        assert_eq!(from_memory.final_url, final_url);
        assert!(from_memory.redirected);

        let second = FileCachingFetcher::with_dir(NoopFetcher, dir.path());
        let from_disk = second.fetch(original).unwrap();
        assert_eq!(from_disk.content, b"<redirected/>");
        assert_eq!(from_disk.final_url, final_url);
        assert!(from_disk.redirected);
        let target = second.fetch(final_url).unwrap();
        assert_eq!(target.final_url, final_url);
        assert!(!target.redirected);
    }

    #[test]
    fn test_file_caching_fetcher_ignores_a_content_file_without_its_record() {
        let dir = tempfile::TempDir::new().unwrap();
        let url = "http://example.com/a.xsd";
        // A file left by an older version, or by an interrupted write.
        std::fs::write(dir.path().join(cache_filename(url)), b"stale").unwrap();

        let responses = HashMap::from([(url.to_string(), b"<fresh/>".to_vec())]);
        let fetcher = FileCachingFetcher::with_dir(TrackingFetcher::new(responses), dir.path());
        assert_eq!(fetcher.fetch(url).unwrap().content, b"<fresh/>");
        assert_eq!(fetcher.inner().call_count(), 1);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_async_file_caching_fetcher_persistent_dir_is_reused() {
        use crate::schema::fetcher::AsyncSchemaFetcher;

        let dir = tempfile::TempDir::new().unwrap();
        let url = "http://example.com/a.xsd";
        let first = FileCachingFetcher::with_dir(
            RedirectFetcher {
                content: b"<redirected/>".to_vec(),
                final_url: "http://example.com/final.xsd".to_string(),
            },
            dir.path(),
        );
        first.fetch(url).unwrap();

        // The sync and async fetchers share the on-disk format.
        let second = AsyncFileCachingFetcher::with_dir(NoopAsync, dir.path());
        let r = second.fetch(url).await.unwrap();
        assert_eq!(r.content, b"<redirected/>");
        assert_eq!(r.final_url, "http://example.com/final.xsd");
        assert!(r.redirected);
    }

    /// An async fetcher that fails every request.
    #[cfg(feature = "tokio")]
    struct NoopAsync;

    #[cfg(feature = "tokio")]
    #[async_trait::async_trait]
    impl crate::schema::fetcher::AsyncSchemaFetcher for NoopAsync {
        async fn fetch(&self, url: &str) -> Result<FetchResult> {
            Err(crate::schema::fetcher::error::FetchError::RequestFailed {
                url: url.to_string(),
                message: "offline".to_string(),
            }
            .into())
        }
    }
}
