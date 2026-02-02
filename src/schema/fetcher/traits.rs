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
