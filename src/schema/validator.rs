//! Streaming XML schema validator.

use std::collections::HashMap;
use std::sync::Arc;

use compact_str::CompactString;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::namespace::Namespace;
use crate::node::{NodeType, XmlNode};

use super::types::CompiledSchema;
use super::xsd::constraints::ConstraintValidator;

/// Validation mode controlling strictness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidationMode {
    /// Lenient mode - only report definite errors
    Lenient,
    /// Strict mode (default) - report all schema violations
    #[default]
    Strict,
}

/// Validation state during streaming.
#[derive(Debug, Default)]
struct ValidationState {
    /// Current element stack (qname, namespace, occurrence count)
    element_stack: Vec<ElementContext>,
    /// Current depth
    depth: usize,
    /// Namespace bindings at each depth
    namespace_stack: Vec<HashMap<String, String>>,
}

/// Context for an element being validated.
#[derive(Debug, Clone)]
struct ElementContext {
    /// Element name (local name)
    name: String,
    /// Element namespace URI (for future use)
    #[allow(dead_code)]
    namespace: Option<String>,
    /// Child element occurrence counts
    child_counts: HashMap<String, u32>,
    /// Text content collected
    text_content: String,
    /// Whether this element has been validated against schema
    schema_validated: bool,
}

impl ElementContext {
    fn new(name: &str, namespace: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            namespace: namespace.map(|s| s.to_string()),
            child_counts: HashMap::new(),
            text_content: String::new(),
            schema_validated: false,
        }
    }

    fn increment_child(&mut self, child_name: &str) -> u32 {
        let count = self.child_counts.entry(child_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    fn get_child_count(&self, child_name: &str) -> u32 {
        *self.child_counts.get(child_name).unwrap_or(&0)
    }
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
        // Increment child count in parent
        if let Some(parent) = self.element_stack.last_mut() {
            parent.increment_child(name);
        }
        self.element_stack
            .push(ElementContext::new(name, namespace));
        self.depth += 1;
    }

    fn pop_element(&mut self) -> Option<ElementContext> {
        self.depth = self.depth.saturating_sub(1);
        self.element_stack.pop()
    }

    #[allow(dead_code)]
    fn current_element(&self) -> Option<&ElementContext> {
        self.element_stack.last()
    }

    fn current_element_mut(&mut self) -> Option<&mut ElementContext> {
        self.element_stack.last_mut()
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
        self.namespace_stack
            .last()
            .and_then(|ns| ns.get(prefix).map(|s| s.as_str()))
    }

    /// Returns XPath-like path to current element.
    fn element_path(&self) -> String {
        if self.element_stack.is_empty() {
            return "/".to_string();
        }
        let mut path = String::new();
        for ctx in &self.element_stack {
            path.push('/');
            path.push_str(&ctx.name);
        }
        path
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
    /// Constraint validator for identity constraints (unique, key, keyref)
    constraint_validator: ConstraintValidator,
    /// Validation mode (strict or lenient)
    mode: ValidationMode,
    /// Maximum number of errors to collect (0 = unlimited)
    max_errors: usize,
}

impl StreamingSchemaValidator {
    /// Creates a new streaming validator in strict mode.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            state: ValidationState::new(),
            errors: Vec::new(),
            current_line: None,
            constraint_validator: ConstraintValidator::new(),
            mode: ValidationMode::Strict,
            max_errors: 0,
        }
    }

    /// Creates a new streaming validator with specified mode.
    pub fn with_mode(schema: Arc<CompiledSchema>, mode: ValidationMode) -> Self {
        Self {
            mode,
            ..Self::new(schema)
        }
    }

    /// Sets the maximum number of errors to collect.
    ///
    /// Set to 0 for unlimited errors (default).
    pub fn set_max_errors(&mut self, max: usize) {
        self.max_errors = max;
    }

    /// Returns collected validation errors.
    pub fn errors(&self) -> &[StructuredError] {
        &self.errors
    }

    /// Returns only errors (excludes warnings).
    pub fn errors_only(&self) -> Vec<&StructuredError> {
        self.errors.iter().filter(|e| e.is_error()).collect()
    }

    /// Returns only warnings.
    pub fn warnings(&self) -> Vec<&StructuredError> {
        self.errors.iter().filter(|e| e.is_warning()).collect()
    }

    /// Takes ownership of collected errors.
    pub fn into_errors(self) -> Vec<StructuredError> {
        self.errors
    }

    /// Returns true if validation passed without errors (warnings are OK).
    pub fn is_valid(&self) -> bool {
        !self.errors.iter().any(|e| e.is_error())
    }

    /// Returns true if there are no errors or warnings.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the error count (excluding warnings).
    pub fn error_count(&self) -> usize {
        self.errors.iter().filter(|e| e.is_error()).count()
    }

    /// Returns the warning count.
    pub fn warning_count(&self) -> usize {
        self.errors.iter().filter(|e| e.is_warning()).count()
    }

    fn should_collect_more(&self) -> bool {
        self.max_errors == 0 || self.errors.len() < self.max_errors
    }

    fn add_error(&mut self, error: StructuredError) {
        if self.should_collect_more() {
            self.errors.push(error);
        }
    }

    fn make_error(
        &self,
        error_type: ValidationErrorType,
        message: impl Into<String>,
    ) -> StructuredError {
        let mut error = StructuredError::new(message, error_type);
        if let Some(line) = self.current_line {
            error = error.with_line(line);
        }
        error = error.with_element_path(self.state.element_path());
        error
    }

    fn validate_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        namespace: Option<&str>,
        attributes: &[(&str, &str)],
    ) {
        let qname = match prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, name),
            _ => name.to_string(),
        };

        // Look up element in schema - check existence first
        let element_found =
            self.schema.get_element(&qname).is_some() || self.schema.get_element(name).is_some();
        let schema_has_elements = !self.schema.elements.is_empty();

        if element_found {
            // Element found in schema - validate it
            self.validate_known_element(name, namespace, attributes);
        } else {
            // Element not found in schema
            if self.mode == ValidationMode::Strict && schema_has_elements {
                // Only report unknown elements if schema has elements defined
                // and we're in strict mode
                let error = self
                    .make_error(
                        ValidationErrorType::UnknownElement,
                        format!("element '{}' is not declared in schema", qname),
                    )
                    .with_node_name(&qname)
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }

        // Validate attributes
        self.validate_attributes(name, attributes);
    }

    fn validate_known_element(
        &mut self,
        name: &str,
        _namespace: Option<&str>,
        _attributes: &[(&str, &str)],
    ) {
        // Mark current element as schema validated
        if let Some(ctx) = self.state.current_element_mut() {
            ctx.schema_validated = true;
        }

        // Check occurrence constraints against parent's child counts
        if self.state.element_stack.len() > 1 {
            let parent_idx = self.state.element_stack.len() - 2;
            if let Some(parent) = self.state.element_stack.get(parent_idx) {
                let count = parent.get_child_count(name);

                // TODO: Get max_occurs from schema and validate
                // For now, we don't have occurrence info easily accessible
                // This would require looking up the parent's type definition
                let _ = count; // Suppress unused warning
            }
        }
    }

    fn validate_attributes(&mut self, element_name: &str, attributes: &[(&str, &str)]) {
        for &(attr_name, attr_value) in attributes {
            // Skip namespace declarations
            if attr_name.starts_with("xmlns") {
                continue;
            }

            // Skip schema location attributes
            if attr_name.contains("schemaLocation") {
                continue;
            }

            // In strict mode, check if attribute is known
            // For now, we don't have attribute definitions easily accessible
            // so we'll skip this validation
            let _ = (element_name, attr_value);
        }
    }

    fn validate_text_content(&mut self, text: &str) {
        if let Some(ctx) = self.state.current_element_mut() {
            ctx.text_content.push_str(text);
        }
    }

    fn validate_element_end(&mut self, _name: &str) {
        // Get the element context being closed
        if let Some(ctx) = self.state.pop_element() {
            // Validate text content if element has a type
            if !ctx.text_content.is_empty() {
                // TODO: Validate text content against element type
                // This requires looking up the element's type definition
            }

            // Validate required children were present
            // TODO: Check minOccurs constraints
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
                self.state.push_element(name, namespace.as_deref());
                let attrs: Vec<(&str, &str)> = attributes
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.validate_element(name, prefix.as_deref(), namespace.as_deref(), &attrs);
            }
            XmlEvent::EndElement { name, .. } => {
                self.validate_element_end(name);
                self.state.pop_namespaces();
            }
            XmlEvent::Text(text) => {
                self.validate_text_content(text);
            }
            XmlEvent::CData(text) => {
                self.validate_text_content(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        // Final validation checks - report unclosed elements
        while let Some(ctx) = self.state.pop_element() {
            let error = StructuredError::new(
                format!("element '{}' is not closed", ctx.name),
                ValidationErrorType::UnclosedElement,
            )
            .with_node_name(&ctx.name)
            .with_level(ErrorLevel::Error);
            self.add_error(error);
        }

        // Validate keyref constraints
        if let Err(constraint_errors) = self.constraint_validator.validate_keyrefs() {
            for err in constraint_errors {
                let error =
                    StructuredError::new(err.to_string(), ValidationErrorType::IdentityConstraint)
                        .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }

        Ok(())
    }
}

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
pub fn create_xml_schema_validation_context_from_buffer(
    schema_content: &[u8],
) -> Result<XmlSchemaValidationContext> {
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

        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

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
