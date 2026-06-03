//! Lazy schema validators that initialize from xsi:schemaLocation.

use std::sync::{Arc, Mutex};

use compact_str::CompactString;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::schema::fetcher::SchemaFetcher;

use super::streaming::OnePassSchemaValidator;

/// Internal validator with shared error collection for streaming validation functions.
pub(crate) struct LazySchemaValidatorWithSharedErrors<F: SchemaFetcher> {
    fetcher: F,
    validator: Option<OnePassSchemaValidator>,
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
            let mut resolver = crate::schema::xsd::SchemaResolver::new(&self.fetcher);
            let mut loaded_any = false;

            // Fetch and resolve all schemaLocation entries with a single resolver
            for chunk in parts.chunks(2) {
                if chunk.len() == 2 {
                    let location = chunk[1];
                    match self.fetcher.fetch(location) {
                        Ok(result) => {
                            match resolver.resolve_entry(&result.content, &result.final_url) {
                                Ok(()) => {
                                    loaded_any = true;
                                }
                                Err(e) => {
                                    self.shared_errors.lock().unwrap().push(
                                        StructuredError::new(
                                            format!(
                                                "Warning: Failed to parse schema {}: {}",
                                                location, e
                                            ),
                                            ValidationErrorType::SchemaNotFound,
                                        )
                                        .with_level(ErrorLevel::Warning),
                                    );
                                }
                            }
                        }
                        Err(_e) => {
                            // Skip schemas that can't be fetched (may be local paths)
                        }
                    }
                }
            }

            if !loaded_any {
                self.shared_errors.lock().unwrap().push(
                    StructuredError::new(
                        "No schemas could be loaded from xsi:schemaLocation",
                        ValidationErrorType::SchemaNotFound,
                    )
                    .with_level(ErrorLevel::Warning),
                );
                crate::schema::xsd::create_builtin_schema()
            } else {
                let schemas = resolver.take_all_schemas();
                match crate::schema::xsd::compile_schemas(schemas) {
                    Ok(mut compiled) => {
                        crate::schema::xsd::register_builtin_types(&mut compiled);
                        compiled
                    }
                    Err(e) => {
                        self.shared_errors.lock().unwrap().push(
                            StructuredError::new(
                                format!("Warning: Failed to compile schemas: {}", e),
                                ValidationErrorType::SchemaNotFound,
                            )
                            .with_level(ErrorLevel::Warning),
                        );
                        crate::schema::xsd::create_builtin_schema()
                    }
                }
            }
        } else {
            crate::schema::xsd::create_builtin_schema()
        };

        self.validator = Some(OnePassSchemaValidator::new(Arc::new(schema)));
    }
}

impl<F: SchemaFetcher + 'static> XmlEventHandler for LazySchemaValidatorWithSharedErrors<F> {
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

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::NoopFetcher;

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
            column: Some(1),
        });

        let errors = shared_errors.lock().unwrap();
        assert!(errors.is_empty());
    }
}
