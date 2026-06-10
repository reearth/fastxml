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
        let elem_nillable = elem_def.map(|e| e.nillable).unwrap_or(false);
        let elem_abstract = elem_def.map(|e| e.is_abstract).unwrap_or(false);
        let elem_default = elem_def.and_then(|e| e.default.clone());
        let elem_fixed = elem_def.and_then(|e| e.fixed.clone());

        // Construct qname only when needed for error messages or when prefix exists
        let qname_owned: Option<String> = match prefix {
            Some(p) if !p.is_empty() => Some(format!("{}:{}", p.as_ref(), name.as_ref())),
            _ => None,
        };
        let qname: &str = qname_owned.as_deref().unwrap_or_else(|| name.as_ref());

        let elem_known = elem_def.is_some();
        let nilled = attributes
            .iter()
            .any(|&(n, v)| n == "xsi:nil" && v.trim() == "true");

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
                ctx.nillable = elem_nillable;
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
                ctx.nillable = elem_nillable;
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

        // xsi:type substitution: validate the element against the named type
        // instead of the declared one (when the substitution is allowed).
        if let Some(xsi_type) = attributes
            .iter()
            .find(|&&(n, _)| n == "xsi:type")
            .map(|&(_, v)| v)
        {
            let declared = self
                .state
                .current_element()
                .and_then(|ctx| ctx.type_ref.clone());
            match super::super::xsi_type::resolve_xsi_type(
                &self.schema,
                declared.as_deref(),
                xsi_type,
            ) {
                Ok(substituted) => {
                    let flattened = match self.schema.get_type(&substituted) {
                        Some(TypeDef::Complex(complex)) => {
                            Some(Arc::new(self.compute_flattened_children(complex)))
                        }
                        _ => None,
                    };
                    if let Some(ctx) = self.state.current_element_mut() {
                        ctx.type_ref = Some(substituted);
                        if flattened.is_some() {
                            ctx.flattened_children = flattened;
                        }
                    }
                }
                Err(message) => {
                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidAttributeValue,
                            format!("element '{}': {}", qname, message),
                        )
                        .with_node_name(qname)
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }
        }

        // An abstract element may not appear in the instance directly.
        if elem_abstract {
            let error = self
                .make_error(
                    ValidationErrorType::InvalidContent,
                    format!(
                        "element '{}' is abstract and cannot be used directly",
                        qname
                    ),
                )
                .with_node_name(qname)
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }

        // xsi:nil handling: only nillable declarations may carry it.
        if nilled && elem_known && !elem_nillable {
            let error = self
                .make_error(
                    ValidationErrorType::InvalidAttributeValue,
                    format!(
                        "element '{}' is not nillable but has xsi:nil=\"true\"",
                        qname
                    ),
                )
                .with_node_name(qname)
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }
        if let Some(ctx) = self.state.current_element_mut() {
            ctx.nilled = nilled;
            ctx.default_value = elem_default;
            ctx.fixed_value = elem_fixed;
        }

        // Validate attributes
        self.validate_attributes(name, attributes);
    }

    /// Validates attributes on an element against the attribute
    /// declarations of its complex type.
    pub(crate) fn validate_attributes(
        &mut self,
        element_name: &Arc<str>,
        attributes: &[(&str, &str)],
    ) {
        let result = {
            // Resolve the element's complex type: explicit type_ref first,
            // then an inline (anonymous) type from the parent's content model.
            let inline_owned;
            let type_def: Option<&TypeDef> = match self
                .state
                .current_element()
                .and_then(|ctx| ctx.type_ref.as_deref())
            {
                Some(tr) => self.schema.get_type(tr),
                None => {
                    inline_owned = self.get_element_inline_type(element_name);
                    inline_owned.as_ref()
                }
            };

            let Some(TypeDef::Complex(complex)) = type_def else {
                return;
            };

            super::super::attributes::validate_element_attributes(
                &self.schema,
                complex,
                attributes.iter().copied(),
            )
        };

        for message in result.errors {
            let error = self
                .make_error(
                    ValidationErrorType::InvalidAttributeValue,
                    format!("element '{}': {}", element_name, message),
                )
                .with_node_name(element_name.as_ref())
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }
        self.record_ids(result.ids, result.idrefs);
    }

    /// Records `xs:ID` values (checking document-wide uniqueness) and
    /// `xs:IDREF` values (resolved at the end of the document).
    pub(crate) fn record_ids(&mut self, ids: Vec<String>, idrefs: Vec<String>) {
        for id in ids {
            if !self.seen_ids.insert(id.clone()) {
                let error = self
                    .make_error(
                        ValidationErrorType::IdentityConstraint,
                        format!("duplicate ID value '{}'", id),
                    )
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }
        for idref in idrefs {
            self.pending_idrefs
                .push((idref, self.current_line, self.current_column));
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
            // A nilled element must be empty.
            if ctx.nilled && (!ctx.text_content.trim().is_empty() || !ctx.child_counts.is_empty()) {
                let error = self
                    .make_error(
                        ValidationErrorType::InvalidContent,
                        format!(
                            "element '{}' has xsi:nil=\"true\" but is not empty",
                            ctx.name
                        ),
                    )
                    .with_node_name(ctx.name.as_ref())
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }

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
        // Skip everything for an empty element that is nillable or carries a
        // default/fixed value constraint (the constraint value applies).
        if ctx.text_content.is_empty()
            && (ctx.nillable || ctx.default_value.is_some() || ctx.fixed_value.is_some())
        {
            return;
        }

        // User-declared facets. Empty content is still checked — a pattern
        // or enumeration facet can legitimately reject the empty string.
        {
            let constraints = self.create_facet_constraints(simple);

            // Fixed value constraint: non-empty content must match.
            if let Some(ref fixed) = ctx.fixed_value {
                let text = ctx.text_content.trim();
                if text != fixed.trim()
                    && crate::schema::xsd::value_compare::compare_values(
                        constraints.value_kind,
                        text,
                        fixed,
                    ) != Some(std::cmp::Ordering::Equal)
                {
                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidContent,
                            format!(
                                "element '{}' must have the fixed value '{}', found '{}'",
                                ctx.name, fixed, text
                            ),
                        )
                        .with_node_name(ctx.name.as_ref())
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }

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

            // Track ID/IDREF values carried as element content
            let mut id_values = super::super::attributes::AttrValidation::default();
            super::super::attributes::push_id_values_from_constraints(
                &constraints,
                &ctx.text_content,
                &mut id_values,
            );
            self.record_ids(id_values.ids, id_values.idrefs);
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
