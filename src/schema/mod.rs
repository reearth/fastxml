//! XSD schema handling and validation.
//!
//! This module provides support for:
//! - Schema fetching with redirect support and caching
//! - Schema compilation and type definitions
//! - Streaming validation
//!
//! # Architecture
//!
//! The schema system uses a SAX-like event sharing design for memory efficiency:
//!
//! ```text
//! XML Data
//!    │
//!    v
//! StreamingParser ─────────┬─────────────> DocumentBuilder
//!                          │
//!                          └─────────────> OnePassSchemaValidator
//! ```
//!
//! Both the document builder and schema validator receive the same events,
//! allowing single-pass parsing with validation.
//!
//! # Fetching
//!
//! The [`SchemaFetcher`] trait handles schema downloads:
//!
//! - `UreqFetcher` - Sync HTTP client (requires `ureq` feature)
//! - `ReqwestFetcher` - Async HTTP client (requires `tokio` feature)
//! - `NoopFetcher` - No network access (testing)
//! - [`CachingFetcher`] - Wraps any fetcher with in-memory caching
//!
//! # Example
//!
//! ```ignore
//! use fastxml::schema::DefaultFetcher;
//!
//! // DefaultFetcher has built-in caching
//! let fetcher = DefaultFetcher::new();
//! ```

mod builder;
pub mod error;
pub mod export;
pub mod fetcher;
pub mod resolve;
pub mod types;
pub mod validator;
pub mod xsd;

// Re-export the redesigned schema-construction and validation API
pub use builder::{Schema, SchemaBuilder};
pub use validator::{Report, Validator};

// Re-export main types
pub use fetcher::{
    CachingFetcher, CombinedFetcher, DefaultFetcher, FetchResult, FileCachingFetcher, FileFetcher,
    NoopFetcher, SchemaFetcher,
};
pub use types::{
    AttributeDef, CompiledSchema, ComplexType, ContentModel, ElementDef, ProcessContents,
    SimpleType, TypeDef,
};
pub use validator::ValidationMode;

#[cfg(feature = "ureq")]
pub use fetcher::UreqFetcher;

#[cfg(feature = "tokio")]
pub use fetcher::{
    AsyncCachingFetcher, AsyncDefaultFetcher, AsyncFileCachingFetcher, AsyncFileFetcher,
    AsyncSchemaFetcher, ReqwestFetcher,
};

// Re-export schema resolution functions
pub use resolve::{
    ResolveOptions, ResolvedSchema, resolve_schema_from_file, resolve_schema_from_xml,
};
