//! Public API functions for schema validation.

use std::io::{BufRead, Seek};
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::types::CompiledSchema;

use super::context::XmlSchemaValidationContext;
use super::lazy::LazySchemaValidatorWithSharedErrors;
#[allow(deprecated)]
use super::two_pass::TwoPassSchemaValidator;

/// Creates a schema validation context from a schema location.
///
/// If the location is a URL, this will attempt to fetch and parse the XSD.
/// If it's a file path, it will read and parse the file.
///
/// Note: This currently creates a schema with built-in types only.
/// For full import resolution, use `create_xml_schema_validation_context_with_fetcher`.
pub fn create_xml_schema_validation_context(
    schema_location: &str,
) -> Result<XmlSchemaValidationContext> {
    // Check if it's a URL or file path
    if schema_location.starts_with("http://") || schema_location.starts_with("https://") {
        // For URLs, create a schema with built-in types only for now
        // Full resolution would require a fetcher
        let schema = crate::schema::xsd::create_builtin_schema();
        Ok(XmlSchemaValidationContext::new(schema))
    } else {
        // Try to read as a local file
        match std::fs::read(schema_location) {
            Ok(content) => {
                let schema = crate::schema::xsd::parse_xsd(&content)?;
                Ok(XmlSchemaValidationContext::new(schema))
            }
            Err(_) => {
                // Fall back to built-in types only
                let schema = crate::schema::xsd::create_builtin_schema();
                Ok(XmlSchemaValidationContext::new(schema))
            }
        }
    }
}

/// Creates a schema validation context from schema content.
///
/// Parses the provided XSD content and creates a validation context.
/// Built-in XSD and GML types are automatically registered.
pub fn create_xml_schema_validation_context_from_buffer(
    schema_content: &[u8],
) -> Result<XmlSchemaValidationContext> {
    let schema = crate::schema::xsd::parse_xsd(schema_content)?;
    Ok(XmlSchemaValidationContext::new(schema))
}

/// Validates a document against a schema.
pub fn validate_document_by_schema(
    doc: &XmlDocument,
    schema_location: &str,
) -> Result<Vec<StructuredError>> {
    let ctx = create_xml_schema_validation_context(schema_location)?;
    ctx.validate(doc)
}

/// Validates a document using an existing validation context.
pub fn validate_document_by_schema_context(
    doc: &XmlDocument,
    ctx: &XmlSchemaValidationContext,
) -> Result<Vec<StructuredError>> {
    ctx.validate(doc)
}

/// Validates a document using schemas referenced in xsi:schemaLocation.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas, and validates the document.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location};
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let errors = validate_with_schema_location(&doc)?;
/// ```
#[cfg(feature = "ureq")]
pub fn validate_with_schema_location(doc: &XmlDocument) -> Result<Vec<StructuredError>> {
    validate_with_schema_location_and_fetcher(doc, &crate::schema::fetcher::DefaultFetcher::new())
}

/// Validates a document using schemas referenced in xsi:schemaLocation with a custom fetcher.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas using the provided fetcher,
/// and validates the document.
///
/// # Arguments
///
/// * `doc` - The XML document to validate
/// * `fetcher` - A schema fetcher implementation for downloading schemas
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location_and_fetcher};
/// use fastxml::schema::UreqFetcher;
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let fetcher = UreqFetcher::new().timeout(60);
/// let errors = validate_with_schema_location_and_fetcher(&doc, &fetcher)?;
/// ```
pub fn validate_with_schema_location_and_fetcher<F: SchemaFetcher>(
    doc: &XmlDocument,
    fetcher: &F,
) -> Result<Vec<StructuredError>> {
    use crate::parser::parse_schema_locations;

    // Get schema locations from the document
    let locations = parse_schema_locations(doc)?;

    if locations.is_empty() {
        // No schema locations found, use built-in schema only
        let schema = crate::schema::xsd::create_builtin_schema();
        let ctx = XmlSchemaValidationContext::new(schema);
        return ctx.validate(doc);
    }

    // Use a single resolver to avoid duplicate dependency fetches
    let mut resolver = crate::schema::xsd::SchemaResolver::new(fetcher);
    let mut all_errors = Vec::new();
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                match resolver.resolve_entry(&fetch_result.content, &fetch_result.final_url) {
                    Ok(()) => {
                        loaded_any = true;
                    }
                    Err(e) => {
                        all_errors.push(
                            StructuredError::new(
                                format!("Failed to parse schema {}: {}", location, e),
                                ValidationErrorType::SchemaNotFound,
                            )
                            .with_level(ErrorLevel::Warning),
                        );
                    }
                }
            }
            Err(_) => {
                return Err(crate::error::Error::Schema(
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: location.clone(),
                    },
                ));
            }
        }
    }

    if !loaded_any {
        let schema = crate::schema::xsd::create_builtin_schema();
        let ctx = XmlSchemaValidationContext::new(schema);
        return ctx.validate(doc);
    }

    let schemas = resolver.take_all_schemas();
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);

    let ctx = XmlSchemaValidationContext::new(schema);
    match ctx.validate(doc) {
        Ok(errors) => {
            all_errors.extend(errors);
            Ok(all_errors)
        }
        Err(e) => {
            all_errors.push(
                StructuredError::new(
                    format!("Validation error: {}", e),
                    ValidationErrorType::Other,
                )
                .with_level(ErrorLevel::Error),
            );
            Ok(all_errors)
        }
    }
}

/// Gets a compiled schema from xsi:schemaLocation in the document.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas, and returns a compiled schema
/// suitable for streaming validation.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, get_schema_from_schema_location};
/// use fastxml::event::StreamingParser;
/// use fastxml::schema::validator::OnePassSchemaValidator;
/// use std::sync::Arc;
/// use std::io::BufReader;
///
/// let xml_bytes = std::fs::read("document.xml")?;
///
/// // Get compiled schema from xsi:schemaLocation
/// let schema = Arc::new(get_schema_from_schema_location(&xml_bytes)?);
///
/// // Use for streaming validation
/// let mut parser = StreamingParser::new(BufReader::new(xml_bytes.as_slice()));
/// parser.add_handler(Box::new(OnePassSchemaValidator::new(schema)));
/// parser.parse()?;
/// ```
#[cfg(feature = "ureq")]
pub fn get_schema_from_schema_location(xml_content: &[u8]) -> Result<CompiledSchema> {
    get_schema_from_schema_location_with_fetcher(
        xml_content,
        &crate::schema::fetcher::DefaultFetcher::new(),
    )
}

/// Gets a compiled schema from xsi:schemaLocation with a custom fetcher.
///
/// This function reads the `xsi:schemaLocation` attribute from the document's
/// root element, fetches the referenced schemas using the provided fetcher,
/// and returns a compiled schema suitable for streaming validation.
pub fn get_schema_from_schema_location_with_fetcher<F: SchemaFetcher>(
    xml_content: &[u8],
    fetcher: &F,
) -> Result<CompiledSchema> {
    use crate::parser::parse_schema_locations;

    // Parse to extract schema locations
    let doc = crate::parse(xml_content)?;
    let locations = parse_schema_locations(&doc)?;

    if locations.is_empty() {
        return Ok(crate::schema::xsd::create_builtin_schema());
    }

    // Use a single resolver to handle all schema locations
    let mut resolver = crate::schema::xsd::SchemaResolver::new(fetcher);
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                match resolver.resolve_entry(&fetch_result.content, &fetch_result.final_url) {
                    Ok(()) => {
                        loaded_any = true;
                    }
                    Err(_) => {
                        return Err(crate::error::Error::Schema(
                            crate::schema::error::SchemaError::SchemaNotFound {
                                uri: location.clone(),
                            },
                        ));
                    }
                }
            }
            Err(_) => {
                return Err(crate::error::Error::Schema(
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: location.clone(),
                    },
                ));
            }
        }
    }

    if !loaded_any {
        return Ok(crate::schema::xsd::create_builtin_schema());
    }

    let schemas = resolver.take_all_schemas();
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);
    Ok(schema)
}

/// Validates XML from a reader using streaming parser with schemas from xsi:schemaLocation.
///
/// This performs true single-pass streaming validation:
/// 1. On the first StartElement, extracts xsi:schemaLocation
/// 2. Fetches and compiles the referenced schemas
/// 3. Continues streaming validation with the fetched schema
///
/// # Example
///
/// ```ignore
/// use fastxml::streaming_validate_with_schema_location;
/// use std::fs::File;
/// use std::io::BufReader;
///
/// let file = File::open("large_document.xml")?;
/// let errors = streaming_validate_with_schema_location(BufReader::new(file))?;
///
/// if errors.is_empty() {
///     println!("Document is valid!");
/// }
/// ```
#[cfg(feature = "ureq")]
pub fn streaming_validate_with_schema_location<R: std::io::BufRead>(
    reader: R,
) -> Result<Vec<StructuredError>> {
    streaming_validate_with_schema_location_and_fetcher(
        reader,
        crate::schema::fetcher::DefaultFetcher::new(),
    )
}

/// Validates XML from a reader using streaming parser with a custom fetcher.
///
/// This performs true single-pass streaming validation.
pub fn streaming_validate_with_schema_location_and_fetcher<
    R: std::io::BufRead,
    F: SchemaFetcher + 'static,
>(
    reader: R,
    fetcher: F,
) -> Result<Vec<StructuredError>> {
    use crate::event::StreamingParser;
    use std::sync::{Arc, Mutex};

    let mut parser = StreamingParser::new(reader);

    // Shared error collection
    let shared_errors = Arc::new(Mutex::new(Vec::new()));
    let validator = LazySchemaValidatorWithSharedErrors::new(fetcher, Arc::clone(&shared_errors));
    parser.add_handler(Box::new(validator));

    parser.parse()?;

    // Collect errors from shared state
    let errors = shared_errors.lock().unwrap().clone();
    Ok(errors)
}

/// Performs two-pass validation using schemas from xsi:schemaLocation.
///
/// This function extracts xsi:schemaLocation from the XML content, fetches
/// the referenced schemas, and performs two-pass validation.
///
/// # Example
///
/// ```ignore
/// use std::fs::File;
/// use std::io::BufReader;
/// use fastxml::schema::validator::two_pass_validate_with_schema_location;
///
/// let file = File::open("document.xml")?;
/// let reader = BufReader::new(file);
/// let errors = two_pass_validate_with_schema_location(reader)?;
/// ```
#[deprecated(
    since = "0.5.0",
    note = "Use streaming_validate_with_schema_location instead for better performance"
)]
#[allow(deprecated)]
#[cfg(feature = "ureq")]
pub fn two_pass_validate_with_schema_location<R: BufRead + Seek>(
    reader: R,
) -> Result<Vec<StructuredError>> {
    two_pass_validate_with_schema_location_and_fetcher(
        reader,
        &crate::schema::fetcher::DefaultFetcher::new(),
    )
}

/// Performs two-pass validation using schemas from xsi:schemaLocation with a custom fetcher.
///
/// # Arguments
///
/// * `reader` - A seekable reader for the XML content
/// * `fetcher` - A schema fetcher implementation for downloading schemas
///
/// # Example
///
/// ```ignore
/// use std::fs::File;
/// use std::io::BufReader;
/// use fastxml::schema::validator::two_pass_validate_with_schema_location_and_fetcher;
/// use fastxml::schema::UreqFetcher;
///
/// let file = File::open("document.xml")?;
/// let reader = BufReader::new(file);
/// let fetcher = UreqFetcher::new().timeout(60);
/// let errors = two_pass_validate_with_schema_location_and_fetcher(reader, &fetcher)?;
/// ```
#[deprecated(
    since = "0.5.0",
    note = "Use streaming_validate_with_schema_location_and_fetcher instead for better performance"
)]
#[allow(deprecated)]
pub fn two_pass_validate_with_schema_location_and_fetcher<R: BufRead + Seek, F: SchemaFetcher>(
    mut reader: R,
    fetcher: &F,
) -> Result<Vec<StructuredError>> {
    use crate::parser::parse_schema_locations;
    use std::io::SeekFrom;

    // Read the content to extract schema locations
    let mut content = Vec::new();
    reader.read_to_end(&mut content)?;

    // Parse to extract schema locations
    let doc = crate::parse(&content)?;
    let locations = parse_schema_locations(&doc)?;

    // Seek back to start
    reader.seek(SeekFrom::Start(0))?;

    if locations.is_empty() {
        // No schema locations found, use built-in schema
        let schema = crate::schema::xsd::create_builtin_schema();
        return TwoPassSchemaValidator::new(Arc::new(schema)).validate(reader);
    }

    // Use a single resolver to handle all schema locations
    let mut resolver = crate::schema::xsd::SchemaResolver::new(fetcher);
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                if resolver
                    .resolve_entry(&fetch_result.content, &fetch_result.final_url)
                    .is_ok()
                {
                    loaded_any = true;
                }
            }
            Err(_) => {
                return Err(crate::error::Error::Schema(
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: location.clone(),
                    },
                ));
            }
        }
    }

    if !loaded_any {
        let schema = crate::schema::xsd::create_builtin_schema();
        return TwoPassSchemaValidator::new(Arc::new(schema)).validate(reader);
    }

    let schemas = resolver.take_all_schemas();
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);
    TwoPassSchemaValidator::new(Arc::new(schema)).validate(reader)
}

/// Validates a document using schemas referenced in xsi:schemaLocation with an async fetcher.
///
/// This is the async version of [`validate_with_schema_location_and_fetcher`].
/// It fetches schemas asynchronously using the provided fetcher.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location_with_async_fetcher};
/// use fastxml::schema::AsyncDefaultFetcher;
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let fetcher = AsyncDefaultFetcher::new()?;
/// let errors = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await?;
/// ```
#[cfg(feature = "tokio")]
pub async fn validate_with_schema_location_with_async_fetcher<
    F: crate::schema::fetcher::AsyncSchemaFetcher,
>(
    doc: &XmlDocument,
    fetcher: &F,
) -> Result<Vec<StructuredError>> {
    use crate::parser::parse_schema_locations;

    // Get schema locations from the document
    let locations = parse_schema_locations(doc)?;

    if locations.is_empty() {
        // No schema locations found, use built-in schema only
        let schema = crate::schema::xsd::create_builtin_schema();
        let ctx = XmlSchemaValidationContext::new(schema);
        return ctx.validate(doc);
    }

    // Use a single async resolver to avoid duplicate dependency fetches
    let mut resolver = crate::schema::xsd::AsyncSchemaResolver::new(fetcher);
    let mut all_errors = Vec::new();
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location).await {
            Ok(fetch_result) => {
                match resolver
                    .resolve_entry(&fetch_result.content, &fetch_result.final_url)
                    .await
                {
                    Ok(()) => {
                        loaded_any = true;
                    }
                    Err(e) => {
                        all_errors.push(
                            StructuredError::new(
                                format!("Failed to parse schema {}: {}", location, e),
                                ValidationErrorType::SchemaNotFound,
                            )
                            .with_level(ErrorLevel::Warning),
                        );
                    }
                }
            }
            Err(_) => {
                return Err(crate::error::Error::Schema(
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: location.clone(),
                    },
                ));
            }
        }
    }

    if !loaded_any {
        let schema = crate::schema::xsd::create_builtin_schema();
        let ctx = XmlSchemaValidationContext::new(schema);
        return ctx.validate(doc);
    }

    let schemas = resolver.take_all_schemas();
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);

    let ctx = XmlSchemaValidationContext::new(schema);
    match ctx.validate(doc) {
        Ok(errors) => {
            all_errors.extend(errors);
            Ok(all_errors)
        }
        Err(e) => {
            all_errors.push(
                StructuredError::new(
                    format!("Validation error: {}", e),
                    ValidationErrorType::Other,
                )
                .with_level(ErrorLevel::Error),
            );
            Ok(all_errors)
        }
    }
}

/// Validates a document using schemas referenced in xsi:schemaLocation asynchronously.
///
/// This is the async version of [`validate_with_schema_location`].
/// It uses the default async fetcher (`AsyncDefaultFetcher`).
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, validate_with_schema_location_async};
///
/// let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
///                   xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">
///     <child>content</child>
/// </root>"#;
///
/// let doc = parse(xml)?;
/// let errors = validate_with_schema_location_async(&doc).await?;
/// ```
#[cfg(feature = "tokio")]
pub async fn validate_with_schema_location_async(
    doc: &XmlDocument,
) -> Result<Vec<StructuredError>> {
    let fetcher = crate::schema::fetcher::AsyncDefaultFetcher::new()?;
    validate_with_schema_location_with_async_fetcher(doc, &fetcher).await
}

/// Gets a compiled schema from xsi:schemaLocation with an async fetcher.
///
/// This is the async version of [`get_schema_from_schema_location_with_fetcher`].
/// It fetches schemas asynchronously using the provided fetcher.
///
/// # Example
///
/// ```ignore
/// use fastxml::get_schema_from_schema_location_with_async_fetcher;
/// use fastxml::schema::AsyncDefaultFetcher;
///
/// let xml_bytes = std::fs::read("document.xml")?;
/// let fetcher = AsyncDefaultFetcher::new()?;
/// let schema = get_schema_from_schema_location_with_async_fetcher(&xml_bytes, &fetcher).await?;
/// ```
#[cfg(feature = "tokio")]
pub async fn get_schema_from_schema_location_with_async_fetcher<
    F: crate::schema::fetcher::AsyncSchemaFetcher,
>(
    xml_content: &[u8],
    fetcher: &F,
) -> Result<CompiledSchema> {
    use crate::parser::parse_schema_locations;

    // Parse to extract schema locations
    let doc = crate::parse(xml_content)?;
    let locations = parse_schema_locations(&doc)?;

    if locations.is_empty() {
        return Ok(crate::schema::xsd::create_builtin_schema());
    }

    // Use a single async resolver to handle all schema locations
    let mut resolver = crate::schema::xsd::AsyncSchemaResolver::new(fetcher);
    let mut loaded_any = false;

    for (_namespace, location) in &locations {
        match fetcher.fetch(location).await {
            Ok(fetch_result) => {
                match resolver
                    .resolve_entry(&fetch_result.content, &fetch_result.final_url)
                    .await
                {
                    Ok(()) => {
                        loaded_any = true;
                    }
                    Err(_) => {
                        return Err(crate::error::Error::Schema(
                            crate::schema::error::SchemaError::SchemaNotFound {
                                uri: location.clone(),
                            },
                        ));
                    }
                }
            }
            Err(_) => {
                return Err(crate::error::Error::Schema(
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: location.clone(),
                    },
                ));
            }
        }
    }

    if !loaded_any {
        return Ok(crate::schema::xsd::create_builtin_schema());
    }

    let schemas = resolver.take_all_schemas();
    let mut schema = crate::schema::xsd::compile_schemas(schemas)?;
    crate::schema::xsd::register_builtin_types(&mut schema);
    Ok(schema)
}

/// Gets a compiled schema from xsi:schemaLocation asynchronously.
///
/// This is the async version of [`get_schema_from_schema_location`].
/// It uses the default async fetcher (`AsyncDefaultFetcher`).
///
/// # Example
///
/// ```ignore
/// use fastxml::get_schema_from_schema_location_async;
///
/// let xml_bytes = std::fs::read("document.xml")?;
/// let schema = get_schema_from_schema_location_async(&xml_bytes).await?;
/// ```
#[cfg(feature = "tokio")]
pub async fn get_schema_from_schema_location_async(xml_content: &[u8]) -> Result<CompiledSchema> {
    let fetcher = crate::schema::fetcher::AsyncDefaultFetcher::new()?;
    get_schema_from_schema_location_with_async_fetcher(xml_content, &fetcher).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_xml_schema_validation_context_url() {
        // Should create builtin schema for URLs (without actual fetching)
        let ctx = create_xml_schema_validation_context("http://example.com/schema.xsd").unwrap();
        assert!(!ctx.schema().types.is_empty()); // Has builtin types
    }

    #[test]
    fn test_create_xml_schema_validation_context_nonexistent_file() {
        // Should fall back to builtin schema for non-existent file
        let ctx = create_xml_schema_validation_context("/nonexistent/path/schema.xsd").unwrap();
        assert!(!ctx.schema().types.is_empty()); // Has builtin types
    }

    #[test]
    fn test_create_xml_schema_validation_context_from_buffer() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let ctx = create_xml_schema_validation_context_from_buffer(xsd.as_bytes()).unwrap();
        assert!(ctx.schema().elements.contains_key("test"));
    }

    #[test]
    fn test_validate_document_by_schema() {
        let doc = crate::parse("<root/>").unwrap();
        // Use non-existent schema, should fall back to builtin
        let errors = validate_document_by_schema(&doc, "/nonexistent").unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_document_by_schema_context() {
        let schema = crate::schema::xsd::create_builtin_schema();
        let ctx = XmlSchemaValidationContext::new(schema);
        let doc = crate::parse("<root/>").unwrap();
        let errors = validate_document_by_schema_context(&doc, &ctx).unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_with_schema_location_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = validate_with_schema_location_and_fetcher(&doc, &fetcher);
        // No schemaLocation found, uses builtin schema -> Ok
        assert!(result.is_ok());
    }

    #[test]
    fn test_streaming_validate_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let reader = std::io::BufReader::new(xml.as_bytes());
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = streaming_validate_with_schema_location_and_fetcher(reader, fetcher);
        // No schemaLocation, uses builtin schema -> Ok
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_schema_from_schema_location_no_schema_location() {
        let xml = b"<root/>";
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = get_schema_from_schema_location_with_fetcher(xml, &fetcher);
        // No schemaLocation attribute - returns builtin schema
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_schema_from_schema_location_with_schema_location() {
        let xml = br#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://example.com http://example.com/schema.xsd"/>"#;
        let fetcher = crate::schema::fetcher::NoopFetcher;

        let result = get_schema_from_schema_location_with_fetcher(xml, &fetcher);
        // NoopFetcher returns error
        assert!(result.is_err());
    }
}

#[cfg(all(test, feature = "tokio"))]
mod async_tests {
    use super::*;
    use crate::schema::fetcher::{AsyncSchemaFetcher, FetchResult};
    use parking_lot::RwLock;
    use std::collections::HashMap as StdHashMap;
    use std::sync::Arc;

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

    #[tokio::test]
    async fn test_validate_with_schema_location_async_no_schema_location() {
        let xml = r#"<?xml version="1.0"?>
<root>
    <element>content</element>
</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = MockAsyncFetcher::new();

        let result = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await;
        // No schemaLocation found, uses builtin schema -> Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_with_schema_location_async_with_schema() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/ns">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd">content</root>"#;

        let doc = crate::parse(xml.as_bytes()).unwrap();
        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/schema.xsd", xsd.as_bytes());

        let result = validate_with_schema_location_with_async_fetcher(&doc, &fetcher).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_schema_from_schema_location_async_no_schema_location() {
        let xml = b"<root/>";
        let fetcher = MockAsyncFetcher::new();

        let result = get_schema_from_schema_location_with_async_fetcher(xml, &fetcher).await;
        // No schemaLocation attribute - returns builtin schema
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_schema_from_schema_location_async_with_schema() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/ns">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

        let xml = br#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://example.com/ns http://example.com/schema.xsd"/>"#;

        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/schema.xsd", xsd.as_bytes());

        let result = get_schema_from_schema_location_with_async_fetcher(xml, &fetcher).await;
        assert!(result.is_ok());
    }
}
