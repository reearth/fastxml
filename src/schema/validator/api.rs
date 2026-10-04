//! Validation against the schema named by the document's own hints.
//!
//! When [`Validator`](super::Validator) is run without an explicit schema,
//! the schema comes from the root element's `xsi:schemaLocation` /
//! `xsi:noNamespaceSchemaLocation` hints (see [`crate::schema::hints`]).
//! The policy, shared by the DOM and streaming engines:
//!
//! - Every hinted schema, and everything it imports, includes or redefines,
//!   must be fetched, parsed and compiled. If any step fails — or the root
//!   element carries no hint at all, or a hint is malformed — the report gets
//!   an error-level [`SchemaNotFound`](ValidationErrorType::SchemaNotFound)
//!   entry naming what failed, and the document content is **not** validated
//!   (a partial schema would only add misleading errors). The verdict is
//!   therefore never "valid" for a schema that was not loaded.
//! - Otherwise the document is validated with the caller's
//!   [`EngineSettings`] (mode, error cap, aggregation), exactly as with an
//!   explicit schema.

use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::hints::{SchemaHint, SchemaHints};
use crate::schema::types::CompiledSchema;

use super::ValidationMode;
use super::dom::DomSchemaValidator;
use super::lazy::AutodetectStreamingValidator;
use super::streaming::OnePassSchemaValidator;

/// The caller-chosen engine settings, applied identically to the explicit
/// and auto-detected schema paths.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineSettings {
    pub mode: ValidationMode,
    pub max_errors: Option<usize>,
    pub aggregate_errors: bool,
}

impl EngineSettings {
    pub fn dom(&self, schema: Arc<CompiledSchema>) -> DomSchemaValidator {
        let mut v = DomSchemaValidator::new(schema).with_mode(self.mode);
        if let Some(max) = self.max_errors {
            v = v.with_max_errors(max);
        }
        if self.aggregate_errors {
            v = v.with_aggregate_errors();
        }
        v
    }

    pub fn streaming(&self, schema: Arc<CompiledSchema>) -> OnePassSchemaValidator {
        let mut v = OnePassSchemaValidator::new(schema).set_mode(self.mode);
        if let Some(max) = self.max_errors {
            v = v.with_max_errors(max);
        }
        if self.aggregate_errors {
            v = v.with_aggregate_errors();
        }
        v
    }
}

const NOT_VALIDATED: &str = "the document content was not validated";

fn schema_unavailable(message: String) -> StructuredError {
    StructuredError::new(message, ValidationErrorType::SchemaNotFound).with_level(ErrorLevel::Error)
}

/// The root element names no schema.
pub(crate) fn no_hints_error() -> StructuredError {
    schema_unavailable(format!(
        "no schema to validate against: the root element has no xsi:schemaLocation or \
         xsi:noNamespaceSchemaLocation attribute (namespace {}); {NOT_VALIDATED}. \
         Add a hint to the document or pass a schema explicitly with Validator::schema(..)",
        crate::schema::hints::XSI_NAMESPACE
    ))
}

fn hint_problem_error(problem: &str) -> StructuredError {
    schema_unavailable(format!("{problem}; {NOT_VALIDATED}"))
}

fn fetch_failed_error(hint: &SchemaHint, e: &crate::error::Error) -> StructuredError {
    schema_unavailable(format!(
        "could not fetch schema '{}' named by the {}: {e}; {NOT_VALIDATED}. Check that the \
         location is reachable (a relative location is resolved by the fetcher, against its \
         base directory or else the current working directory), or pass a schema explicitly \
         with Validator::schema(..)",
        hint.location,
        hint.describe()
    ))
}

fn load_failed_error(hint: &SchemaHint, e: &crate::error::Error) -> StructuredError {
    schema_unavailable(format!(
        "could not load schema '{}' named by the {} (the schema or one of its \
         xs:import/xs:include/xs:redefine documents failed to fetch or parse): {e}; \
         {NOT_VALIDATED}",
        hint.location,
        hint.describe()
    ))
}

fn compile_failed_error(e: &crate::error::Error) -> StructuredError {
    schema_unavailable(format!(
        "could not compile the schemas named by the document's schema-location hints: {e}; \
         {NOT_VALIDATED}"
    ))
}

/// Errors for hints that are absent or malformed; empty when every hint can
/// be attempted.
fn hint_errors(hints: &SchemaHints) -> Vec<StructuredError> {
    if hints.is_empty() {
        return vec![no_hints_error()];
    }
    hints
        .problems
        .iter()
        .map(|p| hint_problem_error(p))
        .collect()
}

fn compile(
    schemas: Vec<crate::schema::xsd::types::XsdSchema>,
) -> std::result::Result<CompiledSchema, Vec<StructuredError>> {
    let mut compiled =
        crate::schema::xsd::compile_schemas(schemas).map_err(|e| vec![compile_failed_error(&e)])?;
    crate::schema::xsd::register_builtin_types(&mut compiled);
    Ok(compiled)
}

/// Loads every hinted schema with its dependencies and compiles them.
///
/// `Err` carries the error-level entries explaining why the schema is not
/// available; the caller must then skip content validation.
pub(crate) fn load_hinted_schema<F: SchemaFetcher>(
    hints: &SchemaHints,
    fetcher: &F,
) -> std::result::Result<CompiledSchema, Vec<StructuredError>> {
    let mut errors = hint_errors(hints);
    // One resolver for all hints, so shared dependencies are fetched once.
    let mut resolver = crate::schema::xsd::SchemaResolver::new(fetcher);
    for hint in &hints.hints {
        match fetcher.fetch(&hint.location) {
            Ok(fetched) => {
                if let Err(e) = resolver.resolve_entry(&fetched.content, &fetched.final_url) {
                    errors.push(load_failed_error(hint, &e));
                }
            }
            Err(e) => errors.push(fetch_failed_error(hint, &e)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    compile(resolver.take_all_schemas())
}

/// Async counterpart of [`load_hinted_schema`].
#[cfg(feature = "tokio")]
pub(crate) async fn load_hinted_schema_async<F: crate::schema::fetcher::AsyncSchemaFetcher>(
    hints: &SchemaHints,
    fetcher: &F,
) -> std::result::Result<CompiledSchema, Vec<StructuredError>> {
    let mut errors = hint_errors(hints);
    let mut resolver = crate::schema::xsd::AsyncSchemaResolver::new(fetcher);
    for hint in &hints.hints {
        match fetcher.fetch(&hint.location).await {
            Ok(fetched) => {
                if let Err(e) = resolver
                    .resolve_entry(&fetched.content, &fetched.final_url)
                    .await
                {
                    errors.push(load_failed_error(hint, &e));
                }
            }
            Err(e) => errors.push(fetch_failed_error(hint, &e)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    compile(resolver.take_all_schemas())
}

fn validate_dom_with(
    doc: &XmlDocument,
    loaded: std::result::Result<CompiledSchema, Vec<StructuredError>>,
    settings: EngineSettings,
) -> Result<Vec<StructuredError>> {
    match loaded {
        Ok(schema) => settings.dom(Arc::new(schema)).validate(doc),
        Err(errors) => Ok(errors),
    }
}

/// DOM validation against the schema named by the document's hints.
pub(crate) fn validate_dom_autodetect<F: SchemaFetcher>(
    doc: &XmlDocument,
    fetcher: &F,
    settings: EngineSettings,
) -> Result<Vec<StructuredError>> {
    let hints = SchemaHints::from_document(doc)?;
    validate_dom_with(doc, load_hinted_schema(&hints, fetcher), settings)
}

/// Async counterpart of [`validate_dom_autodetect`].
#[cfg(feature = "tokio")]
pub(crate) async fn validate_dom_autodetect_async<F: crate::schema::fetcher::AsyncSchemaFetcher>(
    doc: &XmlDocument,
    fetcher: &F,
    settings: EngineSettings,
) -> Result<Vec<StructuredError>> {
    let hints = SchemaHints::from_document(doc)?;
    validate_dom_with(
        doc,
        load_hinted_schema_async(&hints, fetcher).await,
        settings,
    )
}

/// Single-pass streaming validation against the schema named by the
/// document's hints: the schema is loaded when the root start tag arrives,
/// and the rest of the document streams through the validator.
pub(crate) fn validate_streaming_autodetect<R: std::io::BufRead, F: SchemaFetcher + 'static>(
    reader: R,
    fetcher: F,
    settings: EngineSettings,
) -> Result<Vec<StructuredError>> {
    use crate::event::StreamingParser;

    let mut parser = StreamingParser::new(reader);
    parser.add_handler(Box::new(AutodetectStreamingValidator::new(
        fetcher, settings,
    )));
    parser.parse()?;

    for handler in parser.into_handlers() {
        if let Ok(v) = handler
            .as_any()
            .downcast::<AutodetectStreamingValidator<F>>()
        {
            return Ok(v.into_entries());
        }
    }
    unreachable!("the auto-detecting validator was registered above")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> EngineSettings {
        EngineSettings {
            mode: ValidationMode::Strict,
            max_errors: None,
            aggregate_errors: false,
        }
    }

    const NO_HINT: &str = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

    fn assert_no_schema(errors: &[StructuredError]) {
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].is_error());
        assert_eq!(errors[0].error_type, ValidationErrorType::SchemaNotFound);
    }

    #[test]
    fn dom_without_hints_reports_missing_schema() {
        let doc = crate::parse(NO_HINT.as_bytes()).unwrap();
        let errors =
            validate_dom_autodetect(&doc, &crate::schema::fetcher::NoopFetcher, settings())
                .unwrap();
        assert_no_schema(&errors);
    }

    #[test]
    fn streaming_without_hints_reports_missing_schema() {
        let reader = std::io::BufReader::new(NO_HINT.as_bytes());
        let errors =
            validate_streaming_autodetect(reader, crate::schema::fetcher::NoopFetcher, settings())
                .unwrap();
        assert_no_schema(&errors);
    }
}

#[cfg(all(test, feature = "tokio"))]
mod async_tests {
    use super::*;
    use crate::schema::fetcher::{AsyncSchemaFetcher, FetchResult};
    use parking_lot::RwLock;
    use std::collections::HashMap as StdHashMap;

    /// Mock async fetcher for testing
    struct MockAsyncFetcher {
        responses: Arc<RwLock<StdHashMap<String, Vec<u8>>>>,
    }

    impl MockAsyncFetcher {
        fn new() -> Self {
            Self {
                responses: Arc::new(RwLock::new(StdHashMap::new())),
            }
        }

        fn add_response(&self, url: &str, content: &[u8]) {
            self.responses
                .write()
                .insert(url.to_string(), content.to_vec());
        }
    }

    #[async_trait::async_trait]
    impl AsyncSchemaFetcher for MockAsyncFetcher {
        async fn fetch(&self, url: &str) -> crate::error::Result<FetchResult> {
            let responses = self.responses.read();
            if let Some(content) = responses.get(url) {
                Ok(FetchResult {
                    content: content.clone(),
                    final_url: url.to_string(),
                    redirected: false,
                })
            } else {
                Err(crate::schema::fetcher::error::FetchError::RequestFailed {
                    url: url.to_string(),
                    message: "Not found".to_string(),
                }
                .into())
            }
        }
    }

    fn settings() -> EngineSettings {
        EngineSettings {
            mode: ValidationMode::Strict,
            max_errors: None,
            aggregate_errors: false,
        }
    }

    #[tokio::test]
    async fn async_without_hints_reports_missing_schema() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = MockAsyncFetcher::new();

        let errors = validate_dom_autodetect_async(&doc, &fetcher, settings())
            .await
            .unwrap();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].error_type, ValidationErrorType::SchemaNotFound);
        assert!(errors[0].is_error());
    }

    #[tokio::test]
    async fn async_with_schema() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/ns">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root xmlns="http://example.com/ns" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">content</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/schema.xsd", xsd.as_bytes());

        let errors = validate_dom_autodetect_async(&doc, &fetcher, settings())
            .await
            .unwrap();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[tokio::test]
    async fn async_unreachable_schema_is_an_error() {
        let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="urn:x http://example.com/missing.xsd"/>"#;
        let doc = crate::parse(xml.as_bytes()).unwrap();
        let errors = validate_dom_autodetect_async(&doc, &MockAsyncFetcher::new(), settings())
            .await
            .unwrap();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].is_error());
        assert!(errors[0].message.contains("http://example.com/missing.xsd"));
    }
}
