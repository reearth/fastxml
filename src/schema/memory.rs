//! In-memory schema store.

use std::path::PathBuf;

use dashmap::DashMap;

use super::store::SchemaStore;
use crate::error::Result;

/// Schema store using in-memory storage.
///
/// Useful for testing or when schemas are small and don't need
/// to be persisted to disk.
pub struct InMemoryStore {
    /// Maps URIs to schema content
    schemas: DashMap<String, Vec<u8>>,
    /// Base path for resolve_path (returns virtual paths)
    base_path: PathBuf,
}

impl InMemoryStore {
    /// Creates a new in-memory store.
    pub fn new() -> Self {
        Self {
            schemas: DashMap::new(),
            base_path: PathBuf::from("/virtual/schemas"),
        }
    }

    /// Creates a store with a custom base path.
    pub fn with_base_path(base_path: impl Into<PathBuf>) -> Self {
        Self {
            schemas: DashMap::new(),
            base_path: base_path.into(),
        }
    }

    /// Returns the number of stored schemas.
    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    /// Returns true if no schemas are stored.
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    /// Returns the total size of all stored schemas in bytes.
    pub fn total_size(&self) -> usize {
        self.schemas.iter().map(|r| r.value().len()).sum()
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaStore for InMemoryStore {
    fn get(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.schemas.get(uri).map(|v| v.value().clone()))
    }

    fn put(&self, uri: &str, content: &[u8]) -> Result<()> {
        self.schemas.insert(uri.to_string(), content.to_vec());
        Ok(())
    }

    fn contains(&self, uri: &str) -> bool {
        self.schemas.contains_key(uri)
    }

    fn resolve_path(&self, uri: &str) -> Result<PathBuf> {
        // Generate a virtual path based on the URI
        let hash = xxhash_rust::xxh64::xxh64(uri.as_bytes(), 0);
        Ok(self.base_path.join(format!("{:016x}.xsd", hash)))
    }

    fn list(&self) -> Result<Vec<String>> {
        Ok(self.schemas.iter().map(|r| r.key().clone()).collect())
    }

    fn remove(&self, uri: &str) -> Result<bool> {
        Ok(self.schemas.remove(uri).is_some())
    }

    fn clear(&self) -> Result<()> {
        self.schemas.clear();
        Ok(())
    }
}

#[cfg(feature = "async")]
mod async_impl {
    use super::*;
    use crate::schema::store::AsyncSchemaStore;

    #[async_trait::async_trait]
    impl AsyncSchemaStore for InMemoryStore {
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
    fn test_memory_store() {
        let store = InMemoryStore::new();

        let uri = "http://example.com/schema.xsd";
        let content = b"<schema/>";

        store.put(uri, content).unwrap();
        assert!(store.contains(uri));
        assert_eq!(store.len(), 1);

        let retrieved = store.get(uri).unwrap().unwrap();
        assert_eq!(retrieved, content);

        store.remove(uri).unwrap();
        assert!(!store.contains(uri));
        assert!(store.is_empty());
    }
}
