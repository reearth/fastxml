//! No-op fetcher implementation.

use super::error::FetchError;
use crate::error::Result;

use super::{FetchResult, SchemaFetcher};

/// A no-op fetcher that always fails.
///
/// Useful for testing or when network access is disabled.
pub struct NoopFetcher;

impl SchemaFetcher for NoopFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        Err(FetchError::NetworkDisabled {
            url: url.to_string(),
        }
        .into())
    }
}
