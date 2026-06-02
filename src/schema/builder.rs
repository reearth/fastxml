//! Ergonomic constructors for [`Schema`].
//!
//! `Schema` is an alias for [`CompiledSchema`]. These constructors are the
//! redesigned entry points for building a compiled schema; they wrap the
//! lower-level `parse_xsd*` / `create_builtin_schema` functions in
//! [`crate::schema::xsd`] behind a single consistent surface.
//!
//! ```ignore
//! use fastxml::schema::Schema;
//!
//! let schema = Schema::from_xsd(xsd_bytes)?;          // single document
//! let schema = Schema::builtin();                     // built-in types only
//! let schema = Schema::builder()                      // multiple sources
//!     .add("http://example.com/types.xsd", types_bytes)
//!     .add("http://example.com/main.xsd", main_bytes)
//!     .resolve()?;
//! ```

use crate::error::Result;
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::types::CompiledSchema;
use crate::schema::xsd::{
    create_builtin_schema, parse_xsd, parse_xsd_multiple, parse_xsd_with_imports_multiple,
};

#[cfg(feature = "tokio")]
use crate::schema::fetcher::AsyncSchemaFetcher;
#[cfg(feature = "tokio")]
use crate::schema::xsd::parse_xsd_with_imports_multiple_async;

/// A compiled XSD schema, ready for validation.
///
/// `Schema` is an alias for [`CompiledSchema`]; the redesigned API uses this
/// shorter name together with the constructors [`Schema::from_xsd`],
/// [`Schema::builtin`], and [`Schema::builder`].
pub type Schema = CompiledSchema;

impl CompiledSchema {
    /// Compiles a single XSD document, without resolving `xs:import` /
    /// `xs:include` dependencies.
    ///
    /// Built-in XSD and GML types are registered automatically. To resolve
    /// imports, or to combine several sources, use [`Schema::builder`].
    pub fn from_xsd(content: impl AsRef<[u8]>) -> Result<Self> {
        parse_xsd(content.as_ref())
    }

    /// Creates a schema with only the built-in XSD primitive and GML types
    /// registered (no user-defined elements).
    pub fn builtin() -> Self {
        create_builtin_schema()
    }

    /// Starts building a schema from one or more XSD sources.
    ///
    /// See [`SchemaBuilder`].
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }
}

/// Builds a [`Schema`] from one or more XSD sources.
///
/// Add entry sources with [`add`](Self::add), then finish with
/// [`resolve`](Self::resolve) (all dependencies supplied locally) or
/// [`resolve_with`](Self::resolve_with) (resolve `xs:import` / `xs:include`
/// through a [`SchemaFetcher`]).
#[derive(Debug, Default, Clone)]
pub struct SchemaBuilder {
    entries: Vec<(String, Vec<u8>)>,
}

impl SchemaBuilder {
    /// Creates an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry XSD source, keyed by its URI.
    ///
    /// The URI is used as the base for resolving that source's relative
    /// imports, and to deduplicate shared dependencies across entries.
    pub fn add(mut self, uri: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        self.entries.push((uri.into(), content.into()));
        self
    }

    fn refs(&self) -> Vec<(&str, &[u8])> {
        self.entries
            .iter()
            .map(|(uri, content)| (uri.as_str(), content.as_slice()))
            .collect()
    }

    /// Compiles all added sources together, without fetching imports — every
    /// dependency must already have been supplied via [`add`](Self::add).
    pub fn resolve(self) -> Result<Schema> {
        parse_xsd_multiple(&self.refs())
    }

    /// Compiles all added sources, resolving `xs:import` / `xs:include`
    /// dependencies through `fetcher`. Shared dependencies are fetched once.
    pub fn resolve_with<F: SchemaFetcher>(self, fetcher: &F) -> Result<Schema> {
        parse_xsd_with_imports_multiple(&self.refs(), fetcher)
    }

    /// Async version of [`resolve_with`](Self::resolve_with).
    #[cfg(feature = "tokio")]
    pub async fn resolve_with_async<F: AsyncSchemaFetcher>(self, fetcher: &F) -> Result<Schema> {
        parse_xsd_with_imports_multiple_async(&self.refs(), fetcher).await
    }
}
