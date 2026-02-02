//! XSD Import/Include resolver.
//!
//! This module handles resolving xs:import and xs:include dependencies,
//! fetching remote schemas, and caching them.
//!
//! # Architecture
//!
//! The resolver uses a BFS (breadth-first search) approach to resolve dependencies:
//!
//! 1. Parse the entry schema
//! 2. Queue its imports and includes
//! 3. For each dependency, fetch (with caching), parse, and queue its dependencies
//! 4. Return all schemas in dependency order (dependencies first)
//!
//! # Sync vs Async
//!
//! Two implementations are provided:
//!
//! - [`SchemaResolver`]: Synchronous resolver using `SchemaFetcher` and `SchemaStore`
//! - [`AsyncSchemaResolver`]: Async resolver using `AsyncSchemaFetcher` and `AsyncSchemaStore`
//!   (requires `tokio` feature)
//!
//! Both implementations share common helper functions from the `common` module.

mod common;
mod sync;

#[cfg(feature = "tokio")]
mod async_resolver;

// Re-exports
pub use common::{DependencyTracker, resolve_schemas_from_content, resolve_uri};
pub use sync::SchemaResolver;

#[cfg(feature = "tokio")]
pub use async_resolver::AsyncSchemaResolver;
