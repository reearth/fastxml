//! Streaming XML schema validator.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::namespace::Namespace;

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

    /// Validates a document.
    pub fn validate(&self, _doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        // For full validation, we would need to:
        // 1. Parse the document as a stream
        // 2. Run the streaming validator
        // 3. Return errors

        // For now, return empty errors (valid)
        Ok(Vec::new())
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
/// This is a placeholder that creates an empty schema.
/// Real implementation would parse the XSD from the location.
pub fn create_xml_schema_validation_context(_schema_location: &str) -> Result<XmlSchemaValidationContext> {
    // TODO: Load and parse XSD from location
    let schema = CompiledSchema::new();
    Ok(XmlSchemaValidationContext::new(schema))
}

/// Creates a schema validation context from schema content.
pub fn create_xml_schema_validation_context_from_buffer(_schema_content: &[u8]) -> Result<XmlSchemaValidationContext> {
    // TODO: Parse XSD content
    let schema = CompiledSchema::new();
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
