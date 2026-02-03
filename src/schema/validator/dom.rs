//! DOM-based schema validator.
//!
//! This module provides direct DOM tree validation without re-generating XML events.
//! This approach is faster than the streaming validator for pre-parsed documents
//! as it avoids the overhead of event reconstruction.

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::node::{NodeType, XmlNode};
use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    SimpleType, TypeDef,
};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

use super::ValidationMode;
use super::streaming::ValidationOptions;

/// DOM-based schema validator.
///
/// Validates XML documents by directly traversing the DOM tree,
/// avoiding the overhead of event reconstruction.
///
/// # Example
///
/// ```ignore
/// use fastxml::{parse, schema::validator::DomSchemaValidator};
///
/// let doc = parse(xml_bytes)?;
/// let errors = DomSchemaValidator::new(schema)
///     .with_max_errors(100)
///     .validate(&doc)?;
/// ```
pub struct DomSchemaValidator {
    schema: Arc<CompiledSchema>,
    mode: ValidationMode,
    options: ValidationOptions,
    max_errors: usize,
}

impl DomSchemaValidator {
    /// Creates a new DOM validator.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            mode: ValidationMode::Strict,
            options: ValidationOptions::default(),
            max_errors: 0,
        }
    }

    /// Sets the validation mode.
    pub fn with_mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the validation options.
    pub fn with_options(mut self, options: ValidationOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets the maximum number of errors to collect.
    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Validates the document and returns any errors found.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let mut errors = Vec::new();

        // Start validation from root
        if let Ok(root) = doc.get_root_element() {
            self.validate_node_recursive(&root, None, &mut errors);
        }

        Ok(errors)
    }

    /// Recursively validates a node and its children.
    ///
    /// `parent_allowed_children` contains the set of child element names allowed by the parent's type.
    fn validate_node_recursive(
        &self,
        node: &XmlNode,
        parent_allowed_children: Option<&FlattenedChildren>,
        errors: &mut Vec<StructuredError>,
    ) {
        // Check max errors
        if self.max_errors > 0 && errors.len() >= self.max_errors {
            return;
        }

        match node.get_type() {
            NodeType::Element => {
                let allowed_children = self.validate_element(node, parent_allowed_children, errors);

                // Validate children recursively with this element's allowed children
                for child in node.get_child_elements() {
                    self.validate_node_recursive(&child, allowed_children.as_deref(), errors);
                }
            }
            NodeType::Document => {
                // Validate children of document node
                for child in node.get_child_elements() {
                    self.validate_node_recursive(&child, None, errors);
                }
            }
            _ => {
                // Skip other node types (text, comments, PIs, etc.)
            }
        }
    }

    /// Validates an element node.
    ///
    /// Returns the flattened children constraints for this element's type, so child elements
    /// can be validated against the parent's type definition.
    fn validate_element(
        &self,
        node: &XmlNode,
        parent_allowed_children: Option<&FlattenedChildren>,
        errors: &mut Vec<StructuredError>,
    ) -> Option<Arc<FlattenedChildren>> {
        let name = node.get_name();
        let prefix = node.get_prefix();

        // Look up element definition (global or from parent's type)
        let elem_def = self.lookup_element(&name, prefix.as_deref());
        let schema_has_elements = !self.schema.elements.is_empty();

        // Check if element is allowed by parent's type definition
        let is_allowed_by_parent = parent_allowed_children
            .map(|fc| fc.constraints.contains_key(&name))
            .unwrap_or(false);

        if let Some(elem) = elem_def {
            // Count child elements
            let child_counts = self.count_child_elements(node);

            // Get flattened children for validation
            let flattened = self.get_flattened_children_for_element(elem);
            if let Some(ref fc) = flattened {
                // Validate min_occurs for all children
                self.validate_min_occurs_batch(node, &child_counts, fc, errors);

                // Validate max_occurs for all children
                self.validate_max_occurs_batch(node, &child_counts, fc, errors);

                // Validate sequence order for sequence content models
                self.validate_sequence_order(node, fc, errors);
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
            let qname = match &prefix {
                Some(p) => format!("{}:{}", p, name),
                None => name.to_string(),
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
        }
    }

    /// Counts child elements directly from DOM.
    fn count_child_elements(&self, node: &XmlNode) -> HashMap<String, u32> {
        let mut counts = HashMap::new();
        for child in node.get_child_elements() {
            let name = child.get_name();
            *counts.entry(name).or_insert(0) += 1;
        }
        counts
    }

    /// Collects text content from child nodes.
    fn collect_text_content(&self, node: &XmlNode) -> String {
        let mut text = String::new();
        for child in node.get_child_nodes() {
            match child.get_type() {
                NodeType::Text | NodeType::CData => {
                    if let Some(content) = child.get_content() {
                        text.push_str(&content);
                    }
                }
                _ => {}
            }
        }
        text
    }

    /// Looks up an element definition in the schema.
    fn lookup_element(&self, name: &str, prefix: Option<&str>) -> Option<&ElementDef> {
        // Try local name first
        if let Some(elem) = self.schema.get_element(name) {
            return Some(elem);
        }

        // Try with prefix
        if let Some(p) = prefix {
            if !p.is_empty() {
                let qname = format!("{}:{}", p, name);
                if let Some(elem) = self.schema.get_element(&qname) {
                    return Some(elem);
                }
            }
        }

        None
    }

    /// Gets flattened children for an element from the schema cache.
    fn get_flattened_children_for_element(
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
    fn compute_flattened_children(&self, complex: &ComplexType) -> FlattenedChildren {
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

        for elem in &elements {
            flattened
                .constraints
                .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
            // Store element order for sequence validation
            flattened.ordered_elements.push(elem.name.clone());
        }

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
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
    fn validate_min_occurs_batch(
        &self,
        node: &XmlNode,
        child_counts: &HashMap<String, u32>,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_min_occurs {
            return;
        }

        let node_name = node.get_name();

        // For Choice content model
        if flattened.content_model_type == ContentModelType::Choice {
            let any_choice_present = flattened
                .constraints
                .keys()
                .any(|child_name| self.get_total_count(child_counts, child_name) > 0);

            if !any_choice_present && !flattened.constraints.is_empty() {
                let choices: Vec<_> = flattened.constraints.keys().cloned().collect();
                let error = self
                    .make_error(
                        ValidationErrorType::MissingRequiredElement,
                        format!(
                            "element '{}' requires one of: {}",
                            node_name,
                            choices.join(", ")
                        ),
                        node,
                    )
                    .with_node_name(&node_name)
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
                let actual_count = self.get_total_count(child_counts, child_name);

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
                                node_name, child_name, min_occurs, actual_count
                            ),
                            node,
                        )
                        .with_node_name(&node_name)
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
    fn validate_max_occurs_batch(
        &self,
        node: &XmlNode,
        child_counts: &HashMap<String, u32>,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_max_occurs {
            return;
        }

        for (child_name, &(_, max_occurs)) in &flattened.constraints {
            if let Some(max) = max_occurs {
                let total_count = self.get_total_count(child_counts, child_name);

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
    fn validate_sequence_order(
        &self,
        node: &XmlNode,
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
        let actual_children: Vec<String> = node
            .get_child_elements()
            .iter()
            .map(|c| c.get_name())
            .collect();

        // Track position in expected sequence
        let mut expected_index = 0;

        for actual_name in &actual_children {
            // Find the position of this element in the expected sequence (starting from current position)
            let found_pos = flattened.ordered_elements[expected_index..]
                .iter()
                .position(|e| e == actual_name)
                .map(|p| expected_index + p);

            if let Some(pos) = found_pos {
                expected_index = pos;
            } else {
                // Check if this element exists earlier in the sequence (out of order)
                let earlier_pos = flattened.ordered_elements[..expected_index]
                    .iter()
                    .position(|e| e == actual_name);

                if earlier_pos.is_some() {
                    // Element is out of order
                    let node_name = node.get_name();
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
                                actual_name, node_name, expected_after
                            ),
                            node,
                        )
                        .with_node_name(&node_name)
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
    fn get_total_count(&self, child_counts: &HashMap<String, u32>, child_name: &str) -> u32 {
        let mut count = child_counts.get(child_name).copied().unwrap_or(0);

        // Try with local name if child_name has a prefix
        if let Some((_prefix, local)) = child_name.split_once(':') {
            count += child_counts.get(local).copied().unwrap_or(0);
        }

        // Add counts from substitution group members (unless skipped)
        if !self.options.skip_substitution_groups {
            let all_members = self.get_all_substitution_members(child_name);
            for member in all_members.iter() {
                count += child_counts.get(member).copied().unwrap_or(0);
            }
        }

        count
    }

    /// Gets all substitution group members for a head element.
    #[inline]
    fn get_all_substitution_members(&self, head_name: &str) -> Arc<Vec<String>> {
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
    fn validate_text_content(
        &self,
        node: &XmlNode,
        elem: &ElementDef,
        errors: &mut Vec<StructuredError>,
    ) {
        let text_content = self.collect_text_content(node);
        if text_content.is_empty() {
            return;
        }

        // Get type definition
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref).cloned()
        } else {
            elem.inline_type.clone()
        };

        match type_def {
            Some(TypeDef::Simple(simple)) => {
                // Validate against simple type facets
                self.validate_simple_type_facets(node, &simple, &text_content, errors);
            }
            Some(TypeDef::Complex(complex)) => {
                // Check for SimpleContent with base type
                if let ContentModel::SimpleContent { base_type } = &complex.content {
                    if let Some(TypeDef::Simple(simple)) = self.schema.get_type(base_type) {
                        self.validate_simple_type_facets(node, simple, &text_content, errors);
                    }
                } else if !complex.mixed {
                    // Non-mixed complex types shouldn't have text content
                    if let ContentModel::Sequence(_)
                    | ContentModel::Choice(_)
                    | ContentModel::All(_)
                    | ContentModel::ComplexExtension { .. } = &complex.content
                    {
                        let trimmed = text_content.trim();
                        if !trimmed.is_empty() {
                            let node_name = node.get_name();
                            let error = self
                                .make_error(
                                    ValidationErrorType::InvalidContent,
                                    format!(
                                        "element '{}' has element-only content but contains text",
                                        node_name
                                    ),
                                    node,
                                )
                                .with_node_name(&node_name)
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

    /// Validates text content against simple type facets.
    fn validate_simple_type_facets(
        &self,
        node: &XmlNode,
        simple: &SimpleType,
        text_content: &str,
        errors: &mut Vec<StructuredError>,
    ) {
        let constraints = self.create_facet_constraints(simple);
        let validator = FacetValidator::new(&constraints);

        if let Err(facet_error) = validator.validate(text_content) {
            let node_name = node.get_name();
            let error = self
                .make_error(
                    ValidationErrorType::InvalidTextContent,
                    format!("element '{}': {}", node_name, facet_error),
                    node,
                )
                .with_node_name(&node_name)
                .with_level(ErrorLevel::Error);

            if self.should_add_error(errors) {
                errors.push(error);
            }
        }
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

    /// Creates a structured error with context.
    fn make_error(
        &self,
        error_type: ValidationErrorType,
        message: impl Into<String>,
        node: &XmlNode,
    ) -> StructuredError {
        let mut error = StructuredError::new(message, error_type);
        if let Some(line) = node.line() {
            error = error.with_line(line);
        }
        error
    }

    /// Checks if we should add more errors.
    fn should_add_error(&self, errors: &[StructuredError]) -> bool {
        self.max_errors == 0 || errors.len() < self.max_errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    fn create_test_doc(xml: &str) -> XmlDocument {
        parse(xml.as_bytes()).unwrap()
    }

    #[test]
    fn test_dom_validator_empty_schema() {
        let doc = create_test_doc("<root><child/></root>");
        let schema = CompiledSchema::new();
        let validator = DomSchemaValidator::new(Arc::new(schema));

        let errors = validator.validate(&doc).unwrap();
        // Empty schema should not produce errors
        assert!(errors.is_empty());
    }

    #[test]
    fn test_dom_validator_unknown_element_strict() {
        let doc = create_test_doc("<unknown/>");

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("unknown"));
    }

    #[test]
    fn test_dom_validator_unknown_element_lenient() {
        let doc = create_test_doc("<unknown/>");

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let validator =
            DomSchemaValidator::new(Arc::new(schema)).with_mode(ValidationMode::Lenient);
        let errors = validator.validate(&doc).unwrap();

        assert!(errors.is_empty());
    }

    #[test]
    fn test_dom_validator_min_occurs() {
        let doc = create_test_doc("<parent></parent>");

        let mut schema = CompiledSchema::new();

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

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("required_child"));
    }

    #[test]
    fn test_dom_validator_max_occurs() {
        let doc = create_test_doc("<parent><child/><child/><child/></parent>");

        let mut schema = CompiledSchema::new();

        let complex_type = ComplexType {
            name: "ParentType".to_string(),
            base_type: None,
            content: ContentModel::Sequence(vec![ElementDef::new("child").with_occurs(0, Some(2))]),
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

        // Also define child as global element
        schema
            .elements
            .insert("child".to_string(), ElementDef::new("child"));

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("maximum"));
    }

    #[test]
    fn test_dom_validator_choice_content_model() {
        let doc = create_test_doc("<boundedBy><Envelope/></boundedBy>");

        let mut schema = CompiledSchema::new();

        let mut choice_type = ComplexType::new("BoundingShapeType");
        choice_type.content = ContentModel::Choice(vec![
            ElementDef::new("Envelope").with_type("xs:string"),
            ElementDef::new("Null").with_type("xs:string"),
        ]);

        schema.types.insert(
            "BoundingShapeType".to_string(),
            TypeDef::Complex(choice_type),
        );

        schema.elements.insert(
            "boundedBy".to_string(),
            ElementDef::new("boundedBy").with_type("BoundingShapeType"),
        );
        schema
            .elements
            .insert("Envelope".to_string(), ElementDef::new("Envelope"));
        schema
            .elements
            .insert("Null".to_string(), ElementDef::new("Null"));

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        // Choice should accept one of the options
        assert!(errors.is_empty());
    }

    #[test]
    fn test_dom_validator_substitution_group() {
        let doc = create_test_doc("<parent><ReliefFeature/></parent>");

        let mut schema = CompiledSchema::new();

        // Parent type expects _CityObject
        let mut parent_type = ComplexType::new("ParentType");
        parent_type.content = ContentModel::Sequence(vec![
            ElementDef::new("_CityObject").with_type("AbstractCityObjectType"),
        ]);
        schema
            .types
            .insert("ParentType".to_string(), TypeDef::Complex(parent_type));

        let abstract_type = ComplexType::new("AbstractCityObjectType");
        schema.types.insert(
            "AbstractCityObjectType".to_string(),
            TypeDef::Complex(abstract_type),
        );

        // Define elements
        let mut head_elem = ElementDef::new("_CityObject");
        head_elem.is_abstract = true;
        head_elem.type_ref = Some("AbstractCityObjectType".to_string());
        schema.elements.insert("_CityObject".to_string(), head_elem);

        let mut substitute_elem = ElementDef::new("ReliefFeature");
        substitute_elem.substitution_group = Some("_CityObject".to_string());
        schema
            .elements
            .insert("ReliefFeature".to_string(), substitute_elem);

        schema.elements.insert(
            "parent".to_string(),
            ElementDef::new("parent").with_type("ParentType"),
        );

        // Build substitution group caches
        schema
            .substitution_groups
            .insert("_CityObject".to_string(), vec!["ReliefFeature".to_string()]);
        schema
            .substitution_group_heads
            .insert("ReliefFeature".to_string(), "_CityObject".to_string());
        schema.transitive_substitution_groups.insert(
            "_CityObject".to_string(),
            Arc::new(vec!["ReliefFeature".to_string()]),
        );

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        // ReliefFeature should count toward _CityObject requirement
        assert!(
            errors.is_empty(),
            "Substitution group member should satisfy min_occurs, errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_dom_validator_with_max_errors() {
        let doc = create_test_doc("<root><a/><b/><c/><d/><e/></root>");

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("root".to_string(), ElementDef::new("root"));
        // Only root is known, all children are unknown

        let validator = DomSchemaValidator::new(Arc::new(schema)).with_max_errors(2);
        let errors = validator.validate(&doc).unwrap();

        // Should stop at 2 errors
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_dom_validator_type_inheritance() {
        let doc = create_test_doc("<root><baseElement>content</baseElement></root>");

        let mut schema = CompiledSchema::new();

        // BaseType with "baseElement"
        let mut base_type = ComplexType::new("BaseType");
        base_type.content = ContentModel::Sequence(vec![
            ElementDef::new("baseElement")
                .with_type("xs:string")
                .optional(),
        ]);
        schema
            .types
            .insert("BaseType".to_string(), TypeDef::Complex(base_type));

        // ExtendedType extends BaseType
        let mut extended_type = ComplexType::new("ExtendedType");
        extended_type.content = ContentModel::ComplexExtension {
            base_type: "BaseType".to_string(),
            elements: vec![],
        };
        schema
            .types
            .insert("ExtendedType".to_string(), TypeDef::Complex(extended_type));

        schema.elements.insert(
            "root".to_string(),
            ElementDef::new("root").with_type("ExtendedType"),
        );
        schema.elements.insert(
            "baseElement".to_string(),
            ElementDef::new("baseElement").with_type("xs:string"),
        );

        let validator = DomSchemaValidator::new(Arc::new(schema));
        let errors = validator.validate(&doc).unwrap();

        // Should recognize inherited element
        assert!(
            errors.is_empty(),
            "Should accept inherited elements, errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_dom_validator_count_child_elements() {
        let doc = create_test_doc("<parent><a/><b/><a/><c/><a/></parent>");

        let schema = CompiledSchema::new();
        let validator = DomSchemaValidator::new(Arc::new(schema));

        let root = doc.get_root_element().unwrap();
        let counts = validator.count_child_elements(&root);

        assert_eq!(counts.get("a"), Some(&3));
        assert_eq!(counts.get("b"), Some(&1));
        assert_eq!(counts.get("c"), Some(&1));
        assert_eq!(counts.get("d"), None);
    }

    #[test]
    fn test_dom_validator_collect_text_content() {
        let doc = create_test_doc("<root>Hello <child/>World</root>");

        let schema = CompiledSchema::new();
        let validator = DomSchemaValidator::new(Arc::new(schema));

        let root = doc.get_root_element().unwrap();
        let text = validator.collect_text_content(&root);

        assert_eq!(text, "Hello World");
    }
}
