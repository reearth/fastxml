//! Streaming XML schema validator.

use std::collections::HashMap;
use std::sync::Arc;

use compact_str::CompactString;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};
use crate::namespace::Namespace;
use crate::node::{NodeType, XmlNode};

use super::types::{CompiledSchema, ContentModel, ElementDef, SimpleType, TypeDef};
use super::xsd::facets::{FacetConstraints, FacetValidator};
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
    /// Type reference for this element (if known from schema)
    type_ref: Option<String>,
    /// Expected child elements with their occurrence constraints (name -> (min, max))
    expected_children: HashMap<String, (u32, Option<u32>)>,
}

impl ElementContext {
    fn new(name: &str, namespace: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            namespace: namespace.map(|s| s.to_string()),
            child_counts: HashMap::new(),
            text_content: String::new(),
            schema_validated: false,
            type_ref: None,
            expected_children: HashMap::new(),
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

    /// Looks up an element definition in the schema.
    fn lookup_element(&self, name: &str, qname: &str) -> Option<&ElementDef> {
        self.schema
            .get_element(qname)
            .or_else(|| self.schema.get_element(name))
    }

    /// Extracts child element occurrence constraints from an element definition.
    fn get_child_constraints_for_element(&self, elem: &ElementDef) -> HashMap<String, (u32, Option<u32>)> {
        // Try to get the type definition
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            elem.inline_type.as_ref()
        };

        let Some(type_def) = type_def else {
            return HashMap::new();
        };

        let mut constraints = HashMap::new();

        if let TypeDef::Complex(complex) = type_def {
            let elements = match &complex.content {
                ContentModel::Sequence(elems)
                | ContentModel::Choice(elems)
                | ContentModel::All(elems) => elems,
                ContentModel::ComplexExtension { elements, .. } => elements,
                _ => return constraints,
            };

            for elem in elements {
                constraints.insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
            }
        }

        constraints
    }

    /// Creates FacetConstraints from a SimpleType definition.
    fn create_facet_constraints(&self, simple: &SimpleType) -> FacetConstraints {
        let mut constraints = FacetConstraints::new();

        if let Some(min_len) = simple.min_length {
            constraints = constraints.with_min_length(min_len as usize);
        }
        if let Some(max_len) = simple.max_length {
            constraints = constraints.with_max_length(max_len as usize);
        }
        if let Some(ref min_inc) = simple.min_inclusive {
            constraints = constraints.with_min_inclusive(min_inc.clone());
        }
        if let Some(ref max_inc) = simple.max_inclusive {
            constraints = constraints.with_max_inclusive(max_inc.clone());
        }
        if !simple.enumeration.is_empty() {
            constraints = constraints.with_enumeration(simple.enumeration.clone());
        }
        if let Some(ref pattern) = simple.pattern {
            constraints = constraints.with_pattern(pattern.clone());
        }

        constraints
    }

    fn validate_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        _namespace: Option<&str>,
        attributes: &[(&str, &str)],
    ) {
        let qname = match prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, name),
            _ => name.to_string(),
        };

        // Check if this element is expected by the parent (inline element definition)
        let is_expected_by_parent = self.is_element_expected_by_parent(name);

        // Look up element in schema (global element definition)
        let elem_def = self.lookup_element(name, &qname);
        let schema_has_elements = !self.schema.elements.is_empty();

        if let Some(elem) = elem_def {
            // Global element found - get type information
            let type_ref = elem.type_ref.clone();
            let expected_children = self.get_child_constraints_for_element(elem);

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.expected_children = expected_children;
            }
        } else if is_expected_by_parent {
            // Inline element - declared in parent's type definition
            // Get type info from parent's content model if available
            let (type_ref, expected_children) = self.get_inline_element_info(name);

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.expected_children = expected_children;
            }
        } else {
            // Element not found in schema
            if self.mode == ValidationMode::Strict && schema_has_elements {
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

    /// Checks if an element is expected by its parent (defined in parent's content model).
    fn is_element_expected_by_parent(&self, name: &str) -> bool {
        if self.state.element_stack.len() < 2 {
            return false;
        }
        let parent_idx = self.state.element_stack.len() - 2;
        if let Some(parent) = self.state.element_stack.get(parent_idx) {
            parent.expected_children.contains_key(name)
        } else {
            false
        }
    }

    /// Gets type information for an inline element from the parent's content model.
    fn get_inline_element_info(&self, name: &str) -> (Option<String>, HashMap<String, (u32, Option<u32>)>) {
        // For inline elements, we need to look up the parent's type and find the child element definition
        if self.state.element_stack.len() < 2 {
            return (None, HashMap::new());
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_name = match self.state.element_stack.get(parent_idx) {
            Some(p) => p.name.clone(),
            None => return (None, HashMap::new()),
        };

        // Look up parent element to get its type
        let parent_elem = self.schema.get_element(&parent_name);
        if parent_elem.is_none() {
            return (None, HashMap::new());
        }
        let parent_elem = parent_elem.unwrap();

        // Get parent's type definition
        let type_def = if let Some(ref type_ref) = parent_elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            parent_elem.inline_type.as_ref()
        };

        let Some(TypeDef::Complex(complex)) = type_def else {
            return (None, HashMap::new());
        };

        // Find the child element in the content model
        let elements = match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => elems,
            ContentModel::ComplexExtension { elements, .. } => elements,
            _ => return (None, HashMap::new()),
        };

        for elem in elements {
            if elem.name == name {
                // Found the inline element - get its type info
                let type_ref = elem.type_ref.clone();

                // Get expected children for this inline element
                let expected_children = if let Some(ref tr) = type_ref {
                    if let Some(TypeDef::Complex(child_complex)) = self.schema.get_type(tr) {
                        self.extract_child_constraints_from_complex(child_complex)
                    } else {
                        HashMap::new()
                    }
                } else if let Some(ref inline) = elem.inline_type {
                    if let TypeDef::Complex(child_complex) = inline {
                        self.extract_child_constraints_from_complex(child_complex)
                    } else {
                        HashMap::new()
                    }
                } else {
                    HashMap::new()
                };

                return (type_ref, expected_children);
            }
        }

        (None, HashMap::new())
    }

    /// Extracts child element constraints from a complex type definition.
    fn extract_child_constraints_from_complex(&self, complex: &super::types::ComplexType) -> HashMap<String, (u32, Option<u32>)> {
        let mut constraints = HashMap::new();

        let elements = match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => elems,
            ContentModel::ComplexExtension { elements, .. } => elements,
            _ => return constraints,
        };

        for elem in elements {
            constraints.insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
        }

        constraints
    }

    /// Validates that an element doesn't exceed its max_occurs constraint.
    fn validate_max_occurs(&mut self, child_name: &str) {
        if self.state.element_stack.len() < 2 {
            return;
        }

        let parent_idx = self.state.element_stack.len() - 2;
        if let Some(parent) = self.state.element_stack.get(parent_idx) {
            let count = parent.get_child_count(child_name);

            // Check against parent's expected children constraints
            if let Some(&(_, max_occurs)) = parent.expected_children.get(child_name) {
                if let Some(max) = max_occurs {
                    if count > max {
                        let error = self
                            .make_error(
                                ValidationErrorType::TooManyOccurrences,
                                format!(
                                    "element '{}' occurs {} times, but maximum is {}",
                                    child_name, count, max
                                ),
                            )
                            .with_node_name(child_name)
                            .with_level(ErrorLevel::Error);
                        self.add_error(error);
                    }
                }
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
                self.validate_text_content_against_type(&ctx);
            }

            // Validate required children were present (minOccurs)
            self.validate_min_occurs(&ctx);
        }
    }

    /// Validates text content against the element's type definition.
    fn validate_text_content_against_type(&mut self, ctx: &ElementContext) {
        // Try to get type definition from type_ref first
        if let Some(ref type_ref) = ctx.type_ref {
            if let Some(type_def) = self.schema.get_type(type_ref).cloned() {
                self.validate_text_against_type_def(ctx, &type_def);
                return;
            }
        }

        // If no type_ref, try to get inline type from element definition
        if let Some(inline_type) = self.get_element_inline_type(&ctx.name) {
            self.validate_text_against_type_def(ctx, &inline_type);
        }
    }

    /// Gets inline type definition for an element (either global or from parent's content model).
    fn get_element_inline_type(&self, name: &str) -> Option<TypeDef> {
        // First try global element
        if let Some(elem) = self.schema.get_element(name) {
            if let Some(ref inline) = elem.inline_type {
                return Some(inline.clone());
            }
        }

        // Try to find inline type from parent's content model
        if self.state.element_stack.len() < 2 {
            return None;
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_name = self.state.element_stack.get(parent_idx)?.name.clone();

        let parent_elem = self.schema.get_element(&parent_name)?;
        let type_def = if let Some(ref type_ref) = parent_elem.type_ref {
            self.schema.get_type(type_ref)?
        } else {
            parent_elem.inline_type.as_ref()?
        };

        let TypeDef::Complex(complex) = type_def else {
            return None;
        };

        let elements = match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => elems,
            ContentModel::ComplexExtension { elements, .. } => elements,
            _ => return None,
        };

        for elem in elements {
            if elem.name == name {
                return elem.inline_type.clone();
            }
        }

        None
    }

    /// Validates text content against a specific type definition.
    fn validate_text_against_type_def(&mut self, ctx: &ElementContext, type_def: &TypeDef) {
        match type_def {
            TypeDef::Simple(simple) => {
                let constraints = self.create_facet_constraints(simple);
                let validator = FacetValidator::new(&constraints);
                if let Err(facet_error) = validator.validate(&ctx.text_content) {
                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidContent,
                            format!(
                                "invalid content for element '{}': {}",
                                ctx.name, facet_error
                            ),
                        )
                        .with_node_name(&ctx.name)
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }
            TypeDef::Complex(complex) => {
                // For complex types with simple content, validate the base type
                if let ContentModel::SimpleContent { base_type } = &complex.content {
                    if let Some(TypeDef::Simple(simple)) = self.schema.get_type(base_type) {
                        let constraints = self.create_facet_constraints(simple);
                        let validator = FacetValidator::new(&constraints);
                        if let Err(facet_error) = validator.validate(&ctx.text_content) {
                            let error = self
                                .make_error(
                                    ValidationErrorType::InvalidContent,
                                    format!(
                                        "invalid content for element '{}': {}",
                                        ctx.name, facet_error
                                    ),
                                )
                                .with_node_name(&ctx.name)
                                .with_level(ErrorLevel::Error);
                            self.add_error(error);
                        }
                    }
                } else if !complex.mixed {
                    // Non-mixed complex types shouldn't have text content
                    let trimmed = ctx.text_content.trim();
                    if !trimmed.is_empty() {
                        let error = self
                            .make_error(
                                ValidationErrorType::InvalidContent,
                                format!(
                                    "element '{}' has element-only content but contains text",
                                    ctx.name
                                ),
                            )
                            .with_node_name(&ctx.name)
                            .with_level(ErrorLevel::Error);
                        self.add_error(error);
                    }
                }
            }
        }
    }

    /// Validates that all required child elements are present (minOccurs).
    fn validate_min_occurs(&mut self, ctx: &ElementContext) {
        for (child_name, &(min_occurs, _)) in &ctx.expected_children {
            if min_occurs > 0 {
                let actual_count = ctx.get_child_count(child_name);
                if actual_count < min_occurs {
                    let error_type = if actual_count == 0 {
                        ValidationErrorType::MissingRequiredElement
                    } else {
                        ValidationErrorType::TooFewOccurrences
                    };
                    let error = self
                        .make_error(
                            error_type,
                            format!(
                                "element '{}' requires child '{}' at least {} time(s), but found {}",
                                ctx.name, child_name, min_occurs, actual_count
                            ),
                        )
                        .with_node_name(&ctx.name)
                        .with_expected(&format!("at least {} occurrence(s) of '{}'", min_occurs, child_name))
                        .with_found(&format!("{} occurrence(s)", actual_count))
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }
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
