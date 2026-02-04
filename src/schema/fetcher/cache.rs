//! Caching fetcher wrapper.
//!
//! Wraps any `SchemaFetcher` or `AsyncSchemaFetcher` with an in-memory cache
//! backed by `DashMap` so each URL is fetched at most once.

use dashmap::DashMap;

use crate::error::Result;

use super::result::FetchResult;
use super::traits::SchemaFetcher;

/// A fetcher wrapper that caches fetch results in memory.
///
/// When a URL is requested:
/// 1. Check the cache — if present, return the cached result.
/// 2. Otherwise delegate to the inner fetcher.
/// 3. Store the result under both the requested URL and the final URL
///    (if a redirect occurred).
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::fetcher::{CachingFetcher, NoopFetcher};
///
/// // Wrap any SchemaFetcher with caching (note: DefaultFetcher already has built-in caching)
/// let fetcher = CachingFetcher::new(NoopFetcher);
/// fetcher.seed("http://example.com/schema.xsd", b"<schema/>".to_vec());
/// let result = fetcher.fetch("http://example.com/schema.xsd")?;
/// ```
pub struct CachingFetcher<F: SchemaFetcher> {
    inner: F,
    cache: DashMap<String, FetchResult>,
}

impl<F: SchemaFetcher> CachingFetcher<F> {
    /// Creates a new caching fetcher wrapping the given inner fetcher.
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            cache: DashMap::new(),
        }
    }

    /// Pre-seeds the cache with content for a given URL.
    pub fn seed(&self, url: &str, content: Vec<u8>) {
        self.cache.insert(
            url.to_string(),
            FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            },
        );
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns a reference to the inner fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }
}

impl<F: SchemaFetcher> SchemaFetcher for CachingFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check cache
        if let Some(entry) = self.cache.get(url) {
            return Ok(entry.value().clone());
        }

        // Delegate to inner
        let result = self.inner.fetch(url)?;

        // Cache under both requested URL and final URL
        self.cache.insert(url.to_string(), result.clone());
        if result.final_url != url {
            self.cache.insert(result.final_url.clone(), result.clone());
        }

        Ok(result)
    }
}

/// Async version of [`CachingFetcher`].
#[cfg(feature = "tokio")]
pub struct AsyncCachingFetcher<F: super::traits::AsyncSchemaFetcher> {
    inner: F,
    cache: DashMap<String, FetchResult>,
}

#[cfg(feature = "tokio")]
impl<F: super::traits::AsyncSchemaFetcher> AsyncCachingFetcher<F> {
    /// Creates a new async caching fetcher wrapping the given inner fetcher.
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            cache: DashMap::new(),
        }
    }

    /// Pre-seeds the cache with content for a given URL.
    pub fn seed(&self, url: &str, content: Vec<u8>) {
        self.cache.insert(
            url.to_string(),
            FetchResult {
                content,
                final_url: url.to_string(),
                redirected: false,
            },
        );
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns a reference to the inner fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }
}

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl<F: super::traits::AsyncSchemaFetcher> super::traits::AsyncSchemaFetcher
    for AsyncCachingFetcher<F>
{
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Check cache
        if let Some(entry) = self.cache.get(url) {
            return Ok(entry.value().clone());
        }

        // Delegate to inner
        let result = self.inner.fetch(url).await?;

        // Cache under both requested URL and final URL
        self.cache.insert(url.to_string(), result.clone());
        if result.final_url != url {
            self.cache.insert(result.final_url.clone(), result.clone());
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

    #[test]
    fn test_caching_fetcher_caches_result() {
        let mut responses = HashMap::new();
        responses.insert(
            "http://example.com/a.xsd".to_string(),
            b"<schema/>".to_vec(),
        );
        let inner = TrackingFetcher::new(responses);

        let fetcher = CachingFetcher::new(inner);

        // First fetch
        let r1 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r1.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1);

        // Second fetch should come from cache
        let r2 = fetcher.fetch("http://example.com/a.xsd").unwrap();
        assert_eq!(r2.content, b"<schema/>");
        assert_eq!(fetcher.inner().call_count(), 1); // still 1
    }

    #[test]
    fn test_caching_fetcher_seed() {
        let fetcher = CachingFetcher::new(NoopFetcher);
        fetcher.seed("http://example.com/test.xsd", b"<seeded/>".to_vec());

        let result = fetcher.fetch("http://example.com/test.xsd").unwrap();
        assert_eq!(result.content, b"<seeded/>");
        assert_eq!(fetcher.len(), 1);
    }

    #[test]
    fn test_caching_fetcher_len_is_empty() {
        let fetcher = CachingFetcher::new(NoopFetcher);
        assert!(fetcher.is_empty());
        assert_eq!(fetcher.len(), 0);

        fetcher.seed("http://example.com/a.xsd", b"a".to_vec());
        assert!(!fetcher.is_empty());
        assert_eq!(fetcher.len(), 1);
    }
}
