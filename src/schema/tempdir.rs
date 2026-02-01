//! Temporary directory-based schema store.

use std::fs;
use std::path::PathBuf;

use dashmap::DashMap;
use tempfile::TempDir;

use crate::error::{Error, Result};
use super::store::SchemaStore;

/// Schema store using a temporary directory.
///
/// Schemas are stored as files in a temporary directory that is
/// automatically cleaned up when the store is dropped.
pub struct TempDirStore {
    /// The temporary directory
    dir: TempDir,
    /// Maps URIs to file paths within the directory
    index: DashMap<String, PathBuf>,
}

impl TempDirStore {
    /// Creates a new temporary directory store.
    pub fn new() -> Result<Self> {
        let dir = TempDir::new()
            .map_err(Error::Io)?;

        Ok(Self {
            dir,
            index: DashMap::new(),
        })
    }

    /// Creates a store with a custom prefix for the directory name.
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        let dir = TempDir::with_prefix(prefix)
            .map_err(Error::Io)?;

        Ok(Self {
            dir,
            index: DashMap::new(),
        })
    }

    /// Returns the path to the temporary directory.
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Generates a safe filename from a URI.
    fn uri_to_filename(&self, uri: &str) -> String {
        // Convert URI to a safe filename
        let hash = xxhash_rust::xxh64::xxh64(uri.as_bytes(), 0);
        let extension = uri.rsplit('.').next()
            .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()))
            .unwrap_or("xsd");

        format!("{:016x}.{}", hash, extension)
    }
}

impl Default for TempDirStore {
    fn default() -> Self {
        Self::new().expect("failed to create temp directory")
    }
}

impl SchemaStore for TempDirStore {
    fn get(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        match self.index.get(uri) {
            Some(path) => {
                let content = fs::read(path.value())?;
                Ok(Some(content))
            }
            None => Ok(None),
        }
    }

    fn put(&self, uri: &str, content: &[u8]) -> Result<()> {
        let filename = self.uri_to_filename(uri);
        let path = self.dir.path().join(&filename);

        fs::write(&path, content)?;
        self.index.insert(uri.to_string(), path);

        Ok(())
    }

    fn contains(&self, uri: &str) -> bool {
        self.index.contains_key(uri)
    }

    fn resolve_path(&self, uri: &str) -> Result<PathBuf> {
        match self.index.get(uri) {
            Some(path) => Ok(path.value().clone()),
            None => {
                // Generate path even if not yet stored
                let filename = self.uri_to_filename(uri);
                Ok(self.dir.path().join(filename))
            }
        }
    }

    fn list(&self) -> Result<Vec<String>> {
        Ok(self.index.iter().map(|r| r.key().clone()).collect())
    }

    fn remove(&self, uri: &str) -> Result<bool> {
        match self.index.remove(uri) {
            Some((_, path)) => {
                if path.exists() {
                    fs::remove_file(&path)?;
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn clear(&self) -> Result<()> {
        for entry in self.index.iter() {
            let path = entry.value();
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        self.index.clear();
        Ok(())
    }
}

#[cfg(feature = "async")]
mod async_impl {
    use super::*;
    use crate::schema::store::AsyncSchemaStore;

    #[async_trait::async_trait]
    impl AsyncSchemaStore for TempDirStore {
        async fn get(&self, uri: &str) -> Result<Option<Vec<u8>>> {
            SchemaStore::get(self, uri)
        }

        async fn put(&self, uri: &str, content: &[u8]) -> Result<()> {
            SchemaStore::put(self, uri, content)
        }

        async fn contains(&self, uri: &str) -> bool {
            SchemaStore::contains(self, uri)
        }

        async fn resolve_path(&self, uri: &str) -> Result<PathBuf> {
            SchemaStore::resolve_path(self, uri)
        }

        async fn list(&self) -> Result<Vec<String>> {
            SchemaStore::list(self)
        }

        async fn remove(&self, uri: &str) -> Result<bool> {
            SchemaStore::remove(self, uri)
        }

        async fn clear(&self) -> Result<()> {
            SchemaStore::clear(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempdir_store() {
        let store = TempDirStore::new().unwrap();

        // Store and retrieve
        let uri = "http://example.com/schema.xsd";
        let content = b"<schema/>";
        store.put(uri, content).unwrap();

        assert!(store.contains(uri));

        let retrieved = store.get(uri).unwrap().unwrap();
        assert_eq!(retrieved, content);

        // Check path exists
        let path = store.resolve_path(uri).unwrap();
        assert!(path.exists());

        // Remove
        assert!(store.remove(uri).unwrap());
        assert!(!store.contains(uri));
    }

    #[test]
    fn test_list_and_clear() {
        let store = TempDirStore::new().unwrap();

        store.put("http://a.com/1.xsd", b"1").unwrap();
        store.put("http://b.com/2.xsd", b"2").unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);

        store.clear().unwrap();
        assert!(store.list().unwrap().is_empty());
    }
}
