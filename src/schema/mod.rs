//! XSD schema handling and validation.
//!
//! This module provides support for:
//! - Schema storage abstraction (memory, temp files, etc.)
//! - Schema fetching with redirect support
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
//!                          └─────────────> StreamingSchemaValidator
//! ```
//!
//! Both the document builder and schema validator receive the same events,
//! allowing single-pass parsing with validation.
//!
//! # Schema Store
//!
//! The [`SchemaStore`] trait allows flexible schema storage:
//!
//! - [`TempDirStore`] - Temporary directory (auto-cleanup)
//! - [`InMemoryStore`] - In-memory storage (testing)
//!
//! # Fetching
//!
//! The [`SchemaFetcher`] trait handles schema downloads:
//!
//! - `UreqFetcher` - Sync HTTP client (requires `sync` feature)
//! - `ReqwestFetcher` - Async HTTP client (requires `async` feature)
//! - `NoopFetcher` - No network access (testing)
//!
//! # Example
//!
//! ```ignore
//! use fastxml::schema::{TempDirStore, create_xml_schema_validation_context};
//!
//! // Create a schema store
//! let store = TempDirStore::new()?;
//!
//! // Create validation context
//! let ctx = create_xml_schema_validation_context("schema.xsd")?;
//!
//! // Validate a document
//! let errors = ctx.validate(&document)?;
//! ```

pub mod error;
pub mod fetch_error;
pub mod fetcher;
pub mod memory;
pub mod store;
pub mod tempdir;
pub mod types;
pub mod validator;
pub mod xsd;

// Re-export main types
pub use fetcher::{FetchResult, NoopFetcher, SchemaFetcher};
pub use memory::InMemoryStore;
pub use store::SchemaStore;
pub use tempdir::TempDirStore;
pub use types::{
    AttributeDef, CompiledSchema, ComplexType, ContentModel, ElementDef, ProcessContents,
    SimpleType, TypeDef,
};
pub use validator::{
    LazySchemaValidator, StreamingSchemaValidator, ValidationMode, XmlSchemaValidationContext,
    create_xml_schema_validation_context, create_xml_schema_validation_context_from_buffer,
    get_schema_from_schema_location_with_fetcher,
    streaming_validate_with_schema_location_and_fetcher, validate_document_by_schema,
    validate_document_by_schema_context, validate_with_schema_location_and_fetcher,
};

#[cfg(feature = "ureq")]
pub use validator::{
    get_schema_from_schema_location, streaming_validate_with_schema_location,
    validate_with_schema_location,
};

#[cfg(feature = "ureq")]
pub use fetcher::UreqFetcher;

#[cfg(feature = "reqwest")]
pub use fetcher::ReqwestFetcher;

#[cfg(feature = "async-trait")]
pub use store::AsyncSchemaStore;

// Re-export XSD parsing functions
pub use xsd::{create_builtin_schema, parse_xsd, parse_xsd_multiple, parse_xsd_with_imports};
