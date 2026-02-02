//! Async default fetcher implementation.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::traits::AsyncSchemaFetcher;
use super::{FetchResult, FileFetcher, ReqwestFetcher, SchemaFetcher};

/// Async default schema fetcher with sensible defaults.
///
/// This fetcher combines local file support with async HTTP fetching.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::AsyncDefaultFetcher;
///
/// # async fn example() -> Result<(), fastxml::error::Error> {
/// // Create with default settings
/// let fetcher = AsyncDefaultFetcher::new()?;
///
/// // Create with a base directory for relative paths
/// let fetcher = AsyncDefaultFetcher::with_base_dir("/path/to/schemas")?;
///
/// // Fetch a schema
/// let result = fetcher.fetch("http://example.com/schema.xsd").await?;
/// # Ok(())
/// # }
/// ```
pub struct AsyncDefaultFetcher {
    file_fetcher: FileFetcher,
    http_fetcher: ReqwestFetcher,
}

impl AsyncDefaultFetcher {
    /// Creates a new async default fetcher.
    ///
    /// Combines:
    /// 1. [`FileFetcher`] - for local file:// URLs and paths
    /// 2. [`ReqwestFetcher`] - for HTTP/HTTPS URLs
    pub fn new() -> Result<Self> {
        Self::with_base_dir_option(None)
    }

    /// Creates an async default fetcher with a base directory for resolving relative paths.
    pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Result<Self> {
        Self::with_base_dir_option(Some(base_dir.as_ref().to_path_buf()))
    }

    fn with_base_dir_option(base_dir: Option<PathBuf>) -> Result<Self> {
        let file_fetcher = match base_dir {
            Some(dir) => FileFetcher::with_base_dir(dir),
            None => FileFetcher::new(),
        };

        let http_fetcher = ReqwestFetcher::new()?;

        Ok(Self {
            file_fetcher,
            http_fetcher,
        })
    }

    /// Fetches a schema asynchronously.
    ///
    /// Tries local file first, then falls back to HTTP.
    pub async fn fetch(&self, url: &str) -> Result<FetchResult> {
        // Try local file first
        if let Ok(result) = self.file_fetcher.fetch(url) {
            return Ok(result);
        }

        // Fall back to HTTP
        self.http_fetcher.fetch_async(url).await
    }
}

#[async_trait::async_trait]
impl AsyncSchemaFetcher for AsyncDefaultFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        AsyncDefaultFetcher::fetch(self, url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_default_fetcher_new() {
        let fetcher = AsyncDefaultFetcher::new();
        assert!(fetcher.is_ok());
    }

    #[test]
    fn test_async_default_fetcher_with_base_dir() {
        let fetcher = AsyncDefaultFetcher::with_base_dir("/tmp");
        assert!(fetcher.is_ok());
    }
}
