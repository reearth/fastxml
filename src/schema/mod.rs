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

pub mod builder;
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
pub use validator::{
    LazySchemaValidator, OnePassSchemaValidator, StreamValidator, ValidationMode,
    XmlSchemaValidationContext, create_xml_schema_validation_context,
    create_xml_schema_validation_context_from_buffer, get_schema_from_schema_location_with_fetcher,
    streaming_validate_with_schema_location_and_fetcher, validate_document_by_schema,
    validate_document_by_schema_context, validate_with_schema_location_and_fetcher,
};

#[cfg(feature = "ureq")]
pub use validator::{
    get_schema_from_schema_location, streaming_validate_with_schema_location,
    validate_with_schema_location,
};

#[cfg(feature = "tokio")]
pub use validator::{
    get_schema_from_schema_location_async, get_schema_from_schema_location_with_async_fetcher,
    validate_with_schema_location_async, validate_with_schema_location_with_async_fetcher,
};

#[cfg(feature = "ureq")]
pub use fetcher::UreqFetcher;

#[cfg(feature = "tokio")]
pub use fetcher::{
    AsyncCachingFetcher, AsyncDefaultFetcher, AsyncFileCachingFetcher, AsyncFileFetcher,
    AsyncSchemaFetcher, ReqwestFetcher,
};

// Re-export XSD parsing functions
pub use xsd::{
    create_builtin_schema, parse_xsd, parse_xsd_multiple, parse_xsd_with_imports,
    parse_xsd_with_imports_multiple,
};

#[cfg(feature = "tokio")]
pub use xsd::{parse_xsd_with_imports_async, parse_xsd_with_imports_multiple_async};

// Re-export schema resolution functions
pub use resolve::{
    ResolveOptions, ResolvedSchema, resolve_schema_from_file, resolve_schema_from_xml,
};
