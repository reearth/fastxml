//! Content validation (element validation, text content, facets).

use std::sync::Arc;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::{NsName, TypeDef};
use crate::schema::xsd::primitive::PrimitiveKind;

use super::super::ValidationMode;
use super::super::state::ElementContext;
use super::OnePassSchemaValidator;

impl OnePassSchemaValidator {
    /// Main element validation logic.
    pub(crate) fn validate_element(
        &mut self,
        name: &Arc<str>,
        prefix: Option<&str>,
        qualified_name: &Arc<str>,
        namespace: Option<&str>,
        attributes: &[(&str, &str)],
    ) {
        // Anti-regression guardrail: count every element unconditionally,
        // before any lookup or early return.
        self.counters.elements_validated += 1;
        // The interned qualified name is built once at the tag boundary and
        // threaded here, so neither the lookup nor the error paths re-`format!`
        // it. `qualified_name` equals `name` when there is no prefix.
        let qname: &str = qualified_name.as_ref();

        // Hold a local clone of the schema Arc so the looked-up ElementDef
        // is decoupled from `self` and stays borrowable across the &mut self
        // calls below. This lets identity constraints be passed as a slice
        // instead of cloning the constraint Vec on every element.
        let schema = Arc::clone(&self.schema);
        // The global declaration of this element's expanded name, if any.
        let elem_def = self
            .resolver
            .lookup(&schema, name, prefix, qname, namespace);
        // Value constraints and nillability come from the governing element
        // declaration. For a child matched inline in the parent's content
        // model, the local declaration governs; these are merged in below.
        let mut elem_nillable = elem_def.map(|e| e.nillable).unwrap_or(false);
        let mut elem_default = elem_def.and_then(|e| e.default.clone());
        let mut elem_fixed = elem_def.and_then(|e| e.fixed.clone());

        let mut elem_known = elem_def.is_some();
        // The local declaration this child matched in its parent's content
        // model, when that declaration is not a `ref=` to a global: it then
        // governs instead of any same-named global declaration.
        let mut local_decl: Option<Arc<super::lookup::InlineResolved>> = None;
        let nilled = attributes
            .iter()
            .any(|&(n, v)| n == "xsi:nil" && v.trim() == "true");

        // Check if this element is expected by the parent (inline element definition)
        let is_expected_by_parent = self.is_element_expected_by_parent(name);

        // Count this child toward the parent's wildcard occurrence bound when
        // its namespace matches the wildcard's namespace set. The bound check
        // at element end (only decidable when the wildcard is the sole
        // particle) must not count non-matching children (DOM parity).
        {
            let len = self.state.element_stack.len();
            if len >= 2 {
                let matches = self
                    .state
                    .element_stack
                    .get(len - 2)
                    .and_then(|parent| parent.flattened_children.as_ref())
                    .and_then(|fc| {
                        fc.wildcard
                            .as_ref()
                            .filter(|_| fc.constraints.is_empty())
                            .map(|w| w.matches(namespace))
                    })
                    .unwrap_or(false);
                if matches && let Some(parent) = self.state.element_stack.get_mut(len - 2) {
                    parent.wildcard_matched += 1;
                }
            }
        }

        // Wildcard handling: a skip wildcard admits this whole subtree
        // without validation; a lax wildcard admits undeclared elements.
        let wildcard_mode = self.parent_wildcard_mode(name, namespace, is_expected_by_parent);
        if wildcard_mode == Some(crate::schema::types::ProcessContents::Skip) {
            // The skipped child still consumes a slot in the parent's
            // content model.
            self.step_parent_automaton(qname, name, namespace, false);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.wildcard_mode = Some(crate::schema::types::ProcessContents::Skip);
            }
            return;
        }

        // Priority: inline element definition > global element definition
        // This is important when the same element name exists both as a global element
        // and as an inline element in the parent's content model with different types.
        // For example, gml:exterior in Solid (SurfacePropertyType) vs Polygon (AbstractRingPropertyType)
        if is_expected_by_parent {
            // Try the inline element (declared in the parent's type) first.
            // The global-element fallback is computed only when the inline
            // lookup finds nothing, so the hot path (inline hit) no longer
            // pays for a redundant per-element type resolution. `elem_def`
            // borrows the locally-cloned schema Arc, so it stays valid across
            // the &mut self inline lookup.
            let child_local_sym = self.current_local_sym();
            let info = self.get_inline_element_info(child_local_sym, name);

            // When the child matches a local declaration in the parent's
            // content model, that declaration's value constraints govern.
            // Merge monotonically (prefer a present local constraint, else keep
            // the global one) so a `ref=` to a global element with a fixed
            // value is not lost.
            if info.found {
                elem_known = true;
                if !info.is_ref {
                    // A local declaration (not a reference) governs alone.
                    elem_nillable = false;
                    elem_default = None;
                    elem_fixed = None;
                    local_decl = Some(Arc::clone(&info));
                }
                if let Some(positional) = info.positional.as_ref() {
                    // Same name declared at multiple positions with differing
                    // value constraints: pick by occurrence index. The parent
                    // counted this child at push time, so occurrence N (1-based)
                    // maps to declaration N-1, clamped to the last (excess
                    // occurrences are content-model errors reported elsewhere).
                    let len = self.state.element_stack.len();
                    let occurrence = if len >= 2 {
                        self.state.element_stack[len - 2].get_child_count(name)
                    } else {
                        1
                    };
                    let idx = (occurrence.max(1) as usize - 1).min(positional.len() - 1);
                    let vc = &positional[idx];
                    if vc.default.is_some() {
                        elem_default = vc.default.clone();
                    }
                    if vc.fixed.is_some() {
                        elem_fixed = vc.fixed.clone();
                    }
                    elem_nillable = elem_nillable || vc.nillable;
                } else {
                    if info.default.is_some() {
                        elem_default = info.default.clone();
                    }
                    if info.fixed.is_some() {
                        elem_fixed = info.fixed.clone();
                    }
                    elem_nillable = elem_nillable || info.nillable;
                }
            }

            // Use inline type if available, otherwise fall back to global element
            let (type_ref, type_ns, flattened_children, anon_type) = if info.type_ref.is_some()
                || info.flattened.is_some()
                || info.inline_type.is_some()
            {
                (
                    info.type_ref.clone(),
                    info.type_ns.clone(),
                    info.flattened.clone(),
                    info.inline_type.clone(),
                )
            } else if let Some(elem) = elem_def {
                (
                    elem.type_ref.as_deref().map(Arc::from),
                    elem.type_ns.clone(),
                    self.get_flattened_children_for_element(elem),
                    elem.inline_type.clone(),
                )
            } else {
                (None, None, None, None)
            };

            // Content-model automaton replaces the count-based occurrence
            // and order checks when the parent's type has one.
            if !self.step_parent_automaton(qname, name, namespace, true) {
                // Check max_occurs against parent's expected constraints
                self.validate_max_occurs(name);

                // Check sequence order against parent's expected constraints
                self.validate_sequence_order(name);
            }

            // Update current element context with type info. The type symbol
            // encodes the resolved (namespace, local) identity so child
            // resolution keyed on it is collision-free across namespaces.
            let type_sym = self.type_identity_sym(type_ns.as_ref(), type_ref.as_deref());
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.type_sym = type_sym;
                ctx.type_ns = type_ns;
                ctx.flattened_children = flattened_children;
                ctx.inline_type = anon_type;
            }
        } else if let Some(elem) = elem_def {
            // Global element found - get type information from cache
            let type_ref: Option<Arc<str>> = elem.type_ref.as_deref().map(Arc::from);
            let type_ns = elem.type_ns.clone();
            let flattened_children = self.get_flattened_children_for_element(elem);
            let anon_type = elem.inline_type.clone();

            if !self.step_parent_automaton(qname, name, namespace, true) {
                // Check max_occurs against parent's expected constraints
                self.validate_max_occurs(name);

                // Check sequence order against parent's expected constraints
                self.validate_sequence_order(name);
            }

            // Update current element context with type info. The type symbol
            // encodes the resolved (namespace, local) identity so child
            // resolution keyed on it is collision-free across namespaces.
            let type_sym = self.type_identity_sym(type_ns.as_ref(), type_ref.as_deref());
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.type_sym = type_sym;
                ctx.type_ns = type_ns;
                ctx.flattened_children = flattened_children;
                ctx.inline_type = anon_type;
            }
        } else if wildcard_mode == Some(crate::schema::types::ProcessContents::Lax) {
            // Undeclared element admitted by a lax wildcard; its subtree
            // keeps lax processing. It still consumes a content-model slot.
            self.step_parent_automaton(qname, name, namespace, false);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.wildcard_mode = Some(crate::schema::types::ProcessContents::Lax);
            }
        } else if self.resolves_xsi_type(&schema, attributes) {
            // Undeclared, but its xsi:type names a schema type: the element is
            // validated against that type (applied below) and still occupies
            // a slot in the parent's content model.
            self.step_parent_automaton(qname, name, namespace, true);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
            }
        } else {
            // Element not found in schema
            if self.mode == ValidationMode::Strict && self.resolver.reports_undeclared {
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
            let (declared, has_inline) = self
                .state
                .current_element()
                .map(|ctx| (ctx.type_ref.clone(), ctx.inline_type.is_some()))
                .unwrap_or((None, false));
            // The xsi:type QName is interpreted against the instance
            // document's in-scope namespace declarations.
            match super::super::xsi_type::resolve_xsi_type(
                &schema,
                super::super::xsi_type::DeclaredType::from_parts(declared.as_deref(), has_inline),
                xsi_type,
                |p| self.state.resolve_prefix(p).map(str::to_string),
            ) {
                Ok(substituted) => {
                    // The substituted type's own children picture, automaton
                    // included (the compile-time cache by expanded name, else
                    // the string key's cache entry or a run-time flattening).
                    let flattened = match substituted
                        .ns
                        .as_ref()
                        .and_then(|ns| self.schema.ns_type_children_cache.get(ns))
                    {
                        Some(cached) => Some(Arc::clone(cached)),
                        None => self.resolve_children_for_type_ref(&substituted.key),
                    };
                    let substituted_sym =
                        self.type_identity_sym(substituted.ns.as_ref(), Some(&substituted.key));
                    let type_ref: Arc<str> = Arc::from(substituted.key);
                    if let Some(ctx) = self.state.current_element_mut() {
                        ctx.type_ref = Some(type_ref);
                        ctx.type_sym = substituted_sym;
                        // The substituted type replaces the declared one, so
                        // its own expanded name (or none, falling back to the
                        // string key) replaces the declared `type_ns`.
                        ctx.type_ns = substituted.ns;
                        ctx.inline_type = None;
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

        // An abstract element may not appear in the instance directly (only
        // global declarations can be abstract).
        if local_decl.is_none() && elem_def.is_some_and(|e| e.is_abstract) {
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
            // Only a nillable declaration can be nilled; on any other the
            // xsi:nil attribute is an error (above) and the content is
            // validated as usual.
            ctx.nilled = nilled && elem_nillable;
            ctx.default_value = elem_default;
            ctx.fixed_value = elem_fixed;
        }

        // Identity constraints: open scopes declared on this element, and
        // match this element against the selectors of enclosing scopes.
        let elem_constraints: &[crate::schema::types::CompiledConstraint] = match &local_decl {
            Some(local) => &local.constraints,
            None => elem_def.map(|e| e.constraints.as_slice()).unwrap_or(&[]),
        };
        self.identity_element_start(elem_constraints, attributes);

        // Validate attributes
        self.validate_attributes(name, attributes);
    }

    /// Whether the element carries an `xsi:type` naming a type of the
    /// schema, so an otherwise undeclared element can be validated against
    /// that type.
    fn resolves_xsi_type(
        &self,
        schema: &crate::schema::types::CompiledSchema,
        attributes: &[(&str, &str)],
    ) -> bool {
        attributes
            .iter()
            .find(|&&(n, _)| n == "xsi:type")
            .is_some_and(|&(_, v)| {
                super::super::xsi_type::resolve_xsi_type(
                    schema,
                    super::super::xsi_type::DeclaredType::AnyType,
                    v,
                    |p| self.state.resolve_prefix(p).map(str::to_string),
                )
                .is_ok()
            })
    }

    /// The namespace-safe cache-key symbol for an element's resolved type.
    /// When the compile-time `(namespace, local)` identity is known it is
    /// interned as an integer pair — so same-local-name types in different
    /// namespaces (e.g. two `ct-A` types) get distinct symbols — otherwise the
    /// bare `type_ref` string is interned as a namespace-blind fallback. No
    /// per-element allocation: interning is idempotent and pair keys are
    /// integers.
    fn type_identity_sym(
        &mut self,
        type_ns: Option<&NsName>,
        type_ref: Option<&str>,
    ) -> Option<u32> {
        match (type_ns, type_ref) {
            (Some(ns), _) => {
                let ns_sym = self.symbols.intern(&ns.namespace_uri);
                let local_sym = self.symbols.intern(&ns.local_name);
                Some(self.symbols.intern_pair(ns_sym, local_sym).0)
            }
            (None, Some(tr)) => Some(self.symbols.intern(tr).0),
            (None, None) => None,
        }
    }

    /// The local-name symbol of the current (most recently started) element,
    /// used to key per-parent-type child resolution memoization.
    fn current_local_sym(&self) -> u32 {
        self.state
            .current_element()
            .map(|c| self.symbols.local(super::symbols::SymbolId(c.name_sym)).0)
            .unwrap_or(0)
    }

    /// Returns the (memoized when named) collected attribute picture of the
    /// current element's type, or `None` when it has no type (xs:anyType,
    /// undeclared, or unresolvable), i.e. no attribute constraints.
    ///
    /// Resolution: the named type (`type_ref`) first, then the anonymous type
    /// captured on the context. A simple type admits no attributes. Named
    /// types are cached by type identity; anonymous types build fresh.
    fn collected_element_attrs(&mut self) -> Option<Arc<super::super::attributes::CollectedAttrs>> {
        use super::super::attributes::CollectedAttrs;

        // Named type via the context's resolved type identity. The cache keys
        // on the ns-safe type symbol so same-local-name types in different
        // namespaces do not share collected-attribute sets; resolution prefers
        // the resolved `type_ns` over the namespace-blind `type_ref` string.
        if let Some((type_ref, type_ns, type_sym)) = self.state.current_element().and_then(|ctx| {
            ctx.type_ref
                .clone()
                .map(|tr| (tr, ctx.type_ns.clone(), ctx.type_sym))
        }) {
            if let Some(sym) = type_sym
                && let Some(cached) = self.attr_cache.get(&sym)
            {
                return Some(Arc::clone(cached));
            }
            let schema = Arc::clone(&self.schema);
            let built = Arc::new(match schema.type_by_ref(type_ns.as_ref(), &type_ref)? {
                TypeDef::Complex(complex) => CollectedAttrs::collect(&schema, complex),
                TypeDef::Simple(_) => CollectedAttrs::none(),
            });
            if let Some(sym) = type_sym {
                self.attr_cache.insert(sym, Arc::clone(&built));
            }
            return Some(built);
        }

        // Inline (anonymous) type captured on the context at element start.
        match self.state.current_element()?.inline_type.as_ref()? {
            TypeDef::Complex(complex) => {
                Some(Arc::new(CollectedAttrs::collect(&self.schema, complex)))
            }
            TypeDef::Simple(_) => Some(Arc::new(CollectedAttrs::none())),
        }
    }

    /// Validates attributes on an element against the attribute
    /// declarations of its complex type.
    pub(crate) fn validate_attributes(
        &mut self,
        element_name: &Arc<str>,
        attributes: &[(&str, &str)],
    ) {
        // resolve the (memoized) collected attribute picture of the
        // element's complex type. Returns None when the element has no complex
        // type, i.e. no attributes to validate.
        let Some(collected) = self.collected_element_attrs() else {
            return;
        };

        let result = {
            // Resolve each attribute's namespace from its prefix using the
            // in-scope namespace declarations (unprefixed attributes are in
            // no namespace).
            let with_ns: Vec<(&str, Option<&str>, &str)> = attributes
                .iter()
                .map(|&(name, value)| {
                    let ns = name
                        .split_once(':')
                        .and_then(|(prefix, _)| self.state.resolve_prefix(prefix));
                    (name, ns, value)
                })
                .collect();
            super::super::attributes::validate_element_attributes(
                &self.schema,
                &collected,
                with_ns.iter().copied(),
                &mut self.facet_cache,
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

    /// Returns the wildcard processing mode the parent applies to this
    /// element: a propagated lax/skip subtree mode, or the parent content
    /// model's wildcard when the element is not declared there.
    fn parent_wildcard_mode(
        &self,
        name: &Arc<str>,
        namespace: Option<&str>,
        is_expected_by_parent: bool,
    ) -> Option<crate::schema::types::ProcessContents> {
        let len = self.state.element_stack.len();
        if len < 2 {
            return None;
        }
        let parent = self.state.element_stack.get(len - 2)?;
        if let Some(mode) = parent.wildcard_mode {
            // Inside a skipped subtree everything is skipped; inside a lax
            // subtree undeclared elements stay lax.
            match mode {
                crate::schema::types::ProcessContents::Skip => return Some(mode),
                crate::schema::types::ProcessContents::Lax if !is_expected_by_parent => {
                    return Some(mode);
                }
                _ => {}
            }
        }
        if is_expected_by_parent {
            return None;
        }
        let fc = parent.flattened_children.as_ref()?;
        let w = fc.wildcard.as_ref()?;
        if !fc.constraints.contains_key(name.as_ref()) && w.matches(namespace) {
            Some(w.process_contents)
        } else {
            None
        }
    }

    /// Validates an element when it closes.
    /// Enforces an element's `fixed` value constraint on non-empty content,
    /// comparing in the value space of the element's simple type (e.g. fixed
    /// `1.0` matches content `1.00`). Empty content takes the fixed value as
    /// its effective content and is trivially satisfied. Mirrors the DOM
    /// engine's fixed-value check and is deliberately type-independent so it
    /// covers untyped (anyType) and mixed content too.
    fn validate_fixed_value(&mut self, ctx: &ElementContext) {
        let Some(fixed) = ctx.fixed_value.clone() else {
            return;
        };
        if ctx.text_content.is_empty() {
            return;
        }
        // Resolve the primitive kind from the element's simple type only
        // (None for complex or untyped content → lexical comparison), matching
        // the DOM engine.
        let kind = if let Some(tr) = ctx.type_ref.as_deref() {
            // ns-first (compile-time resolved), string fallback.
            match self.schema.type_by_ref(ctx.type_ns.as_ref(), tr) {
                Some(TypeDef::Simple(s)) => PrimitiveKind::resolve(&self.schema, s),
                _ => None,
            }
        } else if let Some(TypeDef::Simple(s)) = ctx.inline_type.as_ref() {
            PrimitiveKind::resolve(&self.schema, s)
        } else {
            None
        };

        let text = ctx.text_content.trim();
        if text != fixed.trim()
            && crate::schema::xsd::value_compare::compare_values(kind, text, &fixed)
                != Some(std::cmp::Ordering::Equal)
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

    pub(crate) fn validate_element_end(&mut self) {
        // Get the element context being closed
        if let Some(ctx) = self.state.pop_element() {
            // Identity constraint bookkeeping (works on the popped depth)
            if !self.identity_scopes.is_empty() {
                let ended_depth = self.state.element_stack.len() + 1;
                self.identity_element_end(ended_depth, &ctx);
            }
            // A subtree admitted by a skip wildcard is not validated.
            if ctx.wildcard_mode == Some(crate::schema::types::ProcessContents::Skip) {
                return;
            }

            // Wildcard occurrence bounds, decidable only when the wildcard
            // is the sole particle of the content model.
            if let Some(fc) = &ctx.flattened_children
                && let Some(w) = &fc.wildcard
                && fc.constraints.is_empty()
            {
                // Only children whose namespace matched the wildcard's
                // namespace set participate in the occurrence bound; the
                // count is maintained at element start (as in the DOM
                // engine).
                let matched: u32 = ctx.wildcard_matched;
                if matched < w.min_occurs {
                    let error = self
                        .make_error(
                            ValidationErrorType::TooFewOccurrences,
                            format!(
                                "element '{}' requires at least {} wildcard-matched child element(s), found {}",
                                ctx.name, w.min_occurs, matched
                            ),
                        )
                        .with_node_name(ctx.name.as_ref())
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
                if let Some(max) = w.max_occurs
                    && matched > max
                {
                    let error = self
                        .make_error(
                            ValidationErrorType::TooManyOccurrences,
                            format!(
                                "element '{}' allows at most {} wildcard-matched child element(s), found {}",
                                ctx.name, max, matched
                            ),
                        )
                        .with_node_name(ctx.name.as_ref())
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }

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

            // Element fixed-value constraint, checked independently of the
            // element's type (like the DOM engine) so it also covers untyped
            // (anyType) and mixed content that the type-driven text path skips.
            self.validate_fixed_value(&ctx);

            // Always run type validation — primitive types (e.g., xs:integer)
            // need to reject empty content, while types whose lexical space
            // allows empty (xs:string and derivatives) pass through cheaply.
            self.validate_text_content_against_type(&ctx);

            // Validate required children were present: the content-model
            // automaton's acceptance check when available, count-based
            // minOccurs otherwise.
            if !self.finish_automaton(&ctx) {
                self.validate_min_occurs(&ctx);
            }
        }
    }
}
