//! Validation logic for two-pass schema validation.

use std::sync::Arc;

use crate::error::{ErrorLevel, StructuredError, ValidationErrorType};
use crate::schema::types::{
    ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren, SimpleType, TypeDef,
};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};
use crate::schema::xsd::primitive::PrimitiveKind;

use super::super::ValidationMode;
#[allow(deprecated)]
use super::TwoPassSchemaValidator;
use super::skeleton::{DocumentSkeleton, ElementSkeleton};

#[allow(deprecated)]
impl TwoPassSchemaValidator {
    /// Recursively validates a node and its children.
    ///
    /// `parent_allowed_children` contains the set of child element names allowed by the parent's type.
    pub(crate) fn validate_node_recursive(
        &self,
        skeleton: &DocumentSkeleton,
        node_index: usize,
        parent_allowed_children: Option<&FlattenedChildren>,
        errors: &mut Vec<StructuredError>,
    ) {
        // Check max errors
        if self.max_errors > 0 && errors.len() >= self.max_errors {
            return;
        }

        let node = match skeleton.get_node(node_index) {
            Some(n) => n,
            None => return,
        };

        // Look up element definition (global or from parent's type)
        let elem_def = self.lookup_element(&node.name, node.prefix.as_ref());

        let schema_has_elements = !self.schema.elements.is_empty();

        // Check if element is allowed by parent's type definition
        let is_allowed_by_parent = parent_allowed_children
            .map(|fc| fc.constraints.contains_key(node.name.as_ref()))
            .unwrap_or(false);

        let allowed_children = if let Some(elem) = elem_def {
            // Get flattened children for validation
            let flattened = self.get_flattened_children_for_element(elem);
            if let Some(ref fc) = flattened {
                // Validate min_occurs for all children
                self.validate_min_occurs_batch(node, fc, errors);

                // Validate max_occurs for all children
                self.validate_max_occurs_batch(node, fc, errors);

                // Validate sequence order for sequence content models
                self.validate_sequence_order(skeleton, node, fc, errors);
            }

            // Validate text content
            self.validate_text_content(node, elem, errors);

            flattened
        } else if is_allowed_by_parent {
            // Element is defined inline in parent's type - not an error
            // Try to get inline element definition from parent's constraints
            None
        } else if self.mode == ValidationMode::Strict && schema_has_elements {
            // Unknown element
            let qname = match &node.prefix {
                Some(p) => format!("{}:{}", p.as_ref(), node.name.as_ref()),
                None => node.name.to_string(),
            };

            let error = self
                .make_error(
                    ValidationErrorType::UnknownElement,
                    format!("element '{}' is not declared in schema", qname),
                    node,
                )
                .with_node_name(&qname)
                .with_level(ErrorLevel::Error);

            if self.should_add_error(errors) {
                errors.push(error);
            }
            None
        } else {
            None
        };

        // Validate children recursively with this element's allowed children
        for &child_index in &node.children_indices {
            self.validate_node_recursive(
                skeleton,
                child_index,
                allowed_children.as_deref(),
                errors,
            );
        }
    }

    /// Looks up an element definition in the schema.
    pub(crate) fn lookup_element(
        &self,
        name: &Arc<str>,
        prefix: Option<&Arc<str>>,
    ) -> Option<&ElementDef> {
        // Try local name first
        if let Some(elem) = self.schema.get_element(name.as_ref()) {
            return Some(elem);
        }

        // Try with prefix
        if let Some(p) = prefix {
            if !p.is_empty() {
                let qname = format!("{}:{}", p.as_ref(), name.as_ref());
                if let Some(elem) = self.schema.get_element(&qname) {
                    return Some(elem);
                }
            }
        }

        None
    }

    /// Gets flattened children for an element from the schema cache.
    pub(crate) fn get_flattened_children_for_element(
        &self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // Try type reference first
        if let Some(ref type_ref) = elem.type_ref {
            if let Some(cached) = self.schema.type_children_cache.get(type_ref) {
                return Some(Arc::clone(cached));
            }

            // Try without prefix
            if let Some((_prefix, local)) = type_ref.split_once(':') {
                if let Some(cached) = self.schema.type_children_cache.get(local) {
                    return Some(Arc::clone(cached));
                }
            }

            // Compute at runtime if not cached
            if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        // Try inline type
        if let Some(ref inline_type) = elem.inline_type {
            if let TypeDef::Complex(complex) = inline_type {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        None
    }

    /// Computes flattened children for a complex type.
    pub(crate) fn compute_flattened_children(&self, complex: &ComplexType) -> FlattenedChildren {
        let content_model_type = match &complex.content {
            ContentModel::Sequence(_) => ContentModelType::Sequence,
            ContentModel::Choice(_) => ContentModelType::Choice,
            ContentModel::All(_) => ContentModelType::All,
            ContentModel::ComplexExtension { .. } => ContentModelType::Sequence,
            ContentModel::Empty => ContentModelType::Empty,
            ContentModel::SimpleContent { .. } => ContentModelType::Empty,
            ContentModel::Any { .. } => ContentModelType::Sequence,
        };

        let mut flattened = FlattenedChildren::with_content_model(content_model_type);

        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        // Collect ordered elements into a temporary Vec, then convert to Arc<[String]>
        let mut ordered: Vec<String> = Vec::with_capacity(elements.len());
        for elem in &elements {
            flattened
                .constraints
                .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
            // Store element order for sequence validation
            ordered.push(elem.name.clone());
        }
        flattened.ordered_elements = std::sync::Arc::from(ordered);

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
    pub(crate) fn collect_elements_with_inheritance(
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
                if !visited.contains(base_type.as_str()) {
                    visited.insert(base_type.clone());
                    if let Some(TypeDef::Complex(base_complex)) =
                        self.schema.get_type(base_type.as_str())
                    {
                        let base_elements =
                            self.collect_elements_with_inheritance(base_complex, visited);
                        elements.extend(base_elements);
                    }
                }
                elements.extend(ext_elements.iter().cloned());
            }
            _ => {}
        }

        elements
    }

    /// Batch validates min_occurs for all children.
    pub(crate) fn validate_min_occurs_batch(
        &self,
        node: &ElementSkeleton,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_min_occurs {
            return;
        }

        // For Choice content model
        if flattened.content_model_type == ContentModelType::Choice {
            let any_choice_present = flattened
                .constraints
                .keys()
                .any(|child_name| self.get_total_count(node, child_name) > 0);

            if !any_choice_present && !flattened.constraints.is_empty() {
                let choices: Vec<_> = flattened.constraints.keys().cloned().collect();
                let error = self
                    .make_error(
                        ValidationErrorType::MissingRequiredElement,
                        format!(
                            "element '{}' requires one of: {}",
                            node.name,
                            choices.join(", ")
                        ),
                        node,
                    )
                    .with_node_name(node.name.as_ref())
                    .with_expected(format!("one of: {}", choices.join(", ")))
                    .with_found("none".to_string())
                    .with_level(ErrorLevel::Error);

                if self.should_add_error(errors) {
                    errors.push(error);
                }
            }
            return;
        }

        // For Sequence/All content models
        for (child_name, &(min_occurs, _)) in &flattened.constraints {
            if min_occurs > 0 {
                let actual_count = self.get_total_count(node, child_name);

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
                                node.name, child_name, min_occurs, actual_count
                            ),
                            node,
                        )
                        .with_node_name(node.name.as_ref())
                        .with_expected(format!(
                            "at least {} occurrence(s) of '{}'",
                            min_occurs, child_name
                        ))
                        .with_found(format!("{} occurrence(s)", actual_count))
                        .with_level(ErrorLevel::Error);

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
                }
            }
        }
    }

    /// Batch validates max_occurs for all children.
    pub(crate) fn validate_max_occurs_batch(
        &self,
        node: &ElementSkeleton,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_max_occurs {
            return;
        }

        for (child_name, &(_, max_occurs)) in &flattened.constraints {
            if let Some(max) = max_occurs {
                let total_count = self.get_total_count(node, child_name);

                if total_count > max {
                    let error = self
                        .make_error(
                            ValidationErrorType::TooManyOccurrences,
                            format!(
                                "element '{}' (or substitutes) occurs {} times, but maximum is {}",
                                child_name, total_count, max
                            ),
                            node,
                        )
                        .with_node_name(child_name)
                        .with_level(ErrorLevel::Error);

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
                }
            }
        }
    }

    /// Validates that child elements appear in the correct sequence order.
    pub(crate) fn validate_sequence_order(
        &self,
        skeleton: &DocumentSkeleton,
        node: &ElementSkeleton,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        // Only validate sequence content models
        if flattened.content_model_type != ContentModelType::Sequence {
            return;
        }

        // Skip if no ordered elements defined
        if flattened.ordered_elements.is_empty() {
            return;
        }

        // Get actual child element names in order
        let actual_children: Vec<&str> = node
            .children_indices
            .iter()
            .filter_map(|&idx| skeleton.get_node(idx))
            .map(|child| child.name.as_ref())
            .collect();

        // Track position in expected sequence
        let mut expected_index = 0;

        for actual_name in &actual_children {
            // Find the position of this element in the expected sequence (starting from current position)
            let found_pos = flattened.ordered_elements[expected_index..]
                .iter()
                .position(|e| e.as_str() == *actual_name)
                .map(|p| expected_index + p);

            if let Some(pos) = found_pos {
                expected_index = pos;
            } else {
                // Check if this element exists earlier in the sequence (out of order)
                let earlier_pos = flattened.ordered_elements[..expected_index]
                    .iter()
                    .position(|e| e.as_str() == *actual_name);

                if earlier_pos.is_some() {
                    // Element is out of order
                    let expected_after = if expected_index > 0 {
                        flattened.ordered_elements[expected_index - 1].clone()
                    } else {
                        "(beginning)".to_string()
                    };

                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidContent,
                            format!(
                                "element '{}' in '{}' appears out of sequence order (expected after '{}')",
                                actual_name, node.name, expected_after
                            ),
                            node,
                        )
                        .with_node_name(node.name.as_ref())
                        .with_level(ErrorLevel::Error);

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
                    return;
                }
            }
        }
    }

    /// Gets the total count for an element including substitution group members.
    pub(crate) fn get_total_count(&self, node: &ElementSkeleton, child_name: &str) -> u32 {
        let mut count = node.get_child_count(child_name);

        // Try with local name if child_name has a prefix
        if let Some((_prefix, local)) = child_name.split_once(':') {
            count += node.get_child_count(local);
        }

        // Add counts from substitution group members (unless skipped)
        if !self.options.skip_substitution_groups {
            let all_members = self.get_all_substitution_members(child_name);
            for member in all_members.iter() {
                count += node.get_child_count(member);
            }
        }

        count
    }

    /// Gets all substitution group members for a head element.
    #[inline]
    pub(crate) fn get_all_substitution_members(&self, head_name: &str) -> Arc<Vec<String>> {
        // Fast path: direct cache lookup
        if let Some(members) = self.schema.transitive_substitution_groups.get(head_name) {
            return Arc::clone(members);
        }

        // Try with local name if head_name has a prefix
        if let Some((_prefix, local)) = head_name.split_once(':') {
            if let Some(members) = self.schema.transitive_substitution_groups.get(local) {
                return Arc::clone(members);
            }
        }

        Arc::new(Vec::new())
    }

    /// Validates text content against the element's type.
    pub(crate) fn validate_text_content(
        &self,
        node: &ElementSkeleton,
        elem: &ElementDef,
        errors: &mut Vec<StructuredError>,
    ) {
        // Note: we deliberately do NOT skip empty text here. Primitive types
        // (e.g., xs:integer, xs:double) must reject empty content; types whose
        // lexical space allows empty (xs:string and its derivatives) resolve
        // to no PrimitiveKind and pass through with no extra check.

        // Get type definition
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref).cloned()
        } else {
            elem.inline_type.clone()
        };

        match type_def {
            Some(TypeDef::Simple(simple)) => {
                // Validate against simple type facets
                self.validate_simple_type_facets(node, &simple, elem.nillable, errors);
            }
            Some(TypeDef::Complex(complex)) => {
                // Check for SimpleContent with base type
                if let ContentModel::SimpleContent { base_type } = &complex.content {
                    if let Some(TypeDef::Simple(simple)) = self.schema.get_type(base_type) {
                        let simple = simple.clone();
                        self.validate_simple_type_facets(node, &simple, elem.nillable, errors);
                    }
                } else if !complex.mixed && !node.text_content.is_empty() {
                    // Non-mixed complex types shouldn't have text content
                    if let ContentModel::Sequence(_)
                    | ContentModel::Choice(_)
                    | ContentModel::All(_)
                    | ContentModel::ComplexExtension { .. } = &complex.content
                    {
                        let trimmed = node.text_content.trim();
                        if !trimmed.is_empty() {
                            let error = self
                                .make_error(
                                    ValidationErrorType::InvalidContent,
                                    format!(
                                        "element '{}' has element-only content but contains text",
                                        node.name
                                    ),
                                    node,
                                )
                                .with_node_name(node.name.as_ref())
                                .with_level(ErrorLevel::Error);

                            if self.should_add_error(errors) {
                                errors.push(error);
                            }
                        }
                    }
                }
            }
            None => {}
        }
    }

    /// Validates text content against simple type facets and (where the
    /// type resolves to a built-in XSD primitive) its lexical/value space.
    pub(crate) fn validate_simple_type_facets(
        &self,
        node: &ElementSkeleton,
        simple: &SimpleType,
        nillable: bool,
        errors: &mut Vec<StructuredError>,
    ) {
        // User-declared facets. Skip on empty content so we don't pile a
        // facet error on top of any primitive "empty value" error.
        if !node.text_content.is_empty() {
            let constraints = self.create_facet_constraints(simple);
            let validator = FacetValidator::new(&constraints);
            if let Err(facet_error) = validator.validate(&node.text_content) {
                let error = self
                    .make_error(
                        ValidationErrorType::InvalidTextContent,
                        format!("element '{}': {}", node.name, facet_error),
                        node,
                    )
                    .with_node_name(node.name.as_ref())
                    .with_level(ErrorLevel::Error);

                if self.should_add_error(errors) {
                    errors.push(error);
                }
            }
        }

        // Built-in primitive lexical/value-space check. Skip for an empty,
        // nillable element: `xsi:nil="true"` legitimately leaves the content
        // empty and it must not be checked against the primitive type.
        if node.text_content.is_empty() && nillable {
            return;
        }
        if let Some(kind) = PrimitiveKind::resolve(&self.schema, simple)
            && let Err(prim_error) = kind.validate(&node.text_content)
        {
            let error = self
                .make_error(
                    ValidationErrorType::InvalidTextContent,
                    format!("element '{}': {}", node.name, prim_error),
                    node,
                )
                .with_node_name(node.name.as_ref())
                .with_level(ErrorLevel::Error);

            if self.should_add_error(errors) {
                errors.push(error);
            }
        }
    }

    /// Creates FacetConstraints from a SimpleType definition.
    pub(crate) fn create_facet_constraints(&self, simple: &SimpleType) -> FacetConstraints {
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

    /// Creates a structured error with context.
    pub(crate) fn make_error(
        &self,
        error_type: ValidationErrorType,
        message: impl Into<String>,
        node: &ElementSkeleton,
    ) -> StructuredError {
        let mut error = StructuredError::new(message, error_type);
        if let Some(line) = node.line {
            error = error.with_line(line);
        }
        if let Some(column) = node.column {
            error = error.with_column(column);
        }
        error
    }

    /// Checks if we should add more errors.
    pub(crate) fn should_add_error(&self, errors: &[StructuredError]) -> bool {
        self.max_errors == 0 || errors.len() < self.max_errors
    }
}
