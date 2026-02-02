//! Schema validation context.

use std::sync::Arc;

use compact_str::CompactString;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::node::{NodeType, XmlNode};
use crate::schema::types::CompiledSchema;

use super::streaming::StreamingSchemaValidator;

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

    /// Creates a context from an Arc'd schema.
    pub fn from_arc(schema: Arc<CompiledSchema>) -> Self {
        Self { schema }
    }

    /// Validates a document by traversing the DOM tree.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let mut validator = StreamingSchemaValidator::new(Arc::clone(&self.schema));

        // Get root element and validate
        if let Ok(root) = doc.get_root_element() {
            self.validate_node_recursive(&root, &mut validator);
        }

        // Finish validation
        validator.finish()?;

        Ok(validator.into_errors())
    }

    /// Recursively validates a node and its children.
    fn validate_node_recursive(&self, node: &XmlNode, validator: &mut StreamingSchemaValidator) {
        match node.get_type() {
            NodeType::Element => {
                let name = node.get_name();
                let prefix = node.get_prefix();
                let namespace = node.get_namespace_uri();
                let attrs = node.get_attributes();
                let ns_decls = node.get_namespace_declarations();

                // Create attributes list with CompactString
                let attributes: Vec<(CompactString, CompactString)> = attrs
                    .into_iter()
                    .map(|(k, v)| (CompactString::from(k), CompactString::from(v)))
                    .collect();

                // Start element event
                let _ = validator.handle(&XmlEvent::StartElement {
                    name: name.into(),
                    prefix: prefix.map(|p| p.into()),
                    namespace,
                    attributes,
                    namespace_decls: ns_decls,
                    line: None,
                });

                // Validate children
                for child in node.get_child_nodes() {
                    self.validate_node_recursive(&child, validator);
                }

                // End element event
                let _ = validator.handle(&XmlEvent::EndElement {
                    name: node.get_name().into(),
                    prefix: node.get_prefix().map(|p| p.into()),
                });
            }
            NodeType::Text => {
                if let Some(content) = node.get_content() {
                    let _ = validator.handle(&XmlEvent::Text(content));
                }
            }
            NodeType::CData => {
                if let Some(content) = node.get_content() {
                    let _ = validator.handle(&XmlEvent::CData(content));
                }
            }
            NodeType::Document => {
                // Validate children of document node
                for child in node.get_child_nodes() {
                    self.validate_node_recursive(&child, validator);
                }
            }
            _ => {
                // Skip other node types (comments, PIs, etc.)
            }
        }
    }

    /// Creates a streaming validator.
    pub fn create_validator(&self) -> StreamingSchemaValidator {
        StreamingSchemaValidator::new(Arc::clone(&self.schema))
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
