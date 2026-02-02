//! Schema fetcher trait.

use crate::error::Result;

use super::FetchResult;

/// Trait for fetching schemas from URLs.
pub trait SchemaFetcher: Send + Sync {
    /// Fetches a schema from the given URL.
    ///
    /// Follows redirects automatically and returns the final URL.
    fn fetch(&self, url: &str) -> Result<FetchResult>;
}

/// Async trait for fetching schemas from URLs.
#[cfg(feature = "tokio")]
#[async_trait::async_trait]
pub trait AsyncSchemaFetcher: Send + Sync {
    /// Fetches a schema from the given URL asynchronously.
    ///
    /// Follows redirects automatically and returns the final URL.
    async fn fetch(&self, url: &str) -> Result<FetchResult>;
}
