//! DOM-based schema validator.
//!
//! This module provides direct DOM tree validation without re-generating XML events.
//! This approach is faster than the streaming validator for pre-parsed documents
//! as it avoids the overhead of event reconstruction.

mod content;
mod lookup;
mod occurrence;

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::node::{NodeType, XmlNode};
use crate::schema::types::{CompiledSchema, FlattenedChildren};

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
    pub(crate) schema: Arc<CompiledSchema>,
    pub(crate) mode: ValidationMode,
    pub(crate) options: ValidationOptions,
    pub(crate) max_errors: usize,
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
    pub(crate) fn count_child_elements(&self, node: &XmlNode) -> HashMap<String, u32> {
        let mut counts = HashMap::new();
        for child in node.get_child_elements() {
            let name = child.get_name();
            *counts.entry(name).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests;
