//! Content validation (element validation, text content, facets).

use std::sync::Arc;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::{ContentModel, SimpleType, TypeDef};
use crate::schema::xsd::facets::FacetValidator;
use crate::schema::xsd::primitive::PrimitiveKind;

use super::super::ValidationMode;
use super::super::state::ElementContext;
use super::OnePassSchemaValidator;

impl OnePassSchemaValidator {
    /// Main element validation logic.
    pub(crate) fn validate_element(
        &mut self,
        name: &Arc<str>,
        prefix: Option<&Arc<str>>,
        namespace: Option<&str>,
        attributes: &[(&str, &str)],
    ) {
        // Optimization: Try local name lookup first (most common case)
        // Only construct qname if local lookup fails AND prefix exists
        // Also try namespace URI lookup if prefix lookup fails (handles prefix mismatch)
        let elem_def = self.lookup_element_optimized(name, prefix, namespace);

        // Construct qname only when needed for error messages or when prefix exists
        let qname_owned: Option<String> = match prefix {
            Some(p) if !p.is_empty() => Some(format!("{}:{}", p.as_ref(), name.as_ref())),
            _ => None,
        };
        let qname: &str = qname_owned.as_deref().unwrap_or_else(|| name.as_ref());

        // Check if this element is expected by the parent (inline element definition)
        let is_expected_by_parent = self.is_element_expected_by_parent(name);

        let schema_has_elements = !self.schema.elements.is_empty();

        // Priority: inline element definition > global element definition
        // This is important when the same element name exists both as a global element
        // and as an inline element in the parent's content model with different types.
        // For example, gml:exterior in Solid (SurfacePropertyType) vs Polygon (AbstractRingPropertyType)
        if is_expected_by_parent {
            // Try inline element first - declared in parent's type definition
            let (inline_type_ref, inline_flattened) = self.get_inline_element_info(name);

            // Use inline type if available, otherwise fall back to global element
            let (type_ref, flattened_children) =
                if inline_type_ref.is_some() || inline_flattened.is_some() {
                    (inline_type_ref, inline_flattened)
                } else if let Some(elem) = elem_def {
                    // Fall back to global element
                    (
                        elem.type_ref.clone(),
                        self.get_flattened_children_for_element(elem),
                    )
                } else {
                    (None, None)
                };

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Check sequence order against parent's expected constraints
            self.validate_sequence_order(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.flattened_children = flattened_children;
            }
        } else if let Some(elem) = elem_def {
            // Global element found - get type information from cache
            let type_ref = elem.type_ref.clone();
            let flattened_children = self.get_flattened_children_for_element(elem);

            // Check max_occurs against parent's expected constraints
            self.validate_max_occurs(name);

            // Check sequence order against parent's expected constraints
            self.validate_sequence_order(name);

            // Update current element context with type info
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.flattened_children = flattened_children;
            }
        } else {
            // Element not found in schema
            if self.mode == ValidationMode::Strict && schema_has_elements {
                let error = self
                    .make_error(
                        ValidationErrorType::UnknownElement,
                        format!("element '{}' is not declared in schema", qname),
                    )
                    .with_node_name(qname)
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }

        // Validate attributes
        self.validate_attributes(name, attributes);
    }

    /// Validates attributes on an element.
    pub(crate) fn validate_attributes(
        &mut self,
        element_name: &Arc<str>,
        attributes: &[(&str, &str)],
    ) {
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

    /// Accumulates text content for the current element.
    pub(crate) fn validate_text_content(&mut self, text: &str) {
        if let Some(ctx) = self.state.current_element_mut() {
            ctx.text_content.push_str(text);
        }
    }

    /// Validates an element when it closes.
    pub(crate) fn validate_element_end(&mut self, _name: &Arc<str>) {
        // Get the element context being closed
        if let Some(ctx) = self.state.pop_element() {
            // Always run type validation — primitive types (e.g., xs:integer)
            // need to reject empty content, while types whose lexical space
            // allows empty (xs:string and derivatives) pass through cheaply.
            self.validate_text_content_against_type(&ctx);

            // Validate required children were present (minOccurs)
            self.validate_min_occurs(&ctx);
        }
    }

    /// Validates text content against the element's type definition.
    pub(crate) fn validate_text_content_against_type(&mut self, ctx: &ElementContext) {
        // Try to get type definition from type_ref first
        if let Some(ref type_ref) = ctx.type_ref {
            // Note: .cloned() is required to break the borrow from self.schema
            // before calling validate_text_against_type_def which takes &mut self
            if let Some(type_def) = self.schema.get_type(type_ref).cloned() {
                self.validate_text_against_type_def(ctx, &type_def);
                return;
            }
        }

        // If no type_ref, try to get inline type from element definition
        if let Some(inline_type) = self.get_element_inline_type(ctx.name.as_ref()) {
            self.validate_text_against_type_def(ctx, &inline_type);
        }
    }

    /// Validates text content against a specific type definition.
    pub(crate) fn validate_text_against_type_def(
        &mut self,
        ctx: &ElementContext,
        type_def: &TypeDef,
    ) {
        match type_def {
            TypeDef::Simple(simple) => {
                self.validate_text_against_simple_type(ctx, simple);
            }
            TypeDef::Complex(complex) => {
                // For complex types with simple content, validate the base type
                if let ContentModel::SimpleContent { base_type } = &complex.content {
                    if let Some(TypeDef::Simple(simple)) =
                        self.schema.get_type(base_type).cloned().as_ref()
                    {
                        self.validate_text_against_simple_type(ctx, simple);
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
                            .with_node_name(ctx.name.as_ref())
                            .with_level(ErrorLevel::Error);
                        self.add_error(error);
                    }
                }
            }
        }
    }

    /// Validates the accumulated text content of `ctx` against a `SimpleType`:
    /// runs both user-declared facet constraints and (when applicable) the
    /// built-in primitive lexical/value-space check.
    fn validate_text_against_simple_type(&mut self, ctx: &ElementContext, simple: &SimpleType) {
        // User-declared facets. Skip on empty content so we don't double up
        // on top of any primitive-level "empty value" error.
        if !ctx.text_content.is_empty() {
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
                    .with_node_name(ctx.name.as_ref())
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }

        // Built-in primitive lexical/value-space check.
        if let Some(kind) = PrimitiveKind::resolve(&self.schema, simple)
            && let Err(prim_error) = kind.validate(&ctx.text_content)
        {
            let error = self
                .make_error(
                    ValidationErrorType::InvalidTextContent,
                    format!("element '{}': {}", ctx.name, prim_error),
                )
                .with_node_name(ctx.name.as_ref())
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }
    }
}
