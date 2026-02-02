//! Lazy schema validators that initialize from xsi:schemaLocation.

use std::sync::{Arc, Mutex};

use compact_str::CompactString;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::store::SchemaStore;

use super::streaming::StreamingSchemaValidator;

/// A streaming validator that lazily initializes schema from xsi:schemaLocation.
///
/// On the first StartElement event, this validator extracts xsi:schemaLocation,
/// fetches and compiles the schema, then delegates to StreamingSchemaValidator.
pub struct LazySchemaValidator<F: SchemaFetcher> {
    fetcher: F,
    validator: Option<StreamingSchemaValidator>,
    initialized: bool,
    errors: Vec<StructuredError>,
}

impl<F: SchemaFetcher> LazySchemaValidator<F> {
    /// Creates a new lazy schema validator.
    pub fn new(fetcher: F) -> Self {
        Self {
            fetcher,
            validator: None,
            initialized: false,
            errors: Vec::new(),
        }
    }

    /// Returns collected validation errors.
    pub fn errors(&self) -> &[StructuredError] {
        if let Some(v) = &self.validator {
            v.errors()
        } else {
            &self.errors
        }
    }

    fn initialize_from_attributes(&mut self, attributes: &[(CompactString, CompactString)]) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        // Look for xsi:schemaLocation
        let schema_location = attributes
            .iter()
            .find(|(k, _)| k == "xsi:schemaLocation" || k == "schemaLocation")
            .map(|(_, v)| v.as_str());

        let schema = if let Some(loc_value) = schema_location {
            // Parse schemaLocation value (namespace/URL pairs)
            let parts: Vec<&str> = loc_value.split_whitespace().collect();
            let mut compiled = None;
            let mut fetch_error = None;

            for chunk in parts.chunks(2) {
                if chunk.len() == 2 {
                    let location = chunk[1];
                    match self.fetcher.fetch(location) {
                        Ok(result) => {
                            let store = crate::schema::memory::InMemoryStore::new();
                            if let Ok(()) =
                                SchemaStore::put(&store, &result.final_url, &result.content)
                            {
                                match crate::schema::xsd::parse_xsd_with_imports(
                                    &result.content,
                                    &result.final_url,
                                    &self.fetcher,
                                    &store,
                                ) {
                                    Ok(schema) => {
                                        compiled = Some(schema);
                                        break;
                                    }
                                    Err(e) => {
                                        fetch_error = Some(format!(
                                            "Failed to parse schema {}: {}",
                                            location, e
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            fetch_error =
                                Some(format!("Failed to fetch schema {}: {}", location, e));
                        }
                    }
                }
            }

            if compiled.is_none() {
                if let Some(err_msg) = fetch_error {
                    self.errors.push(StructuredError {
                        message: err_msg,
                        line: None,
                        column: None,
                        error_type: ValidationErrorType::SchemaNotFound,
                        level: ErrorLevel::Error,
                        element_path: None,
                        node_name: None,
                        expected: None,
                        found: None,
                    });
                }
            }

            compiled.unwrap_or_else(crate::schema::xsd::create_builtin_schema)
        } else {
            crate::schema::xsd::create_builtin_schema()
        };

        self.validator = Some(StreamingSchemaValidator::new(Arc::new(schema)));
    }
}

impl<F: SchemaFetcher> XmlEventHandler for LazySchemaValidator<F> {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        // Initialize on first StartElement
        if let XmlEvent::StartElement { attributes, .. } = event {
            if !self.initialized {
                self.initialize_from_attributes(attributes);
            }
        }

        // Delegate to inner validator
        if let Some(v) = &mut self.validator {
            v.handle(event)?;
        }

        Ok(())
    }
}

/// Internal validator with shared error collection for streaming validation functions.
pub(crate) struct LazySchemaValidatorWithSharedErrors<F: SchemaFetcher> {
    fetcher: F,
    validator: Option<StreamingSchemaValidator>,
    initialized: bool,
    shared_errors: Arc<Mutex<Vec<StructuredError>>>,
}

impl<F: SchemaFetcher> LazySchemaValidatorWithSharedErrors<F> {
    pub fn new(fetcher: F, shared_errors: Arc<Mutex<Vec<StructuredError>>>) -> Self {
        Self {
            fetcher,
            validator: None,
            initialized: false,
            shared_errors,
        }
    }

    fn initialize_from_attributes(&mut self, attributes: &[(CompactString, CompactString)]) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        // Look for xsi:schemaLocation
        let schema_location = attributes
            .iter()
            .find(|(k, _)| k == "xsi:schemaLocation" || k == "schemaLocation")
            .map(|(_, v)| v.as_str());

        let schema = if let Some(loc_value) = schema_location {
            // Parse schemaLocation value (namespace/URL pairs)
            let parts: Vec<&str> = loc_value.split_whitespace().collect();
            let mut compiled = None;
            let mut fetch_error = None;

            for chunk in parts.chunks(2) {
                if chunk.len() == 2 {
                    let location = chunk[1];
                    match self.fetcher.fetch(location) {
                        Ok(result) => {
                            let store = crate::schema::memory::InMemoryStore::new();
                            if let Ok(()) =
                                SchemaStore::put(&store, &result.final_url, &result.content)
                            {
                                match crate::schema::xsd::parse_xsd_with_imports(
                                    &result.content,
                                    &result.final_url,
                                    &self.fetcher,
                                    &store,
                                ) {
                                    Ok(schema) => {
                                        compiled = Some(schema);
                                        break;
                                    }
                                    Err(e) => {
                                        fetch_error = Some(format!(
                                            "Failed to parse schema {}: {}",
                                            location, e
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            fetch_error =
                                Some(format!("Failed to fetch schema {}: {}", location, e));
                        }
                    }
                }
            }

            if compiled.is_none() {
                if let Some(err_msg) = fetch_error {
                    self.shared_errors.lock().unwrap().push(StructuredError {
                        message: err_msg,
                        line: None,
                        column: None,
                        error_type: ValidationErrorType::SchemaNotFound,
                        level: ErrorLevel::Error,
                        element_path: None,
                        node_name: None,
                        expected: None,
                        found: None,
                    });
                }
            }

            compiled.unwrap_or_else(crate::schema::xsd::create_builtin_schema)
        } else {
            crate::schema::xsd::create_builtin_schema()
        };

        self.validator = Some(StreamingSchemaValidator::new(Arc::new(schema)));
    }
}

impl<F: SchemaFetcher> XmlEventHandler for LazySchemaValidatorWithSharedErrors<F> {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        // Initialize on first StartElement
        if let XmlEvent::StartElement { attributes, .. } = event {
            if !self.initialized {
                self.initialize_from_attributes(attributes);
            }
        }

        // Delegate to inner validator
        if let Some(v) = &mut self.validator {
            v.handle(event)?;
            // Collect validation errors to shared collection
            for err in v.errors() {
                let mut errors = self.shared_errors.lock().unwrap();
                if !errors.iter().any(|e| e.message == err.message) {
                    errors.push(err.clone());
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::NoopFetcher;

    #[test]
    fn test_lazy_validator_new() {
        let fetcher = NoopFetcher;
        let validator = LazySchemaValidator::new(fetcher);
        assert!(validator.errors().is_empty());
    }

    #[test]
    fn test_lazy_validator_no_schema_location() {
        let fetcher = NoopFetcher;
        let mut validator = LazySchemaValidator::new(fetcher);

        // Handle element without schemaLocation
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: None,
        });

        // Should use builtin schema without errors
        assert!(validator.errors().is_empty());
    }

    #[test]
    fn test_lazy_validator_with_shared_errors() {
        let fetcher = NoopFetcher;
        let shared_errors = Arc::new(Mutex::new(Vec::new()));
        let mut validator =
            LazySchemaValidatorWithSharedErrors::new(fetcher, Arc::clone(&shared_errors));

        // Handle element without schemaLocation
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: None,
        });

        let errors = shared_errors.lock().unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_lazy_schema_validator_errors_empty() {
        let fetcher = NoopFetcher;
        let validator = LazySchemaValidator::new(fetcher);
        assert!(validator.errors().is_empty());
    }

    #[test]
    fn test_lazy_schema_validator_handle_with_schema_location() {
        let fetcher = NoopFetcher;
        let mut validator = LazySchemaValidator::new(fetcher);

        // Handle a start element with xsi:schemaLocation
        // NoopFetcher returns an error, so schema won't be loaded
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![
                (
                    "xmlns:xsi".into(),
                    "http://www.w3.org/2001/XMLSchema-instance".into(),
                ),
                (
                    "xsi:schemaLocation".into(),
                    "http://example.com http://example.com/schema.xsd".into(),
                ),
            ],
            namespace_decls: vec![],
            line: Some(1),
        });

        let _ = validator.handle(&XmlEvent::EndElement {
            name: "root".into(),
            prefix: None,
        });

        let _ = validator.handle(&XmlEvent::Eof);
    }

    #[test]
    fn test_lazy_schema_validator_delegates_to_inner() {
        let fetcher = NoopFetcher;
        let mut validator = LazySchemaValidator::new(fetcher);

        // First element initializes
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        });

        // Child element should be delegated to inner validator
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "child".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
        });

        let _ = validator.handle(&XmlEvent::Text("content".to_string()));

        let _ = validator.handle(&XmlEvent::EndElement {
            name: "child".into(),
            prefix: None,
        });

        let _ = validator.handle(&XmlEvent::EndElement {
            name: "root".into(),
            prefix: None,
        });

        let _ = validator.handle(&XmlEvent::Eof);
    }
}
