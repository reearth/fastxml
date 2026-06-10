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
use crate::schema::types::{CompiledSchema, ElementDef, FlattenedChildren, TypeDef};

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
/// Validation context an element provides to its children: the allowed
/// child-name constraints plus the actual local element declarations.
#[derive(Default)]
struct ParentContext {
    allowed: Option<Arc<FlattenedChildren>>,
    elements: Vec<ElementDef>,
}

/// XML Schema Instance namespace.
const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// Looks up an `xsi:*` attribute on a node. The DOM stores attributes under
/// their local names, so the namespace is verified via the node's attribute
/// namespace info.
fn get_xsi_attribute(node: &XmlNode, local: &str) -> Option<String> {
    let value = node.get_attribute(local)?;
    match node.get_attribute_ns_info(local) {
        Some((_, ns)) => (ns == XSI_NS).then_some(value),
        None => None,
    }
}

/// Document-wide ID / IDREF tracking state.
#[derive(Default)]
pub(crate) struct DocIdState {
    /// `xs:ID` values seen so far (uniqueness checking)
    seen_ids: std::collections::HashSet<String>,
    /// `xs:IDREF` values with their locations, resolved after traversal
    pending_idrefs: Vec<(String, Option<usize>, Option<usize>)>,
}

impl DocIdState {
    /// Records ID values (returning duplicate-ID error messages) and IDREF
    /// values found at the given location.
    pub(crate) fn record(
        &mut self,
        ids: Vec<String>,
        idrefs: Vec<String>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for id in ids {
            if !self.seen_ids.insert(id.clone()) {
                errors.push(format!("duplicate ID value '{}'", id));
            }
        }
        for idref in idrefs {
            self.pending_idrefs.push((idref, line, column));
        }
        errors
    }
}

#[doc(hidden)]
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

    /// Sets the maximum number of errors to collect.
    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Validates the document and returns any errors found.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let mut errors = Vec::new();
        let mut ids = DocIdState::default();

        // Start validation from root
        if let Ok(root) = doc.get_root_element() {
            self.validate_node_recursive(&root, None, &mut ids, &mut errors);
        }

        // Resolve IDREF references against the IDs seen in the document
        for (idref, line, column) in ids.pending_idrefs {
            if !ids.seen_ids.contains(&idref) {
                let mut error = StructuredError::new(
                    format!("IDREF '{}' does not match any ID in the document", idref),
                    ValidationErrorType::IdentityConstraint,
                )
                .with_level(ErrorLevel::Error);
                if let Some(line) = line {
                    error = error.with_line(line);
                }
                if let Some(column) = column {
                    error = error.with_column(column);
                }
                if self.should_add_error(&errors) {
                    errors.push(error);
                }
            }
        }

        Ok(errors)
    }

    /// Recursively validates a node and its children.
    ///
    /// `parent_ctx` carries the parent type's allowed-children constraints
    /// and element declarations, so locally declared elements can be
    /// resolved and validated.
    fn validate_node_recursive(
        &self,
        node: &XmlNode,
        parent_ctx: Option<&ParentContext>,
        ids: &mut DocIdState,
        errors: &mut Vec<StructuredError>,
    ) {
        // Check max errors
        if self.max_errors > 0 && errors.len() >= self.max_errors {
            return;
        }

        match node.get_type() {
            NodeType::Element => {
                let ctx = self.validate_element(node, parent_ctx, ids, errors);

                // Validate children recursively with this element's context
                for child in node.get_child_elements() {
                    self.validate_node_recursive(&child, Some(&ctx), ids, errors);
                }
            }
            NodeType::Document => {
                // Validate children of document node
                for child in node.get_child_elements() {
                    self.validate_node_recursive(&child, None, ids, errors);
                }
            }
            _ => {
                // Skip other node types (text, comments, PIs, etc.)
            }
        }
    }

    /// Validates an element node.
    ///
    /// Returns the validation context (allowed children + local element
    /// declarations) of this element's type for validating its children.
    fn validate_element(
        &self,
        node: &XmlNode,
        parent_ctx: Option<&ParentContext>,
        ids: &mut DocIdState,
        errors: &mut Vec<StructuredError>,
    ) -> ParentContext {
        let name = node.get_name();
        let prefix = node.get_prefix();

        // Look up element definition: global first, then the parent type's
        // local declarations (searched from the end so a derived type's
        // redeclaration shadows the base type's).
        let elem_def = self.lookup_element(&name, prefix.as_deref()).or_else(|| {
            parent_ctx.and_then(|ctx| ctx.elements.iter().rev().find(|e| e.name == *name))
        });
        let schema_has_elements = !self.schema.elements.is_empty();

        // Check if element is allowed by parent's type definition
        let is_allowed_by_parent = parent_ctx
            .and_then(|ctx| ctx.allowed.as_ref())
            .map(|fc| fc.constraints.contains_key(&name))
            .unwrap_or(false);

        if let Some(elem) = elem_def {
            // xsi:type substitution: validate against the named type instead
            // of the declared one (when the substitution is allowed).
            let mut elem_substituted;
            let mut elem = elem;
            if let Some(xsi_type) = get_xsi_attribute(node, "type") {
                let declared = elem.type_ref.as_deref();
                match super::xsi_type::resolve_xsi_type(&self.schema, declared, &xsi_type) {
                    Ok(substituted) => {
                        elem_substituted = elem.clone();
                        elem_substituted.type_ref = Some(substituted);
                        elem_substituted.inline_type = None;
                        elem = &elem_substituted;
                    }
                    Err(message) => {
                        let error = self
                            .make_error(
                                ValidationErrorType::InvalidAttributeValue,
                                format!("element '{}': {}", name, message),
                                node,
                            )
                            .with_node_name(&name)
                            .with_level(ErrorLevel::Error);
                        if self.should_add_error(errors) {
                            errors.push(error);
                        }
                    }
                }
            }

            // An abstract element may not appear in the instance directly.
            if elem.is_abstract {
                let error = self
                    .make_error(
                        ValidationErrorType::InvalidContent,
                        format!("element '{}' is abstract and cannot be used directly", name),
                        node,
                    )
                    .with_node_name(&name)
                    .with_level(ErrorLevel::Error);
                if self.should_add_error(errors) {
                    errors.push(error);
                }
            }

            // xsi:nil handling: only nillable declarations may carry it, and
            // a nilled element must be empty.
            let nilled = get_xsi_attribute(node, "nil").is_some_and(|v| v.trim() == "true");
            if nilled {
                if !elem.nillable {
                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidAttributeValue,
                            format!(
                                "element '{}' is not nillable but has xsi:nil=\"true\"",
                                name
                            ),
                            node,
                        )
                        .with_node_name(&name)
                        .with_level(ErrorLevel::Error);
                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
                } else if !node.get_child_elements().is_empty()
                    || !self.collect_text_content(node).trim().is_empty()
                {
                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidContent,
                            format!("element '{}' has xsi:nil=\"true\" but is not empty", name),
                            node,
                        )
                        .with_node_name(&name)
                        .with_level(ErrorLevel::Error);
                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
                }
            }

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
            self.validate_text_content(node, elem, ids, errors);

            // Validate attributes against the type's attribute declarations
            self.validate_node_attributes(node, elem, ids, errors);

            ParentContext {
                elements: self.element_child_declarations(elem),
                allowed: flattened,
            }
        } else if is_allowed_by_parent {
            // Element is allowed by the parent type but has no resolvable
            // declaration - nothing further to validate.
            ParentContext::default()
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
            ParentContext::default()
        } else {
            ParentContext::default()
        }
    }

    /// Validates an element's attributes against the attribute declarations
    /// of its complex type.
    fn validate_node_attributes(
        &self,
        node: &XmlNode,
        elem: &ElementDef,
        ids: &mut DocIdState,
        errors: &mut Vec<StructuredError>,
    ) {
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            elem.inline_type.as_ref()
        };
        let Some(TypeDef::Complex(complex)) = type_def else {
            return;
        };

        // The DOM stores attribute names without prefixes; exclude xsi:*
        // control attributes so they aren't matched against declared
        // attributes that happen to share a local name (e.g. "type").
        let attrs = node.get_attributes();
        let filtered: Vec<(&str, &str)> = attrs
            .iter()
            .filter(
                |(k, _)| !matches!(node.get_attribute_ns_info(k), Some((_, ns)) if ns == XSI_NS),
            )
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let result = super::attributes::validate_element_attributes(
            &self.schema,
            complex,
            filtered.iter().copied(),
        );
        let mut messages = result.errors;
        messages.extend(ids.record(result.ids, result.idrefs, node.line(), node.column()));
        for message in messages {
            let name = node.get_name();
            let error = self
                .make_error(
                    ValidationErrorType::InvalidAttributeValue,
                    format!("element '{}': {}", name, message),
                    node,
                )
                .with_node_name(&name)
                .with_level(ErrorLevel::Error);
            if self.should_add_error(errors) {
                errors.push(error);
            }
        }
    }

    /// Collects the child element declarations of an element's type so
    /// locally declared children can be resolved during recursion.
    fn element_child_declarations(&self, elem: &ElementDef) -> Vec<ElementDef> {
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            self.schema.get_type(type_ref)
        } else {
            elem.inline_type.as_ref()
        };
        match type_def {
            Some(TypeDef::Complex(complex)) => {
                let mut visited = std::collections::HashSet::new();
                self.collect_elements_with_inheritance(complex, &mut visited)
            }
            _ => Vec::new(),
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
