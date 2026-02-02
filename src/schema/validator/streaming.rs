//! Streaming schema validator implementation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};

use crate::schema::types::{CompiledSchema, ContentModel, ElementDef, SimpleType, TypeDef};
use crate::schema::xsd::constraints::ConstraintValidator;
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

use super::ValidationMode;
use super::state::{ChildConstraints, ElementContext, ValidationState};

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

    pub(crate) fn add_error(&mut self, error: StructuredError) {
        if self.should_collect_more() {
            self.errors.push(error);
        }
    }

    pub(crate) fn make_error(
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
    fn get_child_constraints_for_element(&self, elem: &ElementDef) -> ChildConstraints {
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
    fn get_inline_element_info(&self, name: &str) -> (Option<String>, ChildConstraints) {
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
    fn extract_child_constraints_from_complex(
        &self,
        complex: &crate::schema::types::ComplexType,
    ) -> ChildConstraints {
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
                        .with_expected(format!("at least {} occurrence(s) of '{}'", min_occurs, child_name))
                        .with_found(format!("{} occurrence(s)", actual_count))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::namespace::Namespace;
    use crate::schema::types::ComplexType;

    // =============================================
    // ValidationMode Tests
    // =============================================

    #[test]
    fn test_validation_mode_default() {
        let mode = ValidationMode::default();
        assert_eq!(mode, ValidationMode::Strict);
    }

    // =============================================
    // ValidationState Tests
    // =============================================

    #[test]
    fn test_validation_state_new() {
        let state = ValidationState::new();
        assert!(state.element_stack.is_empty());
        assert_eq!(state.depth, 0);
        assert_eq!(state.namespace_stack.len(), 1);
    }

    #[test]
    fn test_validation_state_push_pop_element() {
        let mut state = ValidationState::new();

        state.push_element("root", None);
        assert_eq!(state.depth, 1);
        assert_eq!(state.element_stack.len(), 1);
        assert_eq!(state.element_stack[0].name, "root");

        state.push_element("child", Some("http://example.com"));
        assert_eq!(state.depth, 2);
        assert_eq!(state.element_stack.len(), 2);

        let popped = state.pop_element().unwrap();
        assert_eq!(popped.name, "child");
        assert_eq!(state.depth, 1);
    }

    #[test]
    fn test_validation_state_element_path() {
        let mut state = ValidationState::new();
        assert_eq!(state.element_path(), "/");

        state.push_element("root", None);
        assert_eq!(state.element_path(), "/root");

        state.push_element("child", None);
        assert_eq!(state.element_path(), "/root/child");
    }

    // =============================================
    // ElementContext Tests
    // =============================================

    #[test]
    fn test_element_context_new() {
        let ctx = ElementContext::new("test", Some("http://example.com"));
        assert_eq!(ctx.name, "test");
        assert_eq!(ctx.namespace, Some("http://example.com".to_string()));
        assert!(ctx.child_counts.is_empty());
        assert!(ctx.text_content.is_empty());
        assert!(!ctx.schema_validated);
    }

    #[test]
    fn test_element_context_child_counts() {
        let mut ctx = ElementContext::new("parent", None);

        assert_eq!(ctx.get_child_count("child1"), 0);

        assert_eq!(ctx.increment_child("child1"), 1);
        assert_eq!(ctx.get_child_count("child1"), 1);

        assert_eq!(ctx.increment_child("child1"), 2);
        assert_eq!(ctx.get_child_count("child1"), 2);

        assert_eq!(ctx.increment_child("child2"), 1);
        assert_eq!(ctx.get_child_count("child2"), 1);
    }

    // =============================================
    // StreamingSchemaValidator Tests
    // =============================================

    #[test]
    fn test_streaming_validator_new() {
        let schema = CompiledSchema::new();
        let validator = StreamingSchemaValidator::new(Arc::new(schema));
        assert!(validator.is_valid());
        assert!(validator.is_clean());
        assert_eq!(validator.error_count(), 0);
    }

    #[test]
    fn test_streaming_validator_with_mode() {
        let schema = CompiledSchema::new();
        let validator =
            StreamingSchemaValidator::with_mode(Arc::new(schema), ValidationMode::Lenient);
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_max_errors() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));
        validator.set_max_errors(2);

        // Add 3 errors
        validator.add_error(StructuredError::new("error1", ValidationErrorType::Other));
        validator.add_error(StructuredError::new("error2", ValidationErrorType::Other));
        validator.add_error(StructuredError::new("error3", ValidationErrorType::Other));

        // Should only have 2 errors
        assert_eq!(validator.errors().len(), 2);
    }

    #[test]
    fn test_streaming_validator_make_error() {
        let schema = CompiledSchema::new();
        let validator = StreamingSchemaValidator::new(Arc::new(schema));

        let error = validator.make_error(ValidationErrorType::UnknownElement, "test error");
        assert_eq!(error.message, "test error");
        assert_eq!(error.error_type, ValidationErrorType::UnknownElement);
    }

    #[test]
    fn test_streaming_validator_errors_and_warnings() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator.add_error(
            StructuredError::new("error1", ValidationErrorType::Other)
                .with_level(ErrorLevel::Error),
        );
        validator.add_error(
            StructuredError::new("warning1", ValidationErrorType::Other)
                .with_level(ErrorLevel::Warning),
        );
        validator.add_error(
            StructuredError::new("error2", ValidationErrorType::Other)
                .with_level(ErrorLevel::Error),
        );

        assert_eq!(validator.error_count(), 2);
        assert_eq!(validator.warning_count(), 1);
        assert_eq!(validator.errors_only().len(), 2);
        assert_eq!(validator.warnings().len(), 1);
        assert!(!validator.is_valid());
        assert!(!validator.is_clean());
    }

    #[test]
    fn test_streaming_validator_with_schema_elements() {
        use crate::schema::types::{ElementDef, SimpleType, TypeDef};

        let mut schema = CompiledSchema::new();

        // Add a simple element definition
        schema.elements.insert(
            "root".to_string(),
            ElementDef::new("root").with_type("xs:string"),
        );

        // Add type definition
        schema.types.insert(
            "xs:string".to_string(),
            TypeDef::Simple(SimpleType::new("xs:string")),
        );

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Valid element
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "root".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        });

        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_unknown_element_strict() {
        use crate::schema::types::ElementDef;

        let mut schema = CompiledSchema::new();

        // Add at least one element so schema has elements
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        let _ = validator.handle(&XmlEvent::StartElement {
            name: "unknown".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        });

        // Should have an error for unknown element in strict mode
        assert!(!validator.is_valid());
        assert!(
            validator
                .errors()
                .iter()
                .any(|e| e.message.contains("unknown"))
        );
    }

    #[test]
    fn test_streaming_validator_unknown_element_lenient() {
        use crate::schema::types::ElementDef;

        let mut schema = CompiledSchema::new();

        // Add at least one element so schema has elements
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let mut validator =
            StreamingSchemaValidator::with_mode(Arc::new(schema), ValidationMode::Lenient);

        let _ = validator.handle(&XmlEvent::StartElement {
            name: "unknown".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        });

        // Should NOT have an error in lenient mode
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_text_content() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        let _ = validator.handle(&XmlEvent::StartElement {
            name: "test".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: None,
        });

        let _ = validator.handle(&XmlEvent::Text("content".to_string()));

        // Check that text was collected
        let ctx = validator.state.current_element().unwrap();
        assert_eq!(ctx.text_content, "content");
    }

    #[test]
    fn test_streaming_validator_cdata_content() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        let _ = validator.handle(&XmlEvent::StartElement {
            name: "test".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: None,
        });

        let _ = validator.handle(&XmlEvent::CData("cdata content".to_string()));

        // Check that CDATA was collected as text
        let ctx = validator.state.current_element().unwrap();
        assert_eq!(ctx.text_content, "cdata content");
    }

    #[test]
    fn test_streaming_validator_finish_unclosed_element() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start element but don't close it
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "unclosed".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: None,
        });

        let _ = validator.finish();

        // Should report unclosed element
        assert!(!validator.is_valid());
        assert!(
            validator
                .errors()
                .iter()
                .any(|e| e.message.contains("not closed"))
        );
    }

    #[test]
    fn test_streaming_validator_into_errors() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator.add_error(StructuredError::new(
            "test error",
            ValidationErrorType::Other,
        ));

        let errors = validator.into_errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "test error");
    }

    #[test]
    fn test_streaming_validator_min_occurs() {
        use crate::schema::types::{ElementDef, TypeDef};

        let mut schema = CompiledSchema::new();

        // Create a complex type with required child
        let complex_type = ComplexType {
            name: "ParentType".to_string(),
            base_type: None,
            content: ContentModel::Sequence(vec![
                ElementDef::new("required_child").with_occurs(1, Some(1)),
            ]),
            attributes: Vec::new(),
            is_abstract: false,
            mixed: false,
        };

        schema.elements.insert(
            "parent".to_string(),
            ElementDef::new("parent").with_type("ParentType"),
        );

        schema
            .types
            .insert("ParentType".to_string(), TypeDef::Complex(complex_type));

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start parent element
        let _ = validator.handle(&XmlEvent::StartElement {
            name: "parent".into(),
            prefix: None,
            namespace: None,
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(1),
        });

        // End parent without adding required child
        let _ = validator.handle(&XmlEvent::EndElement {
            name: "parent".into(),
            prefix: None,
        });

        // Should have error about missing required child
        assert!(!validator.is_valid());
        assert!(
            validator
                .errors()
                .iter()
                .any(|e| e.message.contains("required_child"))
        );
    }

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
    fn test_streaming_validator_set_max_errors() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));
        validator.set_max_errors(5);
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_errors_methods() {
        let schema = CompiledSchema::new();
        let validator = StreamingSchemaValidator::new(Arc::new(schema));
        assert!(validator.errors().is_empty());
        assert!(validator.errors_only().is_empty());
        assert!(validator.warnings().is_empty());
        assert_eq!(validator.error_count(), 0);
        assert_eq!(validator.warning_count(), 0);
    }

    #[test]
    fn test_streaming_validator_is_clean() {
        let schema = CompiledSchema::new();
        let validator = StreamingSchemaValidator::new(Arc::new(schema));
        assert!(validator.is_clean());
    }

    #[test]
    fn test_streaming_validator_with_prefix() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: Some("ns".into()),
                namespace: Some("http://example.com".to_string()),
                attributes: vec![],
                namespace_decls: vec![Namespace::new(
                    "ns".to_string(),
                    "http://example.com".to_string(),
                )],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: Some("ns".into()),
            })
            .unwrap();

        validator.finish().unwrap();
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_with_attributes() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![
                    ("id".into(), "1".into()),
                    ("xmlns:ns".into(), "http://example.com".into()),
                    (
                        "xsi:schemaLocation".into(),
                        "http://example.com schema.xsd".into(),
                    ),
                ],
                namespace_decls: vec![],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_nested_elements() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::StartElement {
                name: "child".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::StartElement {
                name: "grandchild".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::Text("content".to_string()))
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "grandchild".into(),
                prefix: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "child".into(),
                prefix: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();
        assert!(validator.is_valid());
    }

    #[test]
    fn test_streaming_validator_other_events() {
        let schema = CompiledSchema::new();
        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // ProcessingInstruction
        validator
            .handle(&XmlEvent::ProcessingInstruction {
                target: "xml".to_string(),
                content: Some("version=\"1.0\"".to_string()),
            })
            .unwrap();

        // Comment
        validator
            .handle(&XmlEvent::Comment("This is a comment".to_string()))
            .unwrap();

        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();
        assert!(validator.is_valid());
    }
}
