//! Async local file fetcher implementation.

use std::path::{Path, PathBuf};

use super::error::FetchError;
use crate::error::Result;

use super::FetchResult;
use super::traits::AsyncSchemaFetcher;

/// An async fetcher that reads schemas from the local filesystem using tokio.
///
/// Supports both `file://` URLs and regular file paths.
/// Can optionally resolve relative paths against a base directory.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::AsyncFileFetcher;
/// use fastxml::schema::fetcher::AsyncSchemaFetcher;
///
/// # async fn example() -> Result<(), fastxml::error::Error> {
/// // Create a fetcher without a base directory
/// let fetcher = AsyncFileFetcher::new();
///
/// // Create a fetcher with a base directory for resolving relative paths
/// let fetcher = AsyncFileFetcher::with_base_dir("/path/to/schemas");
///
/// // Fetch a local file
/// let result = fetcher.fetch("file:///path/to/schema.xsd").await?;
/// # Ok(())
/// # }
/// ```
pub struct AsyncFileFetcher {
    base_dir: Option<PathBuf>,
}

impl AsyncFileFetcher {
    /// Creates a new async file fetcher without a base directory.
    ///
    /// Only absolute paths and `file://` URLs will work.
    pub fn new() -> Self {
        Self { base_dir: None }
    }

    /// Creates an async file fetcher with a base directory for resolving relative paths.
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
    async fn resolve_path(&self, url: &str) -> Option<PathBuf> {
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
            if tokio::fs::try_exists(&resolved).await.unwrap_or(false) {
                return Some(resolved);
            }
        }

        // Try as-is (might be a relative path from current directory)
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            return Some(path.to_path_buf());
        }

        None
    }

    /// Fetches a schema from the local filesystem asynchronously.
    pub async fn fetch(&self, url: &str) -> Result<FetchResult> {
        let path = self
            .resolve_path(url)
            .await
            .ok_or_else(|| FetchError::RequestFailed {
                url: url.to_string(),
                message: "Cannot resolve file path".to_string(),
            })?;

        let content = tokio::fs::read(&path)
            .await
            .map_err(|e| FetchError::RequestFailed {
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

impl Default for AsyncFileFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AsyncSchemaFetcher for AsyncFileFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        AsyncFileFetcher::fetch(self, url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_async_file_fetcher_absolute_path() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch(path).await.unwrap();

        assert!(result.content.starts_with(b"test content"));
        assert!(result.final_url.starts_with("file://"));
        assert!(!result.redirected);
    }

    #[tokio::test]
    async fn test_async_file_fetcher_file_url() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();
        let file_url = format!("file://{}", path);

        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch(&file_url).await.unwrap();

        assert!(result.content.starts_with(b"test content"));
    }

    #[tokio::test]
    async fn test_async_file_fetcher_with_base_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("schema.xsd");
        std::fs::write(&file_path, "schema content").unwrap();

        let fetcher = AsyncFileFetcher::with_base_dir(temp_dir.path());
        let result = fetcher.fetch("schema.xsd").await.unwrap();

        assert_eq!(result.content, b"schema content");
    }

    #[tokio::test]
    async fn test_async_file_fetcher_not_found() {
        let fetcher = AsyncFileFetcher::new();
        let result = fetcher.fetch("/nonexistent/path/schema.xsd").await;

        assert!(result.is_err());
    }
}
