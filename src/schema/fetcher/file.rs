//! Local file fetcher implementation.

use std::path::{Path, PathBuf};

use super::error::FetchError;
use crate::error::Result;

use super::{FetchResult, SchemaFetcher};

/// A fetcher that reads schemas from the local filesystem.
///
/// Supports both `file://` URLs and regular file paths.
/// Can optionally resolve relative paths against a base directory.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{FileFetcher, SchemaFetcher};
///
/// // Create a fetcher without a base directory
/// let fetcher = FileFetcher::new();
///
/// // Create a fetcher with a base directory for resolving relative paths
/// let fetcher = FileFetcher::with_base_dir("/path/to/schemas");
///
/// // Fetch a local file
/// let result = fetcher.fetch("file:///path/to/schema.xsd");
/// ```
pub struct FileFetcher {
    base_dir: Option<PathBuf>,
}

impl FileFetcher {
    /// Creates a new file fetcher without a base directory.
    ///
    /// Only absolute paths and `file://` URLs will work.
    pub fn new() -> Self {
        Self { base_dir: None }
    }

    /// Creates a file fetcher with a base directory for resolving relative paths.
    pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: Some(base_dir.as_ref().to_path_buf()),
        }
    }

    /// Sets the base directory for resolving relative paths.
    pub fn base_dir(mut self, base_dir: impl AsRef<Path>) -> Self {
        self.base_dir = Some(base_dir.as_ref().to_path_buf());
        self
    }

    /// Resolves a URL or path to an absolute file path.
    fn resolve_path(&self, url: &str) -> Option<PathBuf> {
        // Handle file:// URLs
        if let Some(path) = url.strip_prefix("file://") {
            return Some(PathBuf::from(path));
        }

        // Handle absolute paths
        let path = Path::new(url);
        if path.is_absolute() {
            return Some(path.to_path_buf());
        }

        // Handle relative paths with base directory
        if let Some(ref base) = self.base_dir {
            let resolved = base.join(url);
            if resolved.exists() {
                return Some(resolved);
            }
        }

        // Try as-is (might be a relative path from current directory)
        if path.exists() {
            return Some(path.to_path_buf());
        }

        None
    }
}

impl Default for FileFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaFetcher for FileFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let path = self
            .resolve_path(url)
            .ok_or_else(|| FetchError::RequestFailed {
                url: url.to_string(),
                message: "Cannot resolve file path".to_string(),
            })?;

        let content = std::fs::read(&path).map_err(|e| FetchError::RequestFailed {
            url: url.to_string(),
            message: e.to_string(),
        })?;

        let final_url = format!("file://{}", path.display());

        Ok(FetchResult {
            content,
            final_url,
            redirected: false,
        })
    }
}
