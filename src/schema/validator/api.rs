//! Public API functions for schema validation.

use std::io::{BufRead, Seek};
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::store::SchemaStore;
use crate::schema::types::CompiledSchema;

use super::context::XmlSchemaValidationContext;
use super::lazy::LazySchemaValidatorWithSharedErrors;
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

    // Fetch and parse all schemas
    let mut all_errors = Vec::new();
    let store = crate::schema::memory::InMemoryStore::new();

    for (_namespace, location) in &locations {
        // Try to fetch the schema
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                // Store the fetched schema
                let schema_key = fetch_result.final_url.clone();
                if !store.contains(&schema_key) {
                    let _ = store.put(&schema_key, &fetch_result.content);
                }

                // Parse the schema with import resolution
                match crate::schema::xsd::parse_xsd_with_imports(
                    &fetch_result.content,
                    &fetch_result.final_url,
                    fetcher,
                    &store,
                ) {
                    Ok(schema) => {
                        let ctx = XmlSchemaValidationContext::new(schema);
                        match ctx.validate(doc) {
                            Ok(errors) => all_errors.extend(errors),
                            Err(e) => {
                                all_errors.push(StructuredError {
                                    message: format!("Validation error: {}", e),
                                    line: None,
                                    column: None,
                                    error_type: ValidationErrorType::Other,
                                    level: ErrorLevel::Error,
                                    element_path: None,
                                    node_name: None,
                                    expected: None,
                                    found: None,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        all_errors.push(StructuredError {
                            message: format!("Failed to parse schema {}: {}", location, e),
                            line: None,
                            column: None,
                            error_type: ValidationErrorType::SchemaNotFound,
                            level: ErrorLevel::Warning,
                            element_path: None,
                            node_name: None,
                            expected: None,
                            found: None,
                        });
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

    Ok(all_errors)
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

    let store = crate::schema::memory::InMemoryStore::new();

    // Try to fetch and compile the first schema
    // (schemaLocation typically has only one relevant schema per namespace)
    if let Some((_namespace, location)) = locations.first() {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                let _ = store.put(&fetch_result.final_url, &fetch_result.content);

                match crate::schema::xsd::parse_xsd_with_imports(
                    &fetch_result.content,
                    &fetch_result.final_url,
                    fetcher,
                    &store,
                ) {
                    Ok(schema) => return Ok(schema),
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

    // This shouldn't be reached if locations is not empty
    Err(crate::error::Error::Schema(
        crate::schema::error::SchemaError::SchemaNotFound {
            uri: "no schema locations".to_string(),
        },
    ))
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
        let validator = TwoPassSchemaValidator::new(reader, Arc::new(schema));
        return validator.validate();
    }

    let store = crate::schema::memory::InMemoryStore::new();

    // Try to fetch and compile the first schema
    if let Some((_namespace, location)) = locations.first() {
        match fetcher.fetch(location) {
            Ok(fetch_result) => {
                let _ = store.put(&fetch_result.final_url, &fetch_result.content);

                match crate::schema::xsd::parse_xsd_with_imports(
                    &fetch_result.content,
                    &fetch_result.final_url,
                    fetcher,
                    &store,
                ) {
                    Ok(schema) => {
                        let validator = TwoPassSchemaValidator::new(reader, Arc::new(schema));
                        return validator.validate();
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

    // Fallback to builtin schema
    let schema = crate::schema::xsd::create_builtin_schema();
    let validator = TwoPassSchemaValidator::new(reader, Arc::new(schema));
    validator.validate()
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
