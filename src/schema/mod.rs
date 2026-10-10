//! XSD schema handling and validation.
//!
//! This module provides support for:
//! - Schema fetching with redirect support and caching
//! - Schema compilation and type definitions ([`Schema`])
//! - Validation of DOM documents and streams ([`Validator`])
//!
//! # Validation
//!
//! [`Validator`] validates either a parsed [`XmlDocument`](crate::XmlDocument)
//! (DOM engine) or raw XML from a string, bytes or a reader (single-pass
//! streaming engine, suited to large files). The schema is given with
//! [`Validator::schema`] or loaded from the document's
//! `xsi:schemaLocation` / `xsi:noNamespaceSchemaLocation` hints.
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
pub(crate) mod hints;
pub mod resolve;
pub mod types;
pub mod validator;
pub mod xsd;

// Re-export the schema-construction and validation API
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
