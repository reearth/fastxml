//! Async temporary directory-based schema store.

use std::path::PathBuf;

use dashmap::DashMap;
use tempfile::TempDir;

use super::store::AsyncSchemaStore;
use crate::error::{Error, Result};

/// Async schema store using a temporary directory with tokio::fs.
///
/// Schemas are stored as files in a temporary directory that is
/// automatically cleaned up when the store is dropped.
/// All file operations are performed asynchronously using tokio::fs.
///
/// # Example
///
/// ```no_run
/// use fastxml::schema::AsyncTempDirStore;
/// use fastxml::schema::store::AsyncSchemaStore;
///
/// # async fn example() -> fastxml::error::Result<()> {
/// let store = AsyncTempDirStore::new()?;
///
/// // Store a schema
/// store.put("http://example.com/schema.xsd", b"<schema/>").await?;
///
/// // Retrieve it
/// if let Some(content) = store.get("http://example.com/schema.xsd").await? {
///     println!("Got {} bytes", content.len());
/// }
/// # Ok(())
/// # }
/// ```
pub struct AsyncTempDirStore {
    /// The temporary directory
    dir: TempDir,
    /// Maps URIs to file paths within the directory
    index: DashMap<String, PathBuf>,
}

impl AsyncTempDirStore {
    /// Creates a new async temporary directory store.
    pub fn new() -> Result<Self> {
        let dir = TempDir::new().map_err(Error::Io)?;

        Ok(Self {
            dir,
            index: DashMap::new(),
        })
    }

    /// Creates a store with a custom prefix for the directory name.
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        let dir = TempDir::with_prefix(prefix).map_err(Error::Io)?;

        Ok(Self {
            dir,
            index: DashMap::new(),
        })
    }

    /// Returns the path to the temporary directory.
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Returns the number of stored schemas.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns true if no schemas are stored.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns the total size of all stored schemas in bytes.
    pub async fn total_size(&self) -> usize {
        let mut total = 0;
        for entry in self.index.iter() {
            if let Ok(metadata) = tokio::fs::metadata(entry.value()).await {
                total += metadata.len() as usize;
            }
        }
        total
    }

    /// Generates a safe filename from a URI.
    fn uri_to_filename(&self, uri: &str) -> String {
        // Convert URI to a safe filename
        let hash = xxhash_rust::xxh64::xxh64(uri.as_bytes(), 0);
        let extension = uri
            .rsplit('.')
            .next()
            .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()))
            .unwrap_or("xsd");

        format!("{:016x}.{}", hash, extension)
    }
}

#[async_trait::async_trait]
impl AsyncSchemaStore for AsyncTempDirStore {
    async fn get(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        match self.index.get(uri) {
            Some(path) => {
                let content = tokio::fs::read(path.value()).await?;
                Ok(Some(content))
            }
            None => Ok(None),
        }
    }

    async fn put(&self, uri: &str, content: &[u8]) -> Result<()> {
        let filename = self.uri_to_filename(uri);
        let path = self.dir.path().join(&filename);

        tokio::fs::write(&path, content).await?;
        self.index.insert(uri.to_string(), path);

        Ok(())
    }

    async fn contains(&self, uri: &str) -> bool {
        self.index.contains_key(uri)
    }

    async fn resolve_path(&self, uri: &str) -> Result<PathBuf> {
        match self.index.get(uri) {
            Some(path) => Ok(path.value().clone()),
            None => {
                // Generate path even if not yet stored
                let filename = self.uri_to_filename(uri);
                Ok(self.dir.path().join(filename))
            }
        }
    }

    async fn list(&self) -> Result<Vec<String>> {
        Ok(self.index.iter().map(|r| r.key().clone()).collect())
    }

    async fn remove(&self, uri: &str) -> Result<bool> {
        match self.index.remove(uri) {
            Some((_, path)) => {
                if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                    tokio::fs::remove_file(&path).await?;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn clear(&self) -> Result<()> {
        for entry in self.index.iter() {
            let path = entry.value();
            if tokio::fs::try_exists(path).await.unwrap_or(false) {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        self.index.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_async_tempdir_store_put_get() {
        let store = AsyncTempDirStore::new().unwrap();

        let uri = "http://example.com/schema.xsd";
        let content = b"<schema/>";
        store.put(uri, content).await.unwrap();

        assert!(store.contains(uri).await);

        let retrieved = store.get(uri).await.unwrap().unwrap();
        assert_eq!(retrieved, content);
    }

    #[tokio::test]
    async fn test_async_tempdir_store_resolve_path() {
        let store = AsyncTempDirStore::new().unwrap();

        let uri = "http://example.com/schema.xsd";
        let content = b"<schema/>";
        store.put(uri, content).await.unwrap();

        let path = store.resolve_path(uri).await.unwrap();
        assert!(tokio::fs::try_exists(&path).await.unwrap());
    }

    #[tokio::test]
    async fn test_async_tempdir_store_remove() {
        let store = AsyncTempDirStore::new().unwrap();

        let uri = "http://example.com/schema.xsd";
        store.put(uri, b"content").await.unwrap();

        assert!(store.contains(uri).await);
        assert!(store.remove(uri).await.unwrap());
        assert!(!store.contains(uri).await);
    }

    #[tokio::test]
    async fn test_async_tempdir_store_list_and_clear() {
        let store = AsyncTempDirStore::new().unwrap();

        store.put("http://a.com/1.xsd", b"1").await.unwrap();
        store.put("http://b.com/2.xsd", b"2").await.unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);

        store.clear().await.unwrap();
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_async_tempdir_store_total_size() {
        let store = AsyncTempDirStore::new().unwrap();

        store.put("http://a.com/1.xsd", b"hello").await.unwrap();
        store.put("http://b.com/2.xsd", b"world!").await.unwrap();

        let size = store.total_size().await;
        assert_eq!(size, 11); // "hello" (5) + "world!" (6)
    }

    #[tokio::test]
    async fn test_async_tempdir_store_with_prefix() {
        let store = AsyncTempDirStore::with_prefix("fastxml-test").unwrap();
        assert!(store.path().to_string_lossy().contains("fastxml-test"));
    }

    #[tokio::test]
    async fn test_async_tempdir_store_len_is_empty() {
        let store = AsyncTempDirStore::new().unwrap();

        assert!(store.is_empty());
        assert_eq!(store.len(), 0);

        store
            .put("http://example.com/test.xsd", b"test")
            .await
            .unwrap();

        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }
}
