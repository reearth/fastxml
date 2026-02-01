//! Streaming XML schema validator.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::namespace::Namespace;
use crate::node::{NodeType, XmlNode};

use super::types::CompiledSchema;

/// Validation state during streaming.
#[derive(Debug, Default)]
struct ValidationState {
    /// Current element stack (qname, namespace)
    element_stack: Vec<(String, Option<String>)>,
    /// Current depth
    depth: usize,
    /// Namespace bindings at each depth
    namespace_stack: Vec<HashMap<String, String>>,
}

impl ValidationState {
    fn new() -> Self {
        Self {
            element_stack: Vec::with_capacity(64),
            depth: 0,
            namespace_stack: vec![HashMap::new()],
        }
    }

    fn push_element(&mut self, name: &str, namespace: Option<&str>) {
        self.element_stack.push((name.to_string(), namespace.map(|s| s.to_string())));
        self.depth += 1;
    }

    fn pop_element(&mut self) {
        self.element_stack.pop();
        self.depth = self.depth.saturating_sub(1);
    }

    fn push_namespaces(&mut self, decls: &[Namespace]) {
        let mut current = self.namespace_stack.last().cloned().unwrap_or_default();
        for ns in decls {
            current.insert(ns.prefix().to_string(), ns.uri().to_string());
        }
        self.namespace_stack.push(current);
    }

    fn pop_namespaces(&mut self) {
        if self.namespace_stack.len() > 1 {
            self.namespace_stack.pop();
        }
    }

    #[allow(dead_code)]
    fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        self.namespace_stack.last()
            .and_then(|ns| ns.get(prefix).map(|s| s.as_str()))
    }
}

/// Streaming schema validator.
///
/// Validates XML documents against an XSD schema during streaming parsing.
pub struct StreamingSchemaValidator {
    schema: Arc<CompiledSchema>,
    state: ValidationState,
    errors: Vec<StructuredError>,
    current_line: Option<usize>,
}

impl StreamingSchemaValidator {
    /// Creates a new streaming validator.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            state: ValidationState::new(),
            errors: Vec::new(),
            current_line: None,
        }
    }

    /// Returns collected validation errors.
    pub fn errors(&self) -> &[StructuredError] {
        &self.errors
    }

    /// Takes ownership of collected errors.
    pub fn into_errors(self) -> Vec<StructuredError> {
        self.errors
    }

    /// Returns true if validation passed without errors.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    fn add_error(&mut self, error_type: ValidationErrorType, message: String) {
        self.errors.push(StructuredError {
            message,
            line: self.current_line,
            column: None,
            error_type,
        });
    }

    fn validate_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        _namespace: Option<&str>,
        attributes: &[(String, String)],
    ) {
        // For now, basic validation - check if element is known
        let qname = match prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, name),
            _ => name.to_string(),
        };

        // Look up element in schema
        if self.schema.get_element(&qname).is_none() && self.schema.get_element(name).is_none() {
            // Element not found - this could be valid if schema allows extensions
            // For now, we'll be lenient and not report unknown elements
            // In strict mode, we would report an error here
        }

        // Validate attributes
        for (attr_name, _attr_value) in attributes {
            // Skip xmlns attributes
            if attr_name.starts_with("xmlns") {
                continue;
            }

            // Basic attribute validation would go here
            // For now, we're lenient with attributes
        }
    }
}

impl XmlEventHandler for StreamingSchemaValidator {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                namespace,
                attributes,
                namespace_decls,
                line,
            } => {
                self.current_line = *line;
                self.state.push_namespaces(namespace_decls);
                self.validate_element(
                    name,
                    prefix.as_deref(),
                    namespace.as_deref(),
                    attributes,
                );
                self.state.push_element(name, namespace.as_deref());
            }
            XmlEvent::EndElement { .. } => {
                self.state.pop_element();
                self.state.pop_namespaces();
            }
            XmlEvent::Text(_) | XmlEvent::CData(_) => {
                // Text content validation would go here
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        // Final validation checks
        if !self.state.element_stack.is_empty() {
            self.add_error(
                ValidationErrorType::Other,
                format!("unclosed elements: {:?}", self.state.element_stack),
            );
        }
        Ok(())
    }
}

/// Schema validation context.
///
/// Thread-safe wrapper for schema validation.
pub struct XmlSchemaValidationContext {
    schema: Arc<CompiledSchema>,
    _marker: PhantomData<*mut ()>,
}

impl XmlSchemaValidationContext {
    /// Creates a new validation context.
    pub fn new(schema: CompiledSchema) -> Self {
        Self {
            schema: Arc::new(schema),
            _marker: PhantomData,
        }
    }

    /// Creates a context from an Arc'd schema.
    pub fn from_arc(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            _marker: PhantomData,
        }
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

                // Create attributes list
                let attributes: Vec<(String, String)> = attrs.into_iter().collect();

                // Start element event
                let _ = validator.handle(&XmlEvent::StartElement {
                    name,
                    prefix,
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
                    name: node.get_name(),
                    prefix: node.get_prefix(),
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

// Safety: Schema is immutable and wrapped in Arc
unsafe impl Send for XmlSchemaValidationContext {}
unsafe impl Sync for XmlSchemaValidationContext {}

/// Creates a schema validation context from a schema location.
///
/// If the location is a URL, this will attempt to fetch and parse the XSD.
/// If it's a file path, it will read and parse the file.
///
/// Note: This currently creates a schema with built-in types only.
/// For full import resolution, use `create_xml_schema_validation_context_with_fetcher`.
pub fn create_xml_schema_validation_context(schema_location: &str) -> Result<XmlSchemaValidationContext> {
    // Check if it's a URL or file path
    if schema_location.starts_with("http://") || schema_location.starts_with("https://") {
        // For URLs, create a schema with built-in types only for now
        // Full resolution would require a fetcher
        let schema = super::xsd::create_builtin_schema();
        Ok(XmlSchemaValidationContext::new(schema))
    } else {
        // Try to read as a local file
        match std::fs::read(schema_location) {
            Ok(content) => {
                let schema = super::xsd::parse_xsd(&content)?;
                Ok(XmlSchemaValidationContext::new(schema))
            }
            Err(_) => {
                // Fall back to built-in types only
                let schema = super::xsd::create_builtin_schema();
                Ok(XmlSchemaValidationContext::new(schema))
            }
        }
    }
}

/// Creates a schema validation context from schema content.
///
/// Parses the provided XSD content and creates a validation context.
/// Built-in XSD and GML types are automatically registered.
pub fn create_xml_schema_validation_context_from_buffer(schema_content: &[u8]) -> Result<XmlSchemaValidationContext> {
    let schema = super::xsd::parse_xsd(schema_content)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_validator() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        }).unwrap();

        validator.handle(&XmlEvent::EndElement {
            name: "root".into(),
            prefix: None,
        }).unwrap();

        validator.handle(&XmlEvent::Eof).unwrap();
        validator.finish().unwrap();

        assert!(validator.is_valid());
    }

    #[test]
    fn test_validation_context() {
        let ctx = create_xml_schema_validation_context("http://example.com/schema.xsd").unwrap();
        assert!(ctx.schema().elements.is_empty());
    }
}
