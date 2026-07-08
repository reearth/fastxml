//! Text-content validation with per-type memoized dispatch.
//!
//! Deciding how a declared type's text content must be checked (facets +
//! primitive kind, element-only rejection, or nothing) is a pure function of
//! the type. Resolving it otherwise costs a `get_type` probe plus a
//! facet-cache probe on every element close; [`TextOp`] captures the outcome
//! once per distinct type, keyed by its interned type symbol.

use std::sync::Arc;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::{ContentModel, NsName, TypeDef};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

use super::super::state::ElementContext;
use super::OnePassSchemaValidator;
use super::numeric::{self, NumClass, NumericPlan};

/// Memoized text-validation plan for a declared type (S5).
#[derive(Clone)]
pub(crate) enum TextOp {
    /// The declared type could not be resolved: fall through to the inline /
    /// element-declaration fallbacks (mirrors the original control flow).
    NotFound,
    /// The type is resolved and imposes no text check here (mixed complex
    /// content, or simple content whose base is not a simple type).
    Allow,
    /// Element-only (non-mixed complex) content: non-empty text is an error.
    RejectText,
    /// Simple or simple-content type: validate against these facet
    /// constraints (and the built-in primitive kind they carry).
    Simple(Arc<FacetConstraints>),
    /// Value-check fast path (PR-B) for an *unconstrained* numeric scalar: a
    /// single value of a numeric primitive with no facets beyond
    /// `whiteSpace=collapse`. A lean lexical scan replaces the
    /// `FacetValidator` + regex path; on any rejection we defer to the
    /// canonical slow path (via `constraints`) so error messages stay
    /// byte-identical.
    Numeric {
        /// Numeric lexical family to scan for.
        kind: NumClass,
        /// Canonical constraints for the fallback slow path.
        constraints: Arc<FacetConstraints>,
    },
    /// Value-check fast path (PR-B) for an *unconstrained* numeric list: e.g.
    /// `gml:posList` / `gml:coordinates` (a list of `double`). One pass over
    /// the raw bytes tokenizing on whitespace, scanning each item; deferring
    /// to the slow path on any rejection.
    NumericList {
        /// Numeric lexical family of the list items.
        kind: NumClass,
        /// Canonical constraints for the fallback slow path.
        constraints: Arc<FacetConstraints>,
    },
}

impl OnePassSchemaValidator {
    /// Validates text content against the element's type definition.
    pub(crate) fn validate_text_content_against_type(&mut self, ctx: &ElementContext) {
        // Anti-regression guardrail: count every text-content check
        // unconditionally, before any type resolution or early return.
        self.counters.text_nodes_checked += 1;

        // Fast path: a declared named type resolves through the memoized plan,
        // avoiding a get_type + facet-cache probe per element.
        if let (Some(type_sym), Some(type_ref)) = (ctx.type_sym, ctx.type_ref.clone()) {
            let op = self.resolve_text_op(type_sym, ctx.type_ns.as_ref(), &type_ref);
            if !matches!(op, TextOp::NotFound) {
                self.apply_text_op(ctx, &op);
                return;
            }
            // The type_ref did not resolve to a type: fall through to the
            // inline / element-declaration fallbacks, exactly as before.
        }

        // Elements admitted by a wildcard have no declared type to check.
        if ctx.wildcard_mode.is_some() && ctx.type_ref.is_none() && ctx.inline_type.is_none() {
            return;
        }

        // Inline (anonymous) type captured at element start.
        if let Some(inline_type) = ctx.inline_type.as_ref() {
            self.validate_text_against_type_def(ctx, inline_type);
            return;
        }

        // If no type_ref, try to get the inline type from the element declaration.
        if let Some(inline_type) = self.get_element_inline_type(ctx.name.as_ref()) {
            self.validate_text_against_type_def(ctx, &inline_type);
        }
    }

    /// Returns the (memoized) [`TextOp`] for a declared named type. `type_sym`
    /// encodes the resolved `(namespace, local)` identity, so the cache does
    /// not conflate same-local-name types across namespaces.
    fn resolve_text_op(
        &mut self,
        type_sym: u32,
        type_ns: Option<&NsName>,
        type_ref: &str,
    ) -> TextOp {
        if let Some(op) = self.text_op_cache.get(&type_sym) {
            return op.clone();
        }
        let op = self.compute_text_op(type_ns, type_ref);
        self.text_op_cache.insert(type_sym, op.clone());
        op
    }

    /// Resolves the text-validation plan for a declared type from the schema,
    /// preferring the compile-time resolved `(namespace, local)` identity over
    /// the namespace-blind `type_ref` string (which stays a fallback).
    fn compute_text_op(&mut self, type_ns: Option<&NsName>, type_ref: &str) -> TextOp {
        // C2: borrow the type via a cheap schema-Arc clone, so the borrow
        // lives independently of `self` across the &mut self facet lookup.
        let schema = Arc::clone(&self.schema);
        match schema.type_by_ref(type_ns, type_ref) {
            Some(TypeDef::Simple(simple)) => {
                Self::classify_simple(self.create_facet_constraints(simple))
            }
            Some(TypeDef::Complex(complex)) => {
                if matches!(&complex.content, ContentModel::SimpleContent { .. }) {
                    // C4: ns-first base hop (string fallback inside
                    // complex_base_def).
                    match schema.complex_base_def(complex) {
                        Some(TypeDef::Simple(simple)) => {
                            Self::classify_simple(self.create_facet_constraints(simple))
                        }
                        // Base is not a simple type: original did nothing.
                        _ => TextOp::Allow,
                    }
                } else if !complex.mixed {
                    TextOp::RejectText
                } else {
                    // Mixed complex content: text is allowed, nothing to check.
                    TextOp::Allow
                }
            }
            None => TextOp::NotFound,
        }
    }

    /// Wraps a type's resolved facet constraints in the most specific
    /// [`TextOp`]: the numeric value-check fast path when the type is an
    /// unconstrained numeric scalar / list, otherwise the general
    /// facet-driven [`TextOp::Simple`].
    fn classify_simple(constraints: Arc<FacetConstraints>) -> TextOp {
        match numeric::classify(&constraints) {
            Some(NumericPlan::Scalar(kind)) => TextOp::Numeric { kind, constraints },
            Some(NumericPlan::List(kind)) => TextOp::NumericList { kind, constraints },
            None => TextOp::Simple(constraints),
        }
    }

    /// Applies a resolved [`TextOp`] to the element's accumulated text.
    fn apply_text_op(&mut self, ctx: &ElementContext, op: &TextOp) {
        match op {
            TextOp::Simple(constraints) => self.validate_text_against_facets(ctx, constraints),
            TextOp::Numeric { kind, constraints } => {
                self.validate_numeric_fast(ctx, *kind, constraints, false)
            }
            TextOp::NumericList { kind, constraints } => {
                self.validate_numeric_fast(ctx, *kind, constraints, true)
            }
            TextOp::RejectText => self.reject_text_if_present(ctx),
            TextOp::Allow | TextOp::NotFound => {}
        }
    }

    /// Value-check fast path: scan the accumulated text with the lean numeric
    /// scanner. The scanner is an accelerator only — the canonical slow path
    /// remains the single source of truth for error messages, so on any
    /// rejection (or on the rare constrained cases the fast path does not
    /// model: fixed values, empty nillable/default content) we delegate to
    /// [`validate_text_against_facets`], producing byte-identical output.
    fn validate_numeric_fast(
        &mut self,
        ctx: &ElementContext,
        kind: NumClass,
        constraints: &FacetConstraints,
        is_list: bool,
    ) {
        // Rare element-level cases carry semantics the byte-level scan does not
        // model; hand them to the canonical path unchanged.
        if ctx.fixed_value.is_some()
            || (ctx.text_content.is_empty()
                && (ctx.nillable || ctx.default_value.is_some() || ctx.fixed_value.is_some()))
        {
            self.validate_text_against_facets(ctx, constraints);
            return;
        }

        let bytes = ctx.text_content.as_bytes();
        let ok = if is_list {
            numeric::scan_list(bytes, kind)
        } else {
            numeric::scan_scalar(bytes, kind)
        };
        if !ok {
            // Defer to the canonical path so the emitted error is identical.
            self.validate_text_against_facets(ctx, constraints);
        }
    }

    /// Validates text content against a specific type definition (uncached
    /// path for inline/anonymous types).
    pub(crate) fn validate_text_against_type_def(
        &mut self,
        ctx: &ElementContext,
        type_def: &TypeDef,
    ) {
        match type_def {
            TypeDef::Simple(simple) => {
                let constraints = self.create_facet_constraints(simple);
                self.validate_text_against_facets(ctx, &constraints);
            }
            TypeDef::Complex(complex) => {
                // For complex types with simple content, validate the base type.
                if matches!(&complex.content, ContentModel::SimpleContent { .. }) {
                    // C2: borrow the base simple type via a cheap schema-Arc
                    // clone instead of cloning the TypeDef.
                    let schema = Arc::clone(&self.schema);
                    if let Some(TypeDef::Simple(simple)) = schema.complex_base_def(complex) {
                        let constraints = self.create_facet_constraints(simple);
                        self.validate_text_against_facets(ctx, &constraints);
                    }
                } else if !complex.mixed {
                    self.reject_text_if_present(ctx);
                }
            }
        }
    }

    /// Reports an error if a non-mixed complex element carries text content.
    fn reject_text_if_present(&mut self, ctx: &ElementContext) {
        if !ctx.text_content.trim().is_empty() {
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

    /// Validates the accumulated text content of `ctx` against resolved facet
    /// constraints: user-declared facets plus (when applicable) the built-in
    /// primitive lexical/value-space check. Single source of truth for both
    /// the memoized fast path and the uncached inline path.
    fn validate_text_against_facets(
        &mut self,
        ctx: &ElementContext,
        constraints: &FacetConstraints,
    ) {
        // Resolve the text to validate. An empty element takes its value
        // constraint as its schema-normalized content — `fixed` if present,
        // else `default` — and that value must itself satisfy the type (so a
        // `fixed`/`default` that violates a narrowing `xsi:type` is rejected).
        // A nilled element, or a plain nillable element with no value
        // constraint, contributes no value and is skipped. The fixed-value
        // *match* check is hoisted to `validate_fixed_value` at element end so
        // it also covers untyped (anyType) and mixed content — mirroring DOM.
        let effective_text: &str = if ctx.text_content.is_empty() {
            if ctx.nilled {
                return;
            }
            if let Some(fixed) = ctx.fixed_value.as_deref() {
                fixed
            } else if let Some(default) = ctx.default_value.as_deref() {
                default
            } else if ctx.nillable {
                return;
            } else {
                // Genuinely empty, non-nillable: primitives like xs:integer
                // must still reject the empty string.
                ""
            }
        } else {
            &ctx.text_content
        };

        // User-declared facets. Empty content is still checked — a pattern or
        // enumeration facet can legitimately reject the empty string.
        let validator = FacetValidator::new(constraints);
        if let Err(facet_error) = validator.validate(effective_text) {
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

        // Track ID/IDREF values carried as element content.
        let mut id_values = super::super::attributes::AttrValidation::default();
        super::super::attributes::push_id_values_from_constraints(
            constraints,
            &ctx.text_content,
            &mut id_values,
        );
        self.record_ids(id_values.ids, id_values.idrefs);

        // Built-in primitive lexical/value-space check (reuse the cached
        // value_kind rather than re-resolving the primitive chain).
        if let Some(kind) = constraints.value_kind
            && let Err(prim_error) = kind.validate(effective_text)
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
