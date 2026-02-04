//! Schema fetching with redirect support.
//!
//! This module provides various fetcher implementations for loading schemas:
//!
//! - [`FileFetcher`] - Reads schemas from local filesystem
//! - [`CombinedFetcher`] - Chains multiple fetchers
//! - [`NoopFetcher`] - Always fails (for testing)
//! - [`UreqFetcher`] - Sync HTTP client (requires `ureq` feature)
//! - [`ReqwestFetcher`] - Async HTTP client (requires `tokio` feature)
//! - [`DefaultFetcher`] - Recommended sync default fetcher (requires `ureq` feature for HTTP)
//! - [`AsyncDefaultFetcher`] - Recommended async default fetcher (requires `tokio` feature)
//! - [`AsyncFileFetcher`] - Async local file fetcher (requires `tokio` feature)
//!
//! # Default Fetchers
//!
//! ## Sync (DefaultFetcher)
//!
//! The [`DefaultFetcher`] provides a sensible default configuration for sync usage:
//!
//! - With `ureq` feature: Combines [`FileFetcher`] and [`UreqFetcher`]
//! - Without `ureq` feature: Uses [`FileFetcher`] only
//!
//! ## Async (AsyncDefaultFetcher)
//!
//! The [`AsyncDefaultFetcher`] provides fully async fetching (requires `tokio` feature):
//!
//! - Combines [`AsyncFileFetcher`] (tokio::fs) and [`ReqwestFetcher`]
//!
//! # Examples
//!
//! ```no_run
//! use fastxml::schema::fetcher::{DefaultFetcher, SchemaFetcher};
//!
//! let fetcher = DefaultFetcher::new();
//! let result = fetcher.fetch("http://example.com/schema.xsd");
//! ```
//!
//! ```no_run
//! # #[cfg(feature = "tokio")]
//! use fastxml::schema::fetcher::AsyncDefaultFetcher;
//!
//! # #[cfg(feature = "tokio")]
//! # async fn example() -> Result<(), fastxml::error::Error> {
//! let fetcher = AsyncDefaultFetcher::new()?;
//! let result = fetcher.fetch("http://example.com/schema.xsd").await?;
//! # Ok(())
//! # }
//! ```

pub mod error;

mod cache;
mod combined;
mod file;
mod file_cache;
mod noop;
mod result;
mod traits;

#[cfg(feature = "tokio")]
mod async_default;

#[cfg(feature = "tokio")]
mod async_file;

#[cfg(feature = "tokio")]
mod reqwest;

#[cfg(feature = "ureq")]
mod ureq;

#[cfg(feature = "tokio")]
pub use cache::AsyncCachingFetcher;
pub use cache::CachingFetcher;
pub use combined::CombinedFetcher;
pub use file::FileFetcher;
pub use file_cache::FileCachingFetcher;
pub use noop::NoopFetcher;
pub use result::FetchResult;
pub use traits::SchemaFetcher;

#[cfg(feature = "tokio")]
pub use file_cache::AsyncFileCachingFetcher;

#[cfg(feature = "tokio")]
pub use traits::AsyncSchemaFetcher;

#[cfg(feature = "tokio")]
pub use self::async_default::AsyncDefaultFetcher;

#[cfg(feature = "tokio")]
pub use self::async_file::AsyncFileFetcher;

#[cfg(feature = "tokio")]
pub use self::reqwest::ReqwestFetcher;

#[cfg(feature = "ureq")]
pub use self::ureq::UreqFetcher;

use dashmap::DashMap;
use std::path::Path;

/// Default schema fetcher with sensible defaults and built-in caching.
///
/// This fetcher combines local file support with HTTP fetching (when available)
/// and includes an in-memory cache backed by `DashMap` so each URL is fetched
/// at most once.
///
/// # Configuration
///
/// - With `ureq` feature: Tries local files first, then HTTP
/// - Without `ureq` feature: Local files only
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{DefaultFetcher, SchemaFetcher};
///
/// // Create with default settings
/// let fetcher = DefaultFetcher::new();
///
/// // Create with a base directory for relative paths
/// let fetcher = DefaultFetcher::with_base_dir("/path/to/schemas");
///
/// // Fetch a schema (results are cached automatically)
/// let result = fetcher.fetch("schema.xsd")?;
/// // Second call returns from cache
/// let cached = fetcher.fetch("schema.xsd")?;
/// # Ok::<(), fastxml::error::Error>(())
/// ```
pub struct DefaultFetcher {
    inner: CombinedFetcher,
    cache: DashMap<String, FetchResult>,
}

impl DefaultFetcher {
    /// Creates a new default fetcher.
    ///
    /// With `ureq` feature enabled, this combines:
    /// 1. [`FileFetcher`] - for local file:// URLs and paths
    /// 2. [`UreqFetcher`] - for HTTP/HTTPS URLs
    ///
    /// Without `ureq` feature, only local files are supported.
    ///
    /// Results are cached in memory so the same URL is never fetched twice.
    pub fn new() -> Self {
        Self::with_base_dir_option(None)
    }

    /// Creates a default fetcher with a base directory for resolving relative paths.
    pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
        Self::with_base_dir_option(Some(base_dir.as_ref().to_path_buf()))
    }

    fn with_base_dir_option(base_dir: Option<std::path::PathBuf>) -> Self {
        let file_fetcher = match base_dir {
            Some(dir) => FileFetcher::with_base_dir(dir),
            None => FileFetcher::new(),
        };

        #[cfg(feature = "ureq")]
        let inner = CombinedFetcher::new()
            .with_fetcher(file_fetcher)
            .with_fetcher(UreqFetcher::new());

        #[cfg(not(feature = "ureq"))]
        let inner = CombinedFetcher::new().with_fetcher(file_fetcher);

        Self {
            inner,
            cache: DashMap::new(),
        }
    }

    /// Sets the timeout for HTTP requests (only effective with `ureq` feature).
    #[cfg(feature = "ureq")]
    pub fn timeout(self, secs: u64) -> Self {
        let file_fetcher = FileFetcher::new();
        let inner = CombinedFetcher::new()
            .with_fetcher(file_fetcher)
            .with_fetcher(UreqFetcher::new().timeout(secs));
        Self {
            inner,
            cache: self.cache,
        }
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for DefaultFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaFetcher for DefaultFetcher {
    fn fetch(&self, url: &str) -> crate::error::Result<FetchResult> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_fetcher() {
        let fetcher = NoopFetcher;
        let result = fetcher.fetch("http://example.com/schema.xsd");
        assert!(result.is_err());
    }

    #[test]
    fn test_noop_fetcher_error_message() {
        let fetcher = NoopFetcher;
        let result = fetcher.fetch("http://example.com/test.xsd");
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("network"));
        assert!(msg.contains("http://example.com/test.xsd"));
    }

    #[test]
    fn test_fetch_result_struct() {
        let result = FetchResult {
            content: vec![1, 2, 3],
            final_url: "http://example.com/final.xsd".to_string(),
            redirected: true,
        };
        assert_eq!(result.content, vec![1, 2, 3]);
        assert_eq!(result.final_url, "http://example.com/final.xsd");
        assert!(result.redirected);
    }

    #[test]
    fn test_fetch_result_no_redirect() {
        let result = FetchResult {
            content: b"<schema/>".to_vec(),
            final_url: "http://example.com/schema.xsd".to_string(),
            redirected: false,
        };
        assert_eq!(result.content, b"<schema/>");
        assert!(!result.redirected);
    }

    #[test]
    fn test_fetch_result_clone() {
        let result = FetchResult {
            content: vec![42],
            final_url: "http://example.com".to_string(),
            redirected: false,
        };
        let cloned = result.clone();
        assert_eq!(cloned.content, result.content);
        assert_eq!(cloned.final_url, result.final_url);
        assert_eq!(cloned.redirected, result.redirected);
    }

    #[test]
    fn test_fetch_result_debug() {
        let result = FetchResult {
            content: vec![],
            final_url: "http://test.com".to_string(),
            redirected: true,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("FetchResult"));
        assert!(debug.contains("http://test.com"));
        assert!(debug.contains("true"));
    }

    #[test]
    fn test_default_fetcher_new() {
        let _fetcher = DefaultFetcher::new();
    }

    #[test]
    fn test_default_fetcher_default() {
        let _fetcher = DefaultFetcher::default();
    }

    #[test]
    fn test_default_fetcher_with_base_dir() {
        let _fetcher = DefaultFetcher::with_base_dir("/tmp");
    }

    #[cfg(feature = "ureq")]
    #[test]
    fn test_default_fetcher_timeout() {
        let _fetcher = DefaultFetcher::new().timeout(60);
    }
}
