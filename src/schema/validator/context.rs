//! Schema validation context.

use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError};
use crate::schema::types::CompiledSchema;

use super::dom::DomSchemaValidator;
use super::streaming::OnePassSchemaValidator;

/// Schema validation context.
///
/// Thread-safe wrapper for schema validation.
#[doc(hidden)]
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

    /// Creates a context from an Arc'd schema.
    pub fn from_arc(schema: Arc<CompiledSchema>) -> Self {
        Self { schema }
    }

    /// Validates a document by traversing the DOM tree.
    ///
    /// This uses the fast DOM-based validator that directly traverses the DOM
    /// without reconstructing XML events.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let validator = DomSchemaValidator::new(Arc::clone(&self.schema));
        validator.validate(doc)
    }

    /// Creates a one-pass streaming validator.
    pub fn create_validator(&self) -> OnePassSchemaValidator {
        OnePassSchemaValidator::new(Arc::clone(&self.schema))
    }

    /// Returns a reference to the schema.
    pub fn schema(&self) -> &CompiledSchema {
        &self.schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_schema_validation_context_new() {
        let schema = CompiledSchema::new();
        let ctx = XmlSchemaValidationContext::new(schema);
        assert!(ctx.schema().elements.is_empty());
    }

    #[test]
    fn test_xml_schema_validation_context_from_arc() {
        let schema = Arc::new(CompiledSchema::new());
        let ctx = XmlSchemaValidationContext::from_arc(schema);
        assert!(ctx.schema().elements.is_empty());
    }

    #[test]
    fn test_xml_schema_validation_context_create_validator() {
        let schema = CompiledSchema::new();
        let ctx = XmlSchemaValidationContext::new(schema);
        let validator = ctx.create_validator();
        assert!(validator.is_valid());
    }

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

    #[test]
    fn test_xml_schema_validation_context_schema() {
        use crate::schema::types::ElementDef;

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("test".to_string(), ElementDef::new("test"));
        let ctx = XmlSchemaValidationContext::new(schema);
        assert!(ctx.schema().elements.contains_key("test"));
    }
}
