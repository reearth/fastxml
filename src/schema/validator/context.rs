//! Schema validation context.

use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError};
use crate::schema::types::CompiledSchema;

use super::dom::DomSchemaValidator;

/// Schema validation context.
///
/// Thread-safe wrapper for schema validation.
pub struct XmlSchemaValidationContext {
    schema: Arc<CompiledSchema>,
}

impl XmlSchemaValidationContext {
    /// Creates a new validation context.
    pub fn new(schema: CompiledSchema) -> Self {
        Self {
            schema: Arc::new(schema),
        }
    }

    /// Validates a document by traversing the DOM tree.
    ///
    /// This uses the fast DOM-based validator that directly traverses the DOM
    /// without reconstructing XML events.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let validator = DomSchemaValidator::new(Arc::clone(&self.schema));
        validator.validate(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_schema_validation_context_validate_empty_doc() {
        let schema = CompiledSchema::new();
        let ctx = XmlSchemaValidationContext::new(schema);
        let doc = crate::parse("<root/>").unwrap();

        let errors = ctx.validate(&doc).unwrap();
        // Empty schema should not produce errors
        assert!(errors.is_empty());
    }

    #[test]
    fn test_xml_schema_validation_context_validate_with_text() {
        let schema = CompiledSchema::new();
        let ctx = XmlSchemaValidationContext::new(schema);
        let doc = crate::parse("<root>some text content</root>").unwrap();

        let errors = ctx.validate(&doc).unwrap();
        // Empty schema should not produce errors
        assert!(errors.is_empty());
    }

    #[test]
    fn test_xml_schema_validation_context_validate_nested() {
        let schema = CompiledSchema::new();
        let ctx = XmlSchemaValidationContext::new(schema);
        let doc = crate::parse("<root><child><grandchild/></child></root>").unwrap();

        let errors = ctx.validate(&doc).unwrap();
        assert!(errors.is_empty());
    }
}
