//! The unified [`Validator`] entry point and its [`Report`] result.
//!
//! `Validator` is the redesigned, consistent front door for schema validation.
//! It follows the same shape as the rest of the crate — `from(source)`, a few
//! builder setters, then `run()` — and folds the previously separate validator
//! types and the family of `validate_*` free functions behind one surface:
//!
//! ```ignore
//! use std::sync::Arc;
//! use fastxml::schema::{Schema, Validator};
//!
//! let schema = Arc::new(Schema::from_xsd(xsd_bytes)?);
//!
//! // Streaming validation against an explicit schema.
//! let report = Validator::from(xml).schema(Arc::clone(&schema)).run()?;
//!
//! // DOM validation (the input is just an `&XmlDocument`).
//! let report = Validator::from(&doc).schema(Arc::clone(&schema)).run()?;
//!
//! // Resolve the schema from the document's `xsi:schemaLocation`.
//! let report = Validator::from_reader(file).run()?;            // default fetcher (needs `ureq`)
//! let report = Validator::from(&doc).run_with(fetcher)?;       // custom fetcher
//! ```
//!
//! - The **input** selects the engine: `&XmlDocument` validates the DOM
//!   directly; `&str` / `&[u8]` / a reader validate via the streaming parser.
//! - The **schema** is either explicit ([`schema`](Validator::schema)) or, when
//!   omitted, resolved from `xsi:schemaLocation`.
//! - [`mode`](Validator::mode) / [`max_errors`](Validator::max_errors) apply to
//!   the explicit-schema path.

use std::io::BufRead;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::types::CompiledSchema;

use super::{DomSchemaValidator, OnePassSchemaValidator, ValidationMode};

/// The input to validate.
enum Source<'a> {
    /// A pre-parsed DOM document (validated directly, no re-parse).
    Dom(&'a XmlDocument),
    /// In-memory XML bytes (validated via the streaming parser).
    Bytes(&'a [u8]),
    /// Any buffered reader (validated via the streaming parser).
    Reader(Box<dyn BufRead + 'a>),
}

/// A consistent front door for schema validation.
///
/// `from(source)` → optional `schema` / `mode` / `max_errors` → `run()`.
/// The input type selects the engine (DOM vs streaming); see the type-level
/// examples on [`run`](Self::run) and [`run_with`](Self::run_with).
pub struct Validator<'a> {
    source: Source<'a>,
    schema: Option<Arc<CompiledSchema>>,
    mode: ValidationMode,
    max_errors: Option<usize>,
}

impl<'a> From<&'a XmlDocument> for Validator<'a> {
    fn from(doc: &'a XmlDocument) -> Self {
        Self::with_source(Source::Dom(doc))
    }
}

impl<'a> From<&'a str> for Validator<'a> {
    fn from(xml: &'a str) -> Self {
        Self::with_source(Source::Bytes(xml.as_bytes()))
    }
}

impl<'a> From<&'a [u8]> for Validator<'a> {
    fn from(xml: &'a [u8]) -> Self {
        Self::with_source(Source::Bytes(xml))
    }
}

impl<'a> Validator<'a> {
    fn with_source(source: Source<'a>) -> Self {
        Self {
            source,
            schema: None,
            mode: ValidationMode::default(),
            max_errors: None,
        }
    }

    /// Creates a validator that reads its input from any [`BufRead`].
    ///
    /// `from` cannot be used for readers because of Rust coherence
    /// (`From<&str>` and a blanket `From<R: BufRead>` cannot coexist).
    pub fn from_reader<R: BufRead + 'a>(reader: R) -> Self {
        Self::with_source(Source::Reader(Box::new(reader)))
    }

    /// Validates against an explicit, already-compiled [`Schema`](crate::schema::Schema).
    ///
    /// Without this, the schema is resolved from the document's
    /// `xsi:schemaLocation` at [`run`](Self::run) time.
    pub fn schema(mut self, schema: impl Into<Arc<CompiledSchema>>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Sets the validation mode (default: [`ValidationMode::Strict`]).
    pub fn mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Caps the number of collected errors (validation stops early once reached).
    pub fn max_errors(mut self, max: usize) -> Self {
        self.max_errors = Some(max);
        self
    }

    /// Runs validation and returns a [`Report`].
    ///
    /// With an explicit [`schema`](Self::schema), this never touches the
    /// network. Otherwise the schema is resolved from `xsi:schemaLocation`
    /// using the default fetcher, which requires the `ureq` feature — without
    /// it, use [`run_with`](Self::run_with) and pass a fetcher.
    pub fn run(self) -> Result<Report> {
        let Self {
            source,
            schema,
            mode,
            max_errors,
        } = self;
        let entries = match schema {
            Some(schema) => validate_with_schema(source, schema, mode, max_errors)?,
            None => run_location_default(source)?,
        };
        Ok(Report::new(entries))
    }

    /// Runs validation, resolving any `xsi:schemaLocation` through `fetcher`.
    ///
    /// When an explicit [`schema`](Self::schema) is set the fetcher is unused.
    pub fn run_with<F: SchemaFetcher + 'static>(self, fetcher: F) -> Result<Report> {
        let Self {
            source,
            schema,
            mode,
            max_errors,
        } = self;
        let entries = match schema {
            Some(schema) => validate_with_schema(source, schema, mode, max_errors)?,
            None => match source {
                Source::Dom(doc) => {
                    super::api::validate_with_schema_location_and_fetcher(doc, &fetcher)?
                }
                Source::Bytes(bytes) => {
                    super::api::streaming_validate_with_schema_location_and_fetcher(bytes, fetcher)?
                }
                Source::Reader(reader) => {
                    super::api::streaming_validate_with_schema_location_and_fetcher(
                        reader, fetcher,
                    )?
                }
            },
        };
        Ok(Report::new(entries))
    }

    /// Async version of [`run_with`](Self::run_with), resolving
    /// `xsi:schemaLocation` through an async `fetcher`.
    ///
    /// Async resolution is currently supported for an explicit schema (any
    /// input) and for the DOM + `xsi:schemaLocation` case; async streaming
    /// location resolution returns an error (use [`run_with`](Self::run_with)).
    #[cfg(feature = "tokio")]
    pub async fn run_async_with<F: crate::schema::fetcher::AsyncSchemaFetcher>(
        self,
        fetcher: &F,
    ) -> Result<Report> {
        let Self {
            source,
            schema,
            mode,
            max_errors,
        } = self;
        let entries = match schema {
            Some(schema) => validate_with_schema(source, schema, mode, max_errors)?,
            None => match source {
                Source::Dom(doc) => {
                    super::api::validate_with_schema_location_with_async_fetcher(doc, fetcher)
                        .await?
                }
                _ => {
                    return Err(crate::error::Error::InvalidOperation(
                        "async xsi:schemaLocation resolution is only supported for DOM input; \
                         use run_with for streaming input, or provide a schema"
                            .to_string(),
                    ));
                }
            },
        };
        Ok(Report::new(entries))
    }

    /// Async version of [`run`](Self::run), using the default async fetcher.
    #[cfg(feature = "tokio")]
    pub async fn run_async(self) -> Result<Report> {
        let fetcher = crate::schema::fetcher::AsyncDefaultFetcher::new()?;
        self.run_async_with(&fetcher).await
    }
}

fn validate_with_schema(
    source: Source<'_>,
    schema: Arc<CompiledSchema>,
    mode: ValidationMode,
    max_errors: Option<usize>,
) -> Result<Vec<StructuredError>> {
    match source {
        Source::Dom(doc) => {
            let mut validator = DomSchemaValidator::new(schema).with_mode(mode);
            if let Some(max) = max_errors {
                validator = validator.with_max_errors(max);
            }
            validator.validate(doc)
        }
        Source::Bytes(bytes) => run_streaming_with_schema(bytes, schema, mode, max_errors),
        Source::Reader(reader) => run_streaming_with_schema(reader, schema, mode, max_errors),
    }
}

fn run_streaming_with_schema<R: BufRead>(
    reader: R,
    schema: Arc<CompiledSchema>,
    mode: ValidationMode,
    max_errors: Option<usize>,
) -> Result<Vec<StructuredError>> {
    let mut validator = OnePassSchemaValidator::new(schema).set_mode(mode);
    if let Some(max) = max_errors {
        validator = validator.with_max_errors(max);
    }
    validator.validate(reader)
}

#[cfg(feature = "ureq")]
fn run_location_default(source: Source<'_>) -> Result<Vec<StructuredError>> {
    match source {
        Source::Dom(doc) => super::api::validate_with_schema_location(doc),
        Source::Bytes(bytes) => super::api::streaming_validate_with_schema_location(bytes),
        Source::Reader(reader) => super::api::streaming_validate_with_schema_location(reader),
    }
}

#[cfg(not(feature = "ureq"))]
fn run_location_default(_source: Source<'_>) -> Result<Vec<StructuredError>> {
    Err(crate::error::Error::InvalidOperation(
        "resolving xsi:schemaLocation with the default fetcher requires the `ureq` feature; \
         enable it, call .schema(...) with an explicit schema, or use .run_with(fetcher)"
            .to_string(),
    ))
}

/// The outcome of a validation run.
///
/// Wraps the collected [`StructuredError`]s (which may include warnings) with
/// convenience accessors. [`is_valid`](Report::is_valid) is the usual check.
#[derive(Debug, Clone)]
pub struct Report {
    entries: Vec<StructuredError>,
}

impl Report {
    fn new(entries: Vec<StructuredError>) -> Self {
        Self { entries }
    }

    /// True if there are no error-level entries (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.entries.iter().any(|e| e.is_error())
    }

    /// True if there are no entries at all (no errors and no warnings).
    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }

    /// All collected entries, errors and warnings alike.
    pub fn entries(&self) -> &[StructuredError] {
        &self.entries
    }

    /// Only the error-level entries (excludes warnings).
    pub fn errors(&self) -> Vec<&StructuredError> {
        self.entries.iter().filter(|e| e.is_error()).collect()
    }

    /// Only the warning-level entries.
    pub fn warnings(&self) -> Vec<&StructuredError> {
        self.entries.iter().filter(|e| e.is_warning()).collect()
    }

    /// Number of error-level entries.
    pub fn error_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_error()).count()
    }

    /// Number of warning-level entries.
    pub fn warning_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_warning()).count()
    }

    /// Consumes the report, returning all collected entries.
    pub fn into_entries(self) -> Vec<StructuredError> {
        self.entries
    }
}
