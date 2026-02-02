//! Streaming schema validator implementation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};

use crate::schema::types::{CompiledSchema, ComplexType, ContentModel, ElementDef, SimpleType, TypeDef};
use crate::schema::xsd::constraints::ConstraintValidator;
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

use super::ValidationMode;
use super::state::{ChildConstraints, ContentModelType, ElementContext, ValidationState};

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

    /// Collects all child elements from a complex type, including inherited elements.
    ///
    /// This function follows the type inheritance chain (via ComplexExtension) to collect
    /// all valid child elements, not just those defined directly on the type.
    fn collect_elements_with_inheritance(
        &self,
        complex: &ComplexType,
        visited: &mut std::collections::HashSet<String>,
    ) -> Vec<ElementDef> {
        let mut elements = Vec::new();

        match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => {
                elements.extend(elems.iter().cloned());
            }
            ContentModel::ComplexExtension {
                base_type,
                elements: ext_elements,
            } => {
                // First, get elements from the base type (inherited elements)
                if !visited.contains(base_type.as_str()) {
                    visited.insert(base_type.clone());
                    if let Some(TypeDef::Complex(base_complex)) = self.schema.get_type(base_type.as_str()) {
                        let base_elements =
                            self.collect_elements_with_inheritance(base_complex, visited);
                        elements.extend(base_elements);
                    }
                }
                // Then add the extension's own elements
                elements.extend(ext_elements.iter().cloned());
            }
            _ => {}
        }

        elements
    }

    /// Extracts child element occurrence constraints from an element definition.
    ///
    /// This includes inherited elements from base types when using ComplexExtension.
    fn get_child_constraints_for_element(
        &self,
        elem: &ElementDef,
    ) -> (ChildConstraints, ContentModelType) {
        // Try to get the type definition
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            elem.inline_type.as_ref()
        };

        let Some(type_def) = type_def else {
            return (HashMap::new(), ContentModelType::Empty);
        };

        let mut constraints = HashMap::new();
        let mut content_model_type = ContentModelType::Empty;

        if let TypeDef::Complex(complex) = type_def {
            // Determine content model type
            content_model_type = match &complex.content {
                ContentModel::Sequence(_) => ContentModelType::Sequence,
                ContentModel::Choice(_) => ContentModelType::Choice,
                ContentModel::All(_) => ContentModelType::All,
                ContentModel::ComplexExtension { .. } => ContentModelType::Sequence, // Extension acts like sequence
                ContentModel::Empty => ContentModelType::Empty,
                ContentModel::SimpleContent { .. } => ContentModelType::Empty,
                ContentModel::Any { .. } => ContentModelType::Sequence, // Treat Any as permissive sequence
            };

            // Collect all elements including inherited ones
            let mut visited = std::collections::HashSet::new();
            let elements = self.collect_elements_with_inheritance(complex, &mut visited);

            for elem in &elements {
                constraints.insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
            }
        }

        (constraints, content_model_type)
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
            let (expected_children, content_model_type) =
                self.get_child_constraints_for_element(elem);

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.expected_children = expected_children;
                ctx.content_model_type = content_model_type;
            }
        } else if is_expected_by_parent {
            // Inline element - declared in parent's type definition
            // Get type info from parent's content model if available
            let (type_ref, expected_children, content_model_type) =
                self.get_inline_element_info(name);

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.expected_children = expected_children;
                ctx.content_model_type = content_model_type;
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
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
    fn get_inline_element_info(
        &self,
        name: &str,
    ) -> (Option<String>, ChildConstraints, ContentModelType) {
        // For inline elements, we need to look up the parent's type and find the child element definition
        if self.state.element_stack.len() < 2 {
            return (None, HashMap::new(), ContentModelType::Empty);
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_name = match self.state.element_stack.get(parent_idx) {
            Some(p) => p.name.clone(),
            None => return (None, HashMap::new(), ContentModelType::Empty),
        };

        // Look up parent element to get its type
        let parent_elem = self.schema.get_element(&parent_name);
        if parent_elem.is_none() {
            return (None, HashMap::new(), ContentModelType::Empty);
        }
        let parent_elem = parent_elem.unwrap();

        // Get parent's type definition
        let type_def = if let Some(ref type_ref) = parent_elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            parent_elem.inline_type.as_ref()
        };

        let Some(TypeDef::Complex(complex)) = type_def else {
            return (None, HashMap::new(), ContentModelType::Empty);
        };

        // Collect all elements including inherited ones
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        for elem in &elements {
            if elem.name == name {
                // Found the inline element - get its type info
                let type_ref = elem.type_ref.clone();

                // Get expected children and content model type for this inline element
                let (expected_children, content_model_type) = if let Some(ref tr) = type_ref {
                    if let Some(TypeDef::Complex(child_complex)) = self.schema.get_type(tr) {
                        self.extract_child_constraints_from_complex(child_complex)
                    } else {
                        (HashMap::new(), ContentModelType::Empty)
                    }
                } else if let Some(ref inline) = elem.inline_type {
                    if let TypeDef::Complex(child_complex) = inline {
                        self.extract_child_constraints_from_complex(child_complex)
                    } else {
                        (HashMap::new(), ContentModelType::Empty)
                    }
                } else {
                    (HashMap::new(), ContentModelType::Empty)
                };

                return (type_ref, expected_children, content_model_type);
            }
        }

        (None, HashMap::new(), ContentModelType::Empty)
    }

    /// Extracts child element constraints from a complex type definition.
    ///
    /// This includes inherited elements from base types when using ComplexExtension.
    fn extract_child_constraints_from_complex(
        &self,
        complex: &ComplexType,
    ) -> (ChildConstraints, ContentModelType) {
        let mut constraints = HashMap::new();

        // Determine content model type
        let content_model_type = match &complex.content {
            ContentModel::Sequence(_) => ContentModelType::Sequence,
            ContentModel::Choice(_) => ContentModelType::Choice,
            ContentModel::All(_) => ContentModelType::All,
            ContentModel::ComplexExtension { .. } => ContentModelType::Sequence,
            ContentModel::Empty => ContentModelType::Empty,
            ContentModel::SimpleContent { .. } => ContentModelType::Empty,
            ContentModel::Any { .. } => ContentModelType::Sequence,
        };

        // Use the inheritance-aware element collector
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        for elem in &elements {
            constraints.insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
        }

        (constraints, content_model_type)
    }

    /// Validates max_occurs constraint for a child element.
    ///
    /// This method also considers substitution groups. If the child element is a
    /// substitution group member, the constraint from the head element is used,
    /// and the total count includes all members of the substitution group (transitively).
    fn validate_max_occurs(&mut self, child_name: &str) {
        if self.state.element_stack.len() < 2 {
            return;
        }

        let parent_idx = self.state.element_stack.len() - 2;
        if let Some(parent) = self.state.element_stack.get(parent_idx) {
            // First, try to find which substitution group head this element belongs to
            let head_name = self.find_substitution_group_head(child_name);

            // Determine the constraint source (head or direct)
            let constraint_name = head_name.as_deref().unwrap_or(child_name);

            // Check against parent's expected children constraints
            if let Some(&(_, max_occurs)) = parent.expected_children.get(constraint_name) {
                if let Some(max) = max_occurs {
                    // Calculate total count including all substitution group members (transitively)
                    let mut total_count = parent.get_child_count(constraint_name);

                    // Also try with local name if constraint_name has a prefix
                    if let Some((_prefix, local)) = constraint_name.split_once(':') {
                        total_count += parent.get_child_count(local);
                    }

                    // Add counts from all transitive members
                    let all_members = self.get_all_substitution_members(constraint_name);
                    for member in &all_members {
                        total_count += parent.get_child_count(member);
                    }

                    if total_count > max {
                        let error = self
                            .make_error(
                                ValidationErrorType::TooManyOccurrences,
                                format!(
                                    "element '{}' (or substitutes) occurs {} times, but maximum is {}",
                                    constraint_name, total_count, max
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

    /// Finds the substitution group head for a given element name.
    ///
    /// Returns Some(head_name) if the element is a member of a substitution group,
    /// or None if it's not a member of any substitution group.
    fn find_substitution_group_head(&self, element_name: &str) -> Option<String> {
        // Check if the element itself is defined and has a substitution_group
        if let Some(elem) = self.schema.get_element(element_name) {
            if let Some(ref sg) = elem.substitution_group {
                return Some(sg.clone());
            }
        }

        // Also check in substitution_groups map (reverse lookup)
        for (head, members) in &self.schema.substitution_groups {
            if members.contains(&element_name.to_string()) {
                return Some(head.clone());
            }
        }

        None
    }

    /// Gets all substitution group members for a head element, including transitive members.
    ///
    /// For example, if `_Feature` has member `_CityObject`, and `_CityObject` has member
    /// `ReliefFeature`, then this returns both `_CityObject` and `ReliefFeature` for `_Feature`.
    ///
    /// Also handles namespace prefix normalization (e.g., `gml:_Feature` matches `_Feature`).
    fn get_all_substitution_members(&self, head_name: &str) -> Vec<String> {
        let mut all_members = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_transitive_members(head_name, &mut all_members, &mut visited);
        all_members
    }

    /// Helper function to recursively collect substitution group members.
    fn collect_transitive_members(
        &self,
        head_name: &str,
        members: &mut Vec<String>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if visited.contains(head_name) {
            return;
        }
        visited.insert(head_name.to_string());

        // Try multiple name variants:
        // 1. The full name as-is
        // 2. If it has a prefix, also try the local name
        // 3. If it has no prefix, also try common prefixes (gml:, core:, etc.)
        let mut names_to_try = vec![head_name.to_string()];

        if let Some((_prefix, local)) = head_name.split_once(':') {
            // Has prefix -> also try local name
            names_to_try.push(local.to_string());
        } else {
            // No prefix -> also try with common prefixes
            // Search substitution_groups for keys ending with this local name
            for key in self.schema.substitution_groups.keys() {
                if let Some((_prefix, local)) = key.split_once(':') {
                    if local == head_name {
                        names_to_try.push(key.clone());
                    }
                }
            }
        }

        for name in names_to_try {
            if let Some(direct_members) = self.schema.substitution_groups.get(&name) {
                for member in direct_members {
                    if !members.contains(member) {
                        members.push(member.clone());
                    }
                    // Recursively collect members of this member (it might be a head too)
                    self.collect_transitive_members(member, members, visited);
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
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
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

        // Collect all elements including inherited ones
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        for elem in &elements {
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
    ///
    /// This method also considers substitution group members when counting occurrences.
    /// If the expected child is a substitution group head, occurrences of any member
    /// element (including transitive members) are counted toward the head's requirement.
    fn validate_min_occurs(&mut self, ctx: &ElementContext) {
        // For Choice content model, we only need ONE of the choices to be present
        if ctx.content_model_type == ContentModelType::Choice {
            // Check if at least one choice element is present
            let mut any_choice_present = false;

            for child_name in ctx.expected_children.keys() {
                let mut count = ctx.get_child_count(child_name);

                // Also try with local name if child_name has a prefix
                if let Some((_prefix, local)) = child_name.split_once(':') {
                    count += ctx.get_child_count(local);
                }

                // Add counts from substitution group members
                let all_members = self.get_all_substitution_members(child_name);
                for member in &all_members {
                    count += ctx.get_child_count(member);
                }

                if count > 0 {
                    any_choice_present = true;
                    break;
                }
            }

            // Only report error if no choice element is present and there are expected children
            if !any_choice_present && !ctx.expected_children.is_empty() {
                let choices: Vec<_> = ctx.expected_children.keys().cloned().collect();
                let error = self
                    .make_error(
                        ValidationErrorType::MissingRequiredElement,
                        format!(
                            "element '{}' requires one of: {}",
                            ctx.name,
                            choices.join(", ")
                        ),
                    )
                    .with_node_name(&ctx.name)
                    .with_expected(format!("one of: {}", choices.join(", ")))
                    .with_found("none".to_string())
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
            return;
        }

        // For Sequence/All content models, check each element's min_occurs
        for (child_name, &(min_occurs, _)) in &ctx.expected_children {
            if min_occurs > 0 {
                // Calculate actual count including substitution group members (transitively)
                let mut actual_count = ctx.get_child_count(child_name);

                // Also try with local name if child_name has a prefix
                if let Some((_prefix, local)) = child_name.split_once(':') {
                    actual_count += ctx.get_child_count(local);
                }

                // Add counts from all substitution group members (including transitive)
                let all_members = self.get_all_substitution_members(child_name);
                for member in &all_members {
                    actual_count += ctx.get_child_count(member);
                }

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

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
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

    // =============================================
    // Type Inheritance Tests
    // =============================================

    /// Test that inherited elements from base types are recognized.
    ///
    /// This test reproduces the issue where elements like `creationDate` defined
    /// in a base type (e.g., AbstractCityObjectType) are not recognized when
    /// validating an element whose type extends that base type.
    #[test]
    fn test_inherited_elements_from_base_type() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        // Build a schema with type inheritance:
        // - BaseType has element "baseElement" (like creationDate in _CityObject)
        // - ExtendedType extends BaseType and adds "extElement" (like lod in ReliefFeature)
        // - "root" element uses ExtendedType

        let mut schema = CompiledSchema::new();

        // BaseType with "baseElement"
        let mut base_type = ComplexType::new("BaseType");
        base_type.content = ContentModel::Sequence(vec![
            ElementDef::new("baseElement").with_type("xs:string").optional(),
        ]);
        schema.types.insert("BaseType".to_string(), TypeDef::Complex(base_type));

        // ExtendedType extends BaseType, adds "extElement"
        let mut extended_type = ComplexType::new("ExtendedType");
        extended_type.content = ContentModel::ComplexExtension {
            base_type: "BaseType".to_string(),
            elements: vec![
                ElementDef::new("extElement").with_type("xs:integer").optional(),
            ],
        };
        schema.types.insert("ExtendedType".to_string(), TypeDef::Complex(extended_type));

        // Root element uses ExtendedType
        let root_elem = ElementDef::new("root").with_type("ExtendedType");
        schema.elements.insert("root".to_string(), root_elem);

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start root element
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

        // Add inherited element (baseElement) - this should be valid!
        validator
            .handle(&XmlEvent::StartElement {
                name: "baseElement".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(2),
            })
            .unwrap();

        validator
            .handle(&XmlEvent::Text("inherited content".to_string()))
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "baseElement".into(),
                prefix: None,
            })
            .unwrap();

        // Add direct extension element (extElement)
        validator
            .handle(&XmlEvent::StartElement {
                name: "extElement".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(3),
            })
            .unwrap();

        validator
            .handle(&XmlEvent::Text("42".to_string()))
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "extElement".into(),
                prefix: None,
            })
            .unwrap();

        // End root
        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();

        // Check for errors - inherited element should NOT cause an error
        let errors: Vec<_> = validator.errors().iter()
            .filter(|e| e.message.contains("baseElement"))
            .collect();

        assert!(
            errors.is_empty(),
            "Inherited element 'baseElement' should be recognized, but got errors: {:?}",
            errors
        );

        assert!(
            validator.is_valid(),
            "Validation should pass for inherited elements, but got errors: {:?}",
            validator.errors()
        );
    }

    /// Test multi-level type inheritance (grandparent -> parent -> child).
    #[test]
    fn test_multi_level_inheritance() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let mut schema = CompiledSchema::new();

        // GrandparentType has "grandparentElem"
        let mut grandparent_type = ComplexType::new("GrandparentType");
        grandparent_type.content = ContentModel::Sequence(vec![
            ElementDef::new("grandparentElem").with_type("xs:string").optional(),
        ]);
        schema.types.insert("GrandparentType".to_string(), TypeDef::Complex(grandparent_type));

        // ParentType extends GrandparentType, adds "parentElem"
        let mut parent_type = ComplexType::new("ParentType");
        parent_type.content = ContentModel::ComplexExtension {
            base_type: "GrandparentType".to_string(),
            elements: vec![
                ElementDef::new("parentElem").with_type("xs:string").optional(),
            ],
        };
        schema.types.insert("ParentType".to_string(), TypeDef::Complex(parent_type));

        // ChildType extends ParentType, adds "childElem"
        let mut child_type = ComplexType::new("ChildType");
        child_type.content = ContentModel::ComplexExtension {
            base_type: "ParentType".to_string(),
            elements: vec![
                ElementDef::new("childElem").with_type("xs:string").optional(),
            ],
        };
        schema.types.insert("ChildType".to_string(), TypeDef::Complex(child_type));

        // Root element uses ChildType
        let root_elem = ElementDef::new("root").with_type("ChildType");
        schema.elements.insert("root".to_string(), root_elem);

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start root
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

        // Add grandparent-level element
        validator
            .handle(&XmlEvent::StartElement {
                name: "grandparentElem".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(2),
            })
            .unwrap();
        validator.handle(&XmlEvent::Text("gp".to_string())).unwrap();
        validator
            .handle(&XmlEvent::EndElement {
                name: "grandparentElem".into(),
                prefix: None,
            })
            .unwrap();

        // Add parent-level element
        validator
            .handle(&XmlEvent::StartElement {
                name: "parentElem".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(3),
            })
            .unwrap();
        validator.handle(&XmlEvent::Text("p".to_string())).unwrap();
        validator
            .handle(&XmlEvent::EndElement {
                name: "parentElem".into(),
                prefix: None,
            })
            .unwrap();

        // Add child-level element
        validator
            .handle(&XmlEvent::StartElement {
                name: "childElem".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(4),
            })
            .unwrap();
        validator.handle(&XmlEvent::Text("c".to_string())).unwrap();
        validator
            .handle(&XmlEvent::EndElement {
                name: "childElem".into(),
                prefix: None,
            })
            .unwrap();

        // End root
        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();

        // All three elements should be valid (inherited from different levels)
        assert!(
            validator.is_valid(),
            "Multi-level inheritance should work, but got errors: {:?}",
            validator.errors()
        );
    }

    // =============================================
    // Substitution Group Tests
    // =============================================

    /// Test that substitution group members can be used in place of the head element.
    ///
    /// This test reproduces the issue where elements like `dem:ReliefFeature` are not
    /// recognized as valid substitutes for abstract elements like `_CityObject`.
    #[test]
    fn test_substitution_group_basic() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let mut schema = CompiledSchema::new();

        // Define a parent type that expects "_CityObject" (abstract head element) as REQUIRED
        let mut parent_type = ComplexType::new("ParentType");
        parent_type.content = ContentModel::Sequence(vec![
            // Parent expects "_CityObject" as required child (min_occurs=1)
            ElementDef::new("_CityObject").with_type("AbstractCityObjectType"),
        ]);
        schema.types.insert("ParentType".to_string(), TypeDef::Complex(parent_type));

        // Define the abstract type
        let abstract_type = ComplexType::new("AbstractCityObjectType");
        schema.types.insert("AbstractCityObjectType".to_string(), TypeDef::Complex(abstract_type));

        // Define the concrete type
        let concrete_type = ComplexType::new("ReliefFeatureType");
        schema.types.insert("ReliefFeatureType".to_string(), TypeDef::Complex(concrete_type));

        // Define the head element (abstract)
        let mut head_elem = ElementDef::new("_CityObject");
        head_elem.is_abstract = true;
        head_elem.type_ref = Some("AbstractCityObjectType".to_string());
        schema.elements.insert("_CityObject".to_string(), head_elem);

        // Define the substitute element (concrete)
        let mut substitute_elem = ElementDef::new("ReliefFeature");
        substitute_elem.type_ref = Some("ReliefFeatureType".to_string());
        substitute_elem.substitution_group = Some("_CityObject".to_string());
        schema.elements.insert("ReliefFeature".to_string(), substitute_elem);

        // Define parent element
        let parent_elem = ElementDef::new("parent").with_type("ParentType");
        schema.elements.insert("parent".to_string(), parent_elem);

        // Build substitution groups (head -> members)
        schema.substitution_groups.insert(
            "_CityObject".to_string(),
            vec!["ReliefFeature".to_string()],
        );

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start parent element
        validator
            .handle(&XmlEvent::StartElement {
                name: "parent".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
            })
            .unwrap();

        // Use substitute element (ReliefFeature instead of _CityObject)
        validator
            .handle(&XmlEvent::StartElement {
                name: "ReliefFeature".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(2),
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "ReliefFeature".into(),
                prefix: None,
            })
            .unwrap();

        // End parent
        validator
            .handle(&XmlEvent::EndElement {
                name: "parent".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();

        // Check: ReliefFeature should be accepted as a substitute for _CityObject
        let errors: Vec<_> = validator.errors().iter()
            .filter(|e| e.message.contains("ReliefFeature") && e.message.contains("not declared"))
            .collect();

        assert!(
            errors.is_empty(),
            "Substitution group member 'ReliefFeature' should be accepted in place of '_CityObject', but got errors: {:?}",
            errors
        );

        assert!(
            validator.is_valid(),
            "Validation should pass for substitution group members, but got errors: {:?}",
            validator.errors()
        );
    }

    /// Test that max_occurs is correctly validated for substitution groups.
    ///
    /// When multiple substitution group members are used, their counts should be
    /// summed when checking against the max_occurs constraint.
    #[test]
    fn test_substitution_group_max_occurs() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let mut schema = CompiledSchema::new();

        // Define a parent type that expects "_CityObject" with max_occurs=2
        let mut parent_type = ComplexType::new("ParentType");
        parent_type.content = ContentModel::Sequence(vec![
            // Parent expects "_CityObject" at most 2 times
            ElementDef::new("_CityObject")
                .with_type("AbstractCityObjectType")
                .with_occurs(0, Some(2)),
        ]);
        schema.types.insert("ParentType".to_string(), TypeDef::Complex(parent_type));

        // Define types
        let abstract_type = ComplexType::new("AbstractCityObjectType");
        schema.types.insert("AbstractCityObjectType".to_string(), TypeDef::Complex(abstract_type));

        let relief_type = ComplexType::new("ReliefFeatureType");
        schema.types.insert("ReliefFeatureType".to_string(), TypeDef::Complex(relief_type));

        let building_type = ComplexType::new("BuildingType");
        schema.types.insert("BuildingType".to_string(), TypeDef::Complex(building_type));

        // Define elements
        let mut head_elem = ElementDef::new("_CityObject");
        head_elem.is_abstract = true;
        head_elem.type_ref = Some("AbstractCityObjectType".to_string());
        schema.elements.insert("_CityObject".to_string(), head_elem);

        let mut relief_elem = ElementDef::new("ReliefFeature");
        relief_elem.type_ref = Some("ReliefFeatureType".to_string());
        relief_elem.substitution_group = Some("_CityObject".to_string());
        schema.elements.insert("ReliefFeature".to_string(), relief_elem);

        let mut building_elem = ElementDef::new("Building");
        building_elem.type_ref = Some("BuildingType".to_string());
        building_elem.substitution_group = Some("_CityObject".to_string());
        schema.elements.insert("Building".to_string(), building_elem);

        let parent_elem = ElementDef::new("parent").with_type("ParentType");
        schema.elements.insert("parent".to_string(), parent_elem);

        // Build substitution groups
        schema.substitution_groups.insert(
            "_CityObject".to_string(),
            vec!["ReliefFeature".to_string(), "Building".to_string()],
        );

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start parent
        validator
            .handle(&XmlEvent::StartElement {
                name: "parent".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
            })
            .unwrap();

        // Add 3 substitutes (exceeds max_occurs=2)
        for (i, name) in ["ReliefFeature", "Building", "ReliefFeature"].iter().enumerate() {
            validator
                .handle(&XmlEvent::StartElement {
                    name: (*name).into(),
                    prefix: None,
                    namespace: None,
                    attributes: vec![],
                    namespace_decls: vec![],
                    line: Some(i + 2),
                })
                .unwrap();
            validator
                .handle(&XmlEvent::EndElement {
                    name: (*name).into(),
                    prefix: None,
                })
                .unwrap();
        }

        // End parent
        validator
            .handle(&XmlEvent::EndElement {
                name: "parent".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();

        // Check: Should have a max_occurs error since we have 3 substitutes but max is 2
        let errors: Vec<_> = validator.errors().iter()
            .filter(|e| e.message.contains("occurs") && e.message.contains("maximum"))
            .collect();

        assert!(
            !errors.is_empty(),
            "Should have a max_occurs error when 3 substitutes are used but max is 2, errors: {:?}",
            validator.errors()
        );
    }

    // =============================================
    // Choice Content Model Tests
    // =============================================

    /// Test that Choice content model accepts any one of the choices.
    ///
    /// This test reproduces the issue where `boundedBy` requires `Envelope` OR `Null`,
    /// but the validator incorrectly requires both when using Choice content model.
    #[test]
    fn test_choice_content_model_basic() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let mut schema = CompiledSchema::new();

        // Define a type with Choice content model (like BoundingShapeType)
        // Choice means: ONE of the elements should be present, not ALL
        let mut choice_type = ComplexType::new("BoundingShapeType");
        choice_type.content = ContentModel::Choice(vec![
            ElementDef::new("Envelope").with_type("xs:string"),
            ElementDef::new("Null").with_type("xs:string"),
        ]);
        schema.types.insert("BoundingShapeType".to_string(), TypeDef::Complex(choice_type));

        // Define parent element that uses the choice type
        let parent_elem = ElementDef::new("boundedBy").with_type("BoundingShapeType");
        schema.elements.insert("boundedBy".to_string(), parent_elem);

        let mut validator = StreamingSchemaValidator::new(Arc::new(schema));

        // Start boundedBy
        validator
            .handle(&XmlEvent::StartElement {
                name: "boundedBy".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
            })
            .unwrap();

        // Add Envelope (one of the choices)
        validator
            .handle(&XmlEvent::StartElement {
                name: "Envelope".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(2),
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "Envelope".into(),
                prefix: None,
            })
            .unwrap();

        // End boundedBy
        validator
            .handle(&XmlEvent::EndElement {
                name: "boundedBy".into(),
                prefix: None,
            })
            .unwrap();

        validator.finish().unwrap();

        // Check: Should NOT have an error about missing 'Null' element
        // because Choice means ONE of the options, not ALL
        let errors: Vec<_> = validator.errors().iter()
            .filter(|e| e.message.contains("Null") && e.message.contains("requires"))
            .collect();

        assert!(
            errors.is_empty(),
            "Choice content model should accept any ONE of the choices, not require ALL. Got errors: {:?}",
            errors
        );

        assert!(
            validator.is_valid(),
            "Validation should pass when one choice element is present, but got errors: {:?}",
            validator.errors()
        );
    }
}
