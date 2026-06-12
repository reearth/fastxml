//! DOM-based schema validator.
//!
//! This module provides direct DOM tree validation without re-generating XML events.
//! This approach is faster than the streaming validator for pre-parsed documents
//! as it avoids the overhead of event reconstruction.

mod content;
mod identity;
mod lookup;
mod occurrence;

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::node::{NodeType, XmlNode};
use crate::schema::types::{
    CompiledSchema, ElementDef, FlattenedChildren, ProcessContents, TypeDef, WildcardConstraint,
    WildcardNamespace,
};

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
    /// Wildcard admitting otherwise-undeclared children (from the type's
    /// content model, or propagated from an enclosing lax/skip wildcard).
    wildcard: Option<WildcardConstraint>,
}

/// A wildcard that admits anything, used to propagate lax/skip processing
/// into the subtree of a wildcard-matched element.
fn propagated_wildcard(mode: ProcessContents) -> WildcardConstraint {
    WildcardConstraint {
        namespace: WildcardNamespace::Any,
        process_contents: mode,
        min_occurs: 0,
        max_occurs: None,
        target_namespace: None,
    }
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
    /// Identity constraint tasks (scoping element x constraint), evaluated
    /// after traversal
    constraint_tasks: Vec<identity::ConstraintTask>,
    /// Primitive value-space kind per simple-typed element node, recorded
    /// during traversal so identity-constraint tuples can be compared in
    /// the right value space.
    pub(crate) node_kinds: std::collections::HashMap<
        crate::node::NodeId,
        crate::schema::xsd::primitive::PrimitiveKind,
    >,
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
    /// Memoized facet constraints per named simple type
    pub(crate) facet_cache: std::cell::RefCell<crate::schema::xsd::facets::FacetCache>,
    /// Interned error strings (messages repeat heavily on invalid files)
    pub(crate) error_strings: std::cell::RefCell<rustc_hash::FxHashSet<std::sync::Arc<str>>>,
    /// (message, node_name) -> index into the error vec, used when
    /// `aggregate_errors` is on.
    pub(crate) aggregate_index: std::cell::RefCell<
        rustc_hash::FxHashMap<(std::sync::Arc<str>, Option<std::sync::Arc<str>>), usize>,
    >,
}

impl DomSchemaValidator {
    /// Creates a new DOM validator.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            mode: ValidationMode::Strict,
            options: ValidationOptions::default(),
            max_errors: 0,
            facet_cache: Default::default(),
            error_strings: Default::default(),
            aggregate_index: Default::default(),
        }
    }

    /// Sets the validation mode.
    pub fn with_mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the maximum number of errors to collect.
    /// Collapses identical errors into one entry with a count (builder).
    pub fn with_aggregate_errors(mut self) -> Self {
        self.options.aggregate_errors = true;
        self
    }

    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Interns an error's strings through the validator-lifetime pool.
    fn intern_error(&self, error: StructuredError) -> StructuredError {
        error.interned(&mut self.error_strings.borrow_mut())
    }

    /// Appends an error, respecting `max_errors` and, when enabled,
    /// collapsing identical errors into one entry with a count.
    pub(crate) fn push_error(&self, errors: &mut Vec<StructuredError>, error: StructuredError) {
        let error = self.intern_error(error);
        if self.options.aggregate_errors {
            let key = (
                std::sync::Arc::clone(&error.message),
                error.node_name.clone(),
            );
            let mut index = self.aggregate_index.borrow_mut();
            if let Some(&idx) = index.get(&key) {
                errors[idx].count += 1;
                return;
            }
            if self.should_add_error(errors) {
                index.insert(key, errors.len());
                errors.push(error);
            }
            return;
        }
        if self.should_add_error(errors) {
            errors.push(error);
        }
    }

    /// Validates the document and returns any errors found.
    pub fn validate(&self, doc: &XmlDocument) -> Result<Vec<StructuredError>> {
        let mut errors = Vec::new();
        let mut ids = DocIdState::default();

        // Start validation from root
        if let Ok(root) = doc.get_root_element() {
            self.validate_node_recursive(&root, None, &mut ids, &mut errors);
        }

        // Evaluate identity constraints (unique / key / keyref)
        for message in identity::validate_identity_constraints(
            &self.schema,
            doc,
            &ids.constraint_tasks,
            &ids.node_kinds,
        ) {
            let error = StructuredError::new(message, ValidationErrorType::IdentityConstraint)
                .with_level(ErrorLevel::Error);
            self.push_error(&mut errors, error);
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
                self.push_error(&mut errors, error);
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
        let node_ns = node.get_namespace_uri();

        // Look up the element declaration. A local declaration in the
        // parent's type that carries type information wins over a global
        // element with the same name — the same element name can be
        // declared with different types in different content models (e.g.
        // gml:exterior in Solid vs Polygon, or gen:value as xs:string vs
        // the measure 'value' with required @uom). Bare references (no
        // type info) fall back to the global declaration they point to.
        // Local declarations are searched from the end so a derived type's
        // redeclaration shadows the base type's.
        let inline_def =
            parent_ctx.and_then(|ctx| ctx.elements.iter().rev().find(|e| e.name == *name));
        let elem_def = match inline_def {
            Some(e) if e.type_ref.is_some() || e.inline_type.is_some() => Some(e),
            _ => self
                .lookup_element(&name, prefix.as_deref(), node_ns.as_deref())
                .or(inline_def),
        };
        let schema_has_elements = !self.schema.elements.is_empty();

        // Check if element is allowed by parent's type definition
        let is_allowed_by_parent = parent_ctx
            .and_then(|ctx| ctx.allowed.as_ref())
            .map(|fc| fc.constraints.contains_key(&name))
            .unwrap_or(false);

        // A skip wildcard admits this element without any validation, even
        // when a matching declaration exists.
        if !is_allowed_by_parent {
            if let Some(w) = parent_ctx.and_then(|ctx| ctx.wildcard.as_ref()) {
                if w.process_contents == ProcessContents::Skip
                    && w.matches(node.get_namespace_uri().as_deref())
                {
                    return ParentContext {
                        wildcard: Some(propagated_wildcard(ProcessContents::Skip)),
                        ..ParentContext::default()
                    };
                }
            }
        }

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
                        self.push_error(errors, error);
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
                self.push_error(errors, error);
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
                    self.push_error(errors, error);
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
                    self.push_error(errors, error);
                }
            }

            // Queue identity constraints declared on this element for
            // post-traversal evaluation.
            for constraint in &elem.constraints {
                ids.constraint_tasks.push(identity::ConstraintTask {
                    node: node.clone(),
                    constraint: constraint.clone(),
                });
            }

            // Count child elements
            let child_counts = self.count_child_elements(node);

            // Get flattened children for validation
            let flattened = self.get_flattened_children_for_element(elem);
            if let Some(ref fc) = flattened {
                // The content-model automaton replaces the count-based
                // checks when the type has one.
                if !self.validate_with_automaton(node, &child_counts, fc, nilled, errors) {
                    // Validate min_occurs for all children
                    self.validate_min_occurs_batch(node, &child_counts, fc, errors);

                    // Validate max_occurs for all children
                    self.validate_max_occurs_batch(node, &child_counts, fc, errors);

                    // Validate sequence order for sequence content models
                    self.validate_sequence_order(node, fc, errors);
                }

                // Validate wildcard occurrence bounds. Without a full
                // content-model automaton this is only decidable when the
                // wildcard is the sole particle (no declared siblings).
                if fc.automaton.is_none()
                    && let Some(ref w) = fc.wildcard
                    && fc.constraints.is_empty()
                {
                    let matched = node
                        .get_child_elements()
                        .iter()
                        .filter(|c| {
                            !fc.constraints.contains_key(&c.get_name())
                                && w.matches(c.get_namespace_uri().as_deref())
                        })
                        .count() as u32;
                    if matched < w.min_occurs {
                        let error = self
                            .make_error(
                                ValidationErrorType::TooFewOccurrences,
                                format!(
                                    "element '{}' requires at least {} wildcard-matched child element(s), found {}",
                                    name, w.min_occurs, matched
                                ),
                                node,
                            )
                            .with_node_name(&name)
                            .with_level(ErrorLevel::Error);
                        self.push_error(errors, error);
                    }
                    if let Some(max) = w.max_occurs {
                        if matched > max {
                            let error = self
                                .make_error(
                                    ValidationErrorType::TooManyOccurrences,
                                    format!(
                                        "element '{}' allows at most {} wildcard-matched child element(s), found {}",
                                        name, max, matched
                                    ),
                                    node,
                                )
                                .with_node_name(&name)
                                .with_level(ErrorLevel::Error);
                            self.push_error(errors, error);
                        }
                    }
                }
            }

            // Validate text content
            self.validate_text_content(node, elem, ids, errors);

            // Validate attributes against the type's attribute declarations
            self.validate_node_attributes(node, elem, ids, errors);

            let wildcard = flattened.as_ref().and_then(|fc| fc.wildcard.clone());
            ParentContext {
                elements: self.element_child_declarations(elem),
                allowed: flattened,
                wildcard,
            }
        } else if is_allowed_by_parent {
            // Element is allowed by the parent type but has no resolvable
            // declaration - nothing further to validate.
            ParentContext::default()
        } else if self.mode == ValidationMode::Strict && schema_has_elements {
            // An undeclared element admitted by a lax wildcard is fine; its
            // subtree keeps lax processing.
            if let Some(w) = parent_ctx.and_then(|ctx| ctx.wildcard.as_ref()) {
                if w.process_contents == ProcessContents::Lax
                    && w.matches(node.get_namespace_uri().as_deref())
                {
                    return ParentContext {
                        wildcard: Some(propagated_wildcard(ProcessContents::Lax)),
                        ..ParentContext::default()
                    };
                }
            }

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

            self.push_error(errors, error);
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
        let ns_info: Vec<Option<(String, String)>> = attrs
            .iter()
            .map(|(k, _)| node.get_attribute_ns_info(k))
            .collect();
        let filtered: Vec<(&str, Option<&str>, &str)> = attrs
            .iter()
            .zip(ns_info.iter())
            .filter(|(_, info)| !matches!(info, Some((_, ns)) if ns == XSI_NS))
            .map(|((k, v), info)| {
                (
                    k.as_str(),
                    info.as_ref().map(|(_, ns)| ns.as_str()),
                    v.as_str(),
                )
            })
            .collect();
        let result = super::attributes::validate_element_attributes(
            &self.schema,
            complex,
            filtered.iter().copied(),
            &mut self.facet_cache.borrow_mut(),
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
            self.push_error(errors, error);
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
