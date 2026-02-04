//! Combined fetcher implementation.

use super::error::FetchError;
use crate::error::Result;

use super::{FetchResult, SchemaFetcher};

/// A fetcher that tries multiple fetchers in order.
///
/// Useful for combining local file and HTTP fetching.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{CombinedFetcher, FileFetcher, SchemaFetcher};
/// # #[cfg(feature = "ureq")]
/// use fastxml::schema::UreqFetcher;
///
/// // Create a combined fetcher that tries local files first, then HTTP
/// # #[cfg(feature = "ureq")]
/// let fetcher = CombinedFetcher::new()
///     .with_fetcher(FileFetcher::with_base_dir("/local/schemas"))
///     .with_fetcher(UreqFetcher::new());
/// ```
pub struct CombinedFetcher {
    fetchers: Vec<Box<dyn SchemaFetcher>>,
}

impl CombinedFetcher {
    /// Creates a new empty combined fetcher.
    pub fn new() -> Self {
        Self {
            fetchers: Vec::new(),
        }
    }

    /// Adds a fetcher to the chain.
    pub fn with_fetcher<F: SchemaFetcher + 'static>(mut self, fetcher: F) -> Self {
        self.fetchers.push(Box::new(fetcher));
        self
    }
}

impl Default for CombinedFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaFetcher for CombinedFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let mut last_error = None;

        for fetcher in &self.fetchers {
            match fetcher.fetch(url) {
                Ok(result) => return Ok(result),
                Err(e) => last_error = Some(e),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            FetchError::RequestFailed {
                url: url.to_string(),
                message: "No fetchers configured".to_string(),
            }
            .into()
        }))
    }
}
