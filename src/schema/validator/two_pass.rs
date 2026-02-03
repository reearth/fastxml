//! Two-pass streaming schema validator.
//!
//! This module provides a two-pass validation approach:
//! 1. Pass 1: Build a lightweight skeleton of the document structure
//! 2. Pass 2: Perform batch validation using the skeleton
//!
//! This approach can be more efficient for large documents as it allows
//! batch processing of occurrence counts and other constraints.

use std::collections::HashMap;
use std::io::{BufRead, Seek, SeekFrom};
use std::sync::Arc;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{StreamingParser, XmlEvent, XmlEventHandler};
use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    SimpleType, TypeDef,
};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

use super::ValidationMode;
use super::streaming::ValidationOptions;

/// A lightweight skeleton of an element for batch validation.
#[derive(Debug, Clone)]
pub struct ElementSkeleton {
    /// Element name (local name)
    pub name: Arc<str>,
    /// Element prefix (if any)
    pub prefix: Option<Arc<str>>,
    /// Child element occurrence counts - HashMap for efficient batch lookup
    pub child_counts: HashMap<String, u32>,
    /// Indices of child skeleton nodes in the flat storage
    pub children_indices: Vec<usize>,
    /// Line number for error reporting (1-indexed)
    pub line: Option<usize>,
    /// Column number for error reporting (1-indexed, in UTF-8 characters)
    pub column: Option<usize>,
    /// Collected text content
    pub text_content: String,
}

impl ElementSkeleton {
    /// Creates a new element skeleton.
    pub fn new(
        name: Arc<str>,
        prefix: Option<Arc<str>>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self {
            name,
            prefix,
            child_counts: HashMap::new(),
            children_indices: Vec::new(),
            line,
            column,
            text_content: String::new(),
        }
    }

    /// Increments the count for a child element.
    pub fn increment_child(&mut self, child_name: &str) -> u32 {
        let count = self.child_counts.entry(child_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Gets the count for a child element.
    pub fn get_child_count(&self, child_name: &str) -> u32 {
        self.child_counts.get(child_name).copied().unwrap_or(0)
    }
}

/// A document skeleton containing all element skeletons in a flat structure.
#[derive(Debug, Default)]
pub struct DocumentSkeleton {
    /// Flat storage of all element skeletons
    pub nodes: Vec<ElementSkeleton>,
    /// Index of the root element (if any)
    pub root_index: Option<usize>,
}

impl DocumentSkeleton {
    /// Creates a new empty document skeleton.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            root_index: None,
        }
    }

    /// Gets a reference to a node by index.
    pub fn get_node(&self, index: usize) -> Option<&ElementSkeleton> {
        self.nodes.get(index)
    }
}

/// Builder for creating a document skeleton during the first pass.
struct SkeletonBuilder {
    /// The document skeleton being built
    skeleton: DocumentSkeleton,
    /// Stack of node indices representing the current path
    stack: Vec<usize>,
}

impl SkeletonBuilder {
    /// Creates a new skeleton builder.
    fn new() -> Self {
        Self {
            skeleton: DocumentSkeleton::new(),
            stack: Vec::with_capacity(64),
        }
    }

    /// Consumes the builder and returns the built skeleton.
    fn into_skeleton(self) -> DocumentSkeleton {
        self.skeleton
    }
}

impl XmlEventHandler for SkeletonBuilder {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                line,
                column,
                ..
            } => {
                // Create new skeleton node
                let node = ElementSkeleton::new(
                    Arc::clone(name),
                    prefix.as_ref().map(Arc::clone),
                    *line,
                    *column,
                );

                // Add to flat storage
                let node_index = self.skeleton.nodes.len();
                self.skeleton.nodes.push(node);

                // Update parent's child info
                if let Some(&parent_idx) = self.stack.last() {
                    // Increment parent's child count
                    if let Some(parent) = self.skeleton.nodes.get_mut(parent_idx) {
                        parent.increment_child(name.as_ref());
                        parent.children_indices.push(node_index);
                    }
                } else {
                    // This is the root element
                    self.skeleton.root_index = Some(node_index);
                }

                // Push current node onto stack
                self.stack.push(node_index);
            }
            XmlEvent::EndElement { .. } => {
                // Pop the current element from stack
                self.stack.pop();
            }
            XmlEvent::Text(text) | XmlEvent::CData(text) => {
                // Append text to current element
                if let Some(&current_idx) = self.stack.last() {
                    if let Some(current) = self.skeleton.nodes.get_mut(current_idx) {
                        current.text_content.push_str(text);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

/// Two-pass streaming schema validator.
///
/// This validator performs validation in two passes:
/// 1. Build a lightweight skeleton of the document structure
/// 2. Validate the skeleton against the schema
///
/// # Example
///
/// ```ignore
/// use std::fs::File;
/// use std::io::BufReader;
/// use fastxml::schema::validator::TwoPassSchemaValidator;
///
/// let file = File::open("document.xml")?;
/// let reader = BufReader::new(file);
///
/// let errors = TwoPassSchemaValidator::new(schema)
///     .with_max_errors(100)
///     .validate(reader)?;
/// ```
pub struct TwoPassSchemaValidator {
    schema: Arc<CompiledSchema>,
    options: ValidationOptions,
    mode: ValidationMode,
    max_errors: usize,
}

impl TwoPassSchemaValidator {
    /// Creates a new two-pass validator.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            options: ValidationOptions::default(),
            mode: ValidationMode::Strict,
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

    /// Performs two-pass validation and returns any errors found.
    pub fn validate<R: BufRead + Seek>(self, mut reader: R) -> Result<Vec<StructuredError>> {
        // Pass 1: Build skeleton
        let skeleton = Self::build_skeleton(&mut reader)?;

        // Seek back to start for potential future use or debugging
        let _ = reader.seek(SeekFrom::Start(0));

        // Pass 2: Validate with skeleton
        self.validate_with_skeleton(&skeleton)
    }

    /// Pass 1: Builds the document skeleton.
    fn build_skeleton<R: BufRead>(reader: &mut R) -> Result<DocumentSkeleton> {
        let mut parser = StreamingParser::new(reader);
        let builder = SkeletonBuilder::new();
        parser.add_handler(Box::new(builder));
        parser.parse()?;

        // Extract the builder from the parser
        let handlers = parser.into_handlers();
        for handler in handlers {
            if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
                return Ok(builder.into_skeleton());
            }
        }

        // Fallback: return empty skeleton
        Ok(DocumentSkeleton::new())
    }

    /// Pass 2: Validates the document using the pre-built skeleton.
    fn validate_with_skeleton(&self, skeleton: &DocumentSkeleton) -> Result<Vec<StructuredError>> {
        let mut errors = Vec::new();

        // Start validation from root
        if let Some(root_index) = skeleton.root_index {
            self.validate_node_recursive(skeleton, root_index, None, &mut errors);
        }

        Ok(errors)
    }

    /// Recursively validates a node and its children.
    ///
    /// `parent_allowed_children` contains the set of child element names allowed by the parent's type.
    fn validate_node_recursive(
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
    fn lookup_element(&self, name: &Arc<str>, prefix: Option<&Arc<str>>) -> Option<&ElementDef> {
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
    fn validate_max_occurs_batch(
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
    fn validate_sequence_order(
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
    fn get_total_count(&self, node: &ElementSkeleton, child_name: &str) -> u32 {
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
        node: &ElementSkeleton,
        elem: &ElementDef,
        errors: &mut Vec<StructuredError>,
    ) {
        if node.text_content.is_empty() {
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
                self.validate_simple_type_facets(node, &simple, errors);
            }
            Some(TypeDef::Complex(complex)) => {
                // Check for SimpleContent with base type
                if let ContentModel::SimpleContent { base_type } = &complex.content {
                    if let Some(TypeDef::Simple(simple)) = self.schema.get_type(base_type) {
                        self.validate_simple_type_facets(node, simple, errors);
                    }
                } else if !complex.mixed {
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

    /// Validates text content against simple type facets.
    fn validate_simple_type_facets(
        &self,
        node: &ElementSkeleton,
        simple: &SimpleType,
        errors: &mut Vec<StructuredError>,
    ) {
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
    fn should_add_error(&self, errors: &[StructuredError]) -> bool {
        self.max_errors == 0 || errors.len() < self.max_errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn create_test_reader(xml: &str) -> Cursor<Vec<u8>> {
        Cursor::new(xml.as_bytes().to_vec())
    }

    #[test]
    fn test_element_skeleton_new() {
        let skeleton = ElementSkeleton::new("test".into(), None, Some(1), Some(5));
        assert_eq!(skeleton.name.as_ref(), "test");
        assert!(skeleton.prefix.is_none());
        assert_eq!(skeleton.line, Some(1));
        assert_eq!(skeleton.column, Some(5));
        assert!(skeleton.child_counts.is_empty());
        assert!(skeleton.text_content.is_empty());
    }

    #[test]
    fn test_element_skeleton_child_counts() {
        let mut skeleton = ElementSkeleton::new("parent".into(), None, None, None);

        assert_eq!(skeleton.get_child_count("child"), 0);

        assert_eq!(skeleton.increment_child("child"), 1);
        assert_eq!(skeleton.get_child_count("child"), 1);

        assert_eq!(skeleton.increment_child("child"), 2);
        assert_eq!(skeleton.get_child_count("child"), 2);

        assert_eq!(skeleton.increment_child("other"), 1);
        assert_eq!(skeleton.get_child_count("other"), 1);
    }

    #[test]
    fn test_document_skeleton_new() {
        let skeleton = DocumentSkeleton::new();
        assert!(skeleton.nodes.is_empty());
        assert!(skeleton.root_index.is_none());
    }

    #[test]
    fn test_skeleton_builder_simple() {
        let xml = r#"<root><child>text</child></root>"#;
        let reader = create_test_reader(xml);

        let mut parser = StreamingParser::new(reader);
        let builder = SkeletonBuilder::new();
        parser.add_handler(Box::new(builder));
        parser.parse().unwrap();

        let handlers = parser.into_handlers();
        for handler in handlers {
            if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
                let skeleton = builder.into_skeleton();
                assert_eq!(skeleton.nodes.len(), 2);
                assert_eq!(skeleton.root_index, Some(0));

                // Root node
                let root = skeleton.get_node(0).unwrap();
                assert_eq!(root.name.as_ref(), "root");
                assert_eq!(root.get_child_count("child"), 1);
                assert_eq!(root.children_indices.len(), 1);

                // Child node
                let child = skeleton.get_node(1).unwrap();
                assert_eq!(child.name.as_ref(), "child");
                assert_eq!(child.text_content, "text");

                return;
            }
        }
        panic!("SkeletonBuilder not found");
    }

    #[test]
    fn test_skeleton_builder_nested() {
        let xml = r#"<root><a><b><c/></b></a></root>"#;
        let reader = create_test_reader(xml);

        let mut parser = StreamingParser::new(reader);
        let builder = SkeletonBuilder::new();
        parser.add_handler(Box::new(builder));
        parser.parse().unwrap();

        let handlers = parser.into_handlers();
        for handler in handlers {
            if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
                let skeleton = builder.into_skeleton();
                assert_eq!(skeleton.nodes.len(), 4);

                // Check parent-child relationships
                let root = skeleton.get_node(0).unwrap();
                assert_eq!(root.name.as_ref(), "root");
                assert_eq!(root.children_indices, vec![1]);

                let a = skeleton.get_node(1).unwrap();
                assert_eq!(a.name.as_ref(), "a");
                assert_eq!(a.children_indices, vec![2]);

                let b = skeleton.get_node(2).unwrap();
                assert_eq!(b.name.as_ref(), "b");
                assert_eq!(b.children_indices, vec![3]);

                let c = skeleton.get_node(3).unwrap();
                assert_eq!(c.name.as_ref(), "c");
                assert!(c.children_indices.is_empty());

                return;
            }
        }
        panic!("SkeletonBuilder not found");
    }

    #[test]
    fn test_two_pass_validator_empty_schema() {
        let xml = r#"<root><child/></root>"#;
        let reader = create_test_reader(xml);

        let schema = CompiledSchema::new();
        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();
        // Empty schema should not produce errors
        assert!(errors.is_empty());
    }

    #[test]
    fn test_two_pass_validator_unknown_element_strict() {
        use crate::schema::types::ElementDef;

        let xml = r#"<unknown/>"#;
        let reader = create_test_reader(xml);

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("unknown"));
    }

    #[test]
    fn test_two_pass_validator_unknown_element_lenient() {
        use crate::schema::types::ElementDef;

        let xml = r#"<unknown/>"#;
        let reader = create_test_reader(xml);

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("known".to_string(), ElementDef::new("known"));

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .with_mode(ValidationMode::Lenient)
            .validate(reader)
            .unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_two_pass_validator_min_occurs() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let xml = r#"<parent></parent>"#;
        let reader = create_test_reader(xml);

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

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("required_child"));
    }

    #[test]
    fn test_two_pass_validator_max_occurs() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let xml = r#"<parent><child/><child/><child/></parent>"#;
        let reader = create_test_reader(xml);

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

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("maximum"));
    }

    #[test]
    fn test_two_pass_validator_choice_content_model() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let xml = r#"<boundedBy><Envelope/></boundedBy>"#;
        let reader = create_test_reader(xml);

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

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();

        // Choice should accept one of the options
        assert!(errors.is_empty());
    }

    #[test]
    fn test_two_pass_validator_substitution_group() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let xml = r#"<parent><ReliefFeature/></parent>"#;
        let reader = create_test_reader(xml);

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

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();

        // ReliefFeature should count toward _CityObject requirement
        assert!(
            errors.is_empty(),
            "Substitution group member should satisfy min_occurs, errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_two_pass_validator_with_max_errors() {
        use crate::schema::types::ElementDef;

        let xml = r#"<root><a/><b/><c/><d/><e/></root>"#;
        let reader = create_test_reader(xml);

        let mut schema = CompiledSchema::new();
        schema
            .elements
            .insert("root".to_string(), ElementDef::new("root"));
        // Only root is known, all children are unknown

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .with_max_errors(2)
            .validate(reader)
            .unwrap();
        // Should stop at 2 errors
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_two_pass_validator_type_inheritance() {
        use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

        let xml = r#"<root><baseElement>content</baseElement></root>"#;
        let reader = create_test_reader(xml);

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

        let errors = TwoPassSchemaValidator::new(Arc::new(schema))
            .validate(reader)
            .unwrap();

        // Should recognize inherited element
        assert!(
            errors.is_empty(),
            "Should accept inherited elements, errors: {:?}",
            errors
        );
    }
}
