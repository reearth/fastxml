//! Schema storage abstraction.

use std::path::PathBuf;

use crate::error::Result;

/// Trait for storing and retrieving XML schemas.
///
/// This abstraction allows schemas to be stored in various backends
/// (memory, temporary files, persistent storage, etc.).
pub trait SchemaStore: Send + Sync {
    /// Gets schema content by URI.
    ///
    /// Returns `Ok(None)` if the schema is not found.
    fn get(&self, uri: &str) -> Result<Option<Vec<u8>>>;

    /// Stores schema content.
    fn put(&self, uri: &str, content: &[u8]) -> Result<()>;

    /// Checks if a schema exists.
    fn contains(&self, uri: &str) -> bool;

    /// Resolves a URI to a local file path.
    ///
    /// This is useful for validators that require file paths.
    fn resolve_path(&self, uri: &str) -> Result<PathBuf>;

    /// Lists all stored schema URIs.
    fn list(&self) -> Result<Vec<String>>;

    /// Removes a schema by URI.
    fn remove(&self, uri: &str) -> Result<bool>;

    /// Clears all stored schemas.
    fn clear(&self) -> Result<()>;
}

/// Async version of SchemaStore.
#[cfg(feature = "async-trait")]
#[async_trait::async_trait]
pub trait AsyncSchemaStore: Send + Sync {
    /// Gets schema content by URI.
    async fn get(&self, uri: &str) -> Result<Option<Vec<u8>>>;

    /// Stores schema content.
    async fn put(&self, uri: &str, content: &[u8]) -> Result<()>;

    /// Checks if a schema exists.
    async fn contains(&self, uri: &str) -> bool;

    /// Resolves a URI to a local file path.
    async fn resolve_path(&self, uri: &str) -> Result<PathBuf>;

    /// Lists all stored schema URIs.
    async fn list(&self) -> Result<Vec<String>>;

    /// Removes a schema by URI.
    async fn remove(&self, uri: &str) -> Result<bool>;

    /// Clears all stored schemas.
    async fn clear(&self) -> Result<()>;
}
