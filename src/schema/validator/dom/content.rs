//! Content validation (text content, facets, error helpers) for DOM validation.

use crate::error::{ErrorLevel, StructuredError, ValidationErrorType};
use crate::node::{NodeType, XmlNode};
use crate::schema::types::{ContentModel, ElementDef, SimpleType, TypeDef};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};
use crate::schema::xsd::primitive::PrimitiveKind;

use super::DomSchemaValidator;

impl DomSchemaValidator {
    /// Collects text content from child nodes.
    pub(crate) fn collect_text_content(&self, node: &XmlNode) -> String {
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

    /// Validates text content against the element's type.
    pub(crate) fn validate_text_content(
        &self,
        node: &XmlNode,
        elem: &ElementDef,
        ids: &mut super::DocIdState,
        errors: &mut Vec<StructuredError>,
    ) {
        let text_content = self.collect_text_content(node);

        // An empty element with a default/fixed value constraint takes the
        // constraint value — nothing further to check.
        if text_content.is_empty() && (elem.default.is_some() || elem.fixed.is_some()) {
            return;
        }

        // Get type definition
        let type_def = if let Some(ref type_ref) = elem.type_ref {
            // C4: ns-first (compile-time resolved), string fallback.
            self.schema
                .type_by_ref(elem.type_ns.as_ref(), type_ref)
                .cloned()
        } else {
            elem.inline_type.clone()
        };

        // Fixed value constraint: non-empty content must match.
        if let Some(ref fixed) = elem.fixed {
            let kind = match &type_def {
                Some(TypeDef::Simple(simple)) => PrimitiveKind::resolve(&self.schema, simple),
                _ => None,
            };
            let text = text_content.trim();
            if text != fixed.trim()
                && crate::schema::xsd::value_compare::compare_values(kind, text, fixed)
                    != Some(std::cmp::Ordering::Equal)
            {
                let node_name = node.get_name();
                let error = self
                    .make_error(
                        ValidationErrorType::InvalidContent,
                        format!(
                            "element '{}' must have the fixed value '{}', found '{}'",
                            node_name, fixed, text
                        ),
                        node,
                    )
                    .with_node_name(&node_name)
                    .with_level(ErrorLevel::Error);
                self.push_error(errors, error);
            }
        }

        match type_def {
            Some(TypeDef::Simple(simple)) => {
                // Run both facet and primitive (lexical/value-space) checks.
                // We do not skip on empty text — primitives like xs:integer
                // must reject empty content, while xs:string-derived types
                // resolve to no PrimitiveKind and pass through unchanged.
                self.validate_simple_type_facets(
                    node,
                    &simple,
                    &text_content,
                    elem.nillable,
                    ids,
                    errors,
                );
            }
            Some(TypeDef::Complex(complex)) => {
                // Check for SimpleContent with base type
                if matches!(&complex.content, ContentModel::SimpleContent { .. }) {
                    // C4: ns-first base hop (string fallback inside).
                    if let Some(TypeDef::Simple(simple)) = self.schema.complex_base_def(&complex) {
                        let simple = simple.clone();
                        self.validate_simple_type_facets(
                            node,
                            &simple,
                            &text_content,
                            elem.nillable,
                            ids,
                            errors,
                        );
                    }
                } else if !complex.mixed && !text_content.is_empty() {
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

                            self.push_error(errors, error);
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
        node: &XmlNode,
        simple: &SimpleType,
        text_content: &str,
        nillable: bool,
        ids: &mut super::DocIdState,
        errors: &mut Vec<StructuredError>,
    ) {
        // Skip everything for an empty, nillable element: `xsi:nil="true"`
        // legitimately leaves the content empty.
        if text_content.is_empty() && nillable {
            return;
        }

        // User-declared facets (minLength, pattern, enumeration, …).
        // Empty content is still checked — a pattern or enumeration facet
        // can legitimately reject the empty string.
        {
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

                self.push_error(errors, error);
            }

            // Record the node's value-space kind for identity constraints
            if let Some(kind) = constraints.value_kind {
                ids.node_kinds.insert(node.id(), kind);
            }

            // Track ID/IDREF values carried as element content
            let mut id_values = super::super::attributes::AttrValidation::default();
            super::super::attributes::push_id_values_from_constraints(
                &constraints,
                text_content,
                &mut id_values,
            );
            for message in ids.record(id_values.ids, id_values.idrefs, node.line(), node.column()) {
                let node_name = node.get_name();
                let error = self
                    .make_error(ValidationErrorType::IdentityConstraint, message, node)
                    .with_node_name(&node_name)
                    .with_level(ErrorLevel::Error);
                self.push_error(errors, error);
            }
        }

        // Built-in primitive lexical/value-space check (e.g., xs:integer
        // rejecting "1.5" or "", xs:int rejecting 2147483648).
        if let Some(kind) = PrimitiveKind::resolve(&self.schema, simple)
            && let Err(prim_error) = kind.validate(text_content)
        {
            let node_name = node.get_name();
            let error = self
                .make_error(
                    ValidationErrorType::InvalidTextContent,
                    format!("element '{}': {}", node_name, prim_error),
                    node,
                )
                .with_node_name(&node_name)
                .with_level(ErrorLevel::Error);

            self.push_error(errors, error);
        }
    }

    /// Returns (memoized) FacetConstraints for a SimpleType definition.
    pub(crate) fn create_facet_constraints(
        &self,
        simple: &SimpleType,
    ) -> std::sync::Arc<FacetConstraints> {
        self.facet_cache.borrow_mut().get(&self.schema, simple)
    }

    /// Creates a structured error with context.
    pub(crate) fn make_error(
        &self,
        error_type: ValidationErrorType,
        message: impl Into<String>,
        node: &XmlNode,
    ) -> StructuredError {
        let mut error = StructuredError::new(message, error_type);
        if let Some(line) = node.line() {
            error = error.with_line(line);
        }
        if let Some(column) = node.column() {
            error = error.with_column(column);
        }
        error
    }

    /// Checks if we should add more errors.
    pub(crate) fn should_add_error(&self, errors: &[StructuredError]) -> bool {
        self.max_errors == 0 || errors.len() < self.max_errors
    }
}
