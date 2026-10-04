//! XSD Import/Include resolver.
//!
//! This module handles resolving xs:import, xs:include and xs:redefine
//! dependencies, fetching each referenced schema document through a
//! [`SchemaFetcher`](crate::schema::fetcher::SchemaFetcher) (caching, if
//! any, is the fetcher's job).
//!
//! # Architecture
//!
//! The resolver uses a BFS (breadth-first search) approach to resolve dependencies:
//!
//! 1. Parse the entry schema
//! 2. Queue its imports and includes
//! 3. For each dependency, fetch, parse, and queue its dependencies
//!    (each URI is fetched once; mutual imports are allowed)
//! 4. Return all schemas: `resolve_all` puts the entry last,
//!    `take_all_schemas` returns them in discovery order (entry first)
//!
//! # Sync vs Async
//!
//! Two implementations are provided:
//!
//! - [`SchemaResolver`]: Synchronous resolver using `SchemaFetcher`
//! - [`AsyncSchemaResolver`]: Async resolver using `AsyncSchemaFetcher`
//!   (requires `tokio` feature)
//!
//! Both resolve relative locations with the shared [`resolve_uri`].

mod common;
mod sync;

#[cfg(feature = "tokio")]
mod async_resolver;

// Re-exports
pub use common::{DependencyTracker, resolve_schemas_from_content, resolve_uri};
pub use sync::SchemaResolver;

#[cfg(feature = "tokio")]
pub use async_resolver::AsyncSchemaResolver;
