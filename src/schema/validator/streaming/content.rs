//! Content validation (element validation, text content, facets).

use std::sync::Arc;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::{ComplexType, TypeDef};
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
        // C1: the interned qualified name is built once at the tag boundary and
        // threaded here, so neither the lookup nor the error paths re-`format!`
        // it. `qualified_name` equals `name` when there is no prefix.
        let qname: &str = qualified_name.as_ref();

        // C3: hold a local clone of the schema Arc so the looked-up ElementDef
        // is decoupled from `self` and stays borrowable across the &mut self
        // calls below. This lets identity constraints be passed as a slice
        // instead of cloning the constraint Vec on every element.
        let schema = Arc::clone(&self.schema);
        // Optimization: Try local name lookup first (most common case)
        // Also try namespace URI lookup if prefix lookup fails (handles prefix mismatch)
        let elem_def = self.lookup_element_optimized(&schema, name, prefix, qname, namespace);
        let elem_nillable = elem_def.map(|e| e.nillable).unwrap_or(false);
        let elem_abstract = elem_def.map(|e| e.is_abstract).unwrap_or(false);
        let elem_default = elem_def.and_then(|e| e.default.clone());
        let elem_fixed = elem_def.and_then(|e| e.fixed.clone());

        let elem_known = elem_def.is_some();
        let elem_constraints: &[crate::schema::types::CompiledConstraint] =
            elem_def.map(|e| e.constraints.as_slice()).unwrap_or(&[]);
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

        let schema_has_elements = !self.schema.elements.is_empty();

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
            let (inline_type_ref, inline_flattened, inline_anon_type) =
                self.get_inline_element_info(child_local_sym, name);

            // Use inline type if available, otherwise fall back to global element
            let (type_ref, flattened_children, anon_type) = if inline_type_ref.is_some()
                || inline_flattened.is_some()
                || inline_anon_type.is_some()
            {
                (inline_type_ref, inline_flattened, inline_anon_type)
            } else if let Some(elem) = elem_def {
                (
                    elem.type_ref.as_deref().map(Arc::from),
                    self.get_flattened_children_for_element(elem),
                    elem.inline_type.clone(),
                )
            } else {
                (None, None, None)
            };

            // Content-model automaton replaces the count-based occurrence
            // and order checks when the parent's type has one.
            if !self.step_parent_automaton(qname, name, namespace, true) {
                // Check max_occurs against parent's expected constraints
                self.validate_max_occurs(name);

                // Check sequence order against parent's expected constraints
                self.validate_sequence_order(name);
            }

            // Update current element context with type info
            let type_sym = type_ref.as_deref().map(|t| self.symbols.intern(t).0);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.type_sym = type_sym;
                ctx.flattened_children = flattened_children;
                ctx.inline_type = anon_type;
                ctx.nillable = elem_nillable;
            }
        } else if let Some(elem) = elem_def {
            // Global element found - get type information from cache
            let type_ref: Option<Arc<str>> = elem.type_ref.as_deref().map(Arc::from);
            let flattened_children = self.get_flattened_children_for_element(elem);
            let anon_type = elem.inline_type.clone();

            if !self.step_parent_automaton(qname, name, namespace, true) {
                // Check max_occurs against parent's expected constraints
                self.validate_max_occurs(name);

                // Check sequence order against parent's expected constraints
                self.validate_sequence_order(name);
            }

            // Update current element context with type info
            let type_sym = type_ref.as_deref().map(|t| self.symbols.intern(t).0);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.type_ref = type_ref;
                ctx.type_sym = type_sym;
                ctx.flattened_children = flattened_children;
                ctx.inline_type = anon_type;
                ctx.nillable = elem_nillable;
            }
        } else if wildcard_mode == Some(crate::schema::types::ProcessContents::Lax) {
            // Undeclared element admitted by a lax wildcard; its subtree
            // keeps lax processing. It still consumes a content-model slot.
            self.step_parent_automaton(qname, name, namespace, false);
            if let Some(ctx) = self.state.current_element_mut() {
                ctx.schema_validated = true;
                ctx.wildcard_mode = Some(crate::schema::types::ProcessContents::Lax);
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
                    let substituted_sym = self.symbols.intern(&substituted).0;
                    let substituted: Arc<str> = Arc::from(substituted);
                    if let Some(ctx) = self.state.current_element_mut() {
                        ctx.type_ref = Some(substituted);
                        ctx.type_sym = Some(substituted_sym);
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

        // Identity constraints: open scopes declared on this element, and
        // match this element against the selectors of enclosing scopes.
        self.identity_element_start(elem_constraints, attributes);

        // Validate attributes
        self.validate_attributes(name, attributes);
    }

    /// The local-name symbol of the current (most recently started) element,
    /// used to key per-parent-type child resolution memoization.
    fn current_local_sym(&self) -> u32 {
        self.state
            .current_element()
            .map(|c| self.symbols.local(super::symbols::SymbolId(c.name_sym)).0)
            .unwrap_or(0)
    }

    /// Handles identity-constraint bookkeeping at element start.
    /// The complex type governing the current (most recently started)
    /// element, when one is resolvable from its declared or inline type.
    /// Used to resolve attribute value-space kinds for identity constraints.
    fn current_element_complex_type(&self) -> Option<&ComplexType> {
        let ctx = self.state.current_element()?;
        let type_def = match ctx.type_ref.as_deref() {
            Some(tr) => self.schema.get_type(tr),
            None => ctx.inline_type.as_ref(),
        };
        match type_def {
            Some(TypeDef::Complex(c)) => Some(c),
            _ => None,
        }
    }

    fn identity_element_start(
        &mut self,
        elem_constraints: &[crate::schema::types::CompiledConstraint],
        attributes: &[(&str, &str)],
    ) {
        // C8 (lazy): with no identity scopes open and no constraints declared
        // on this element, there is nothing to match and nothing to open —
        // skip building the per-element attr_kinds / local_names vectors.
        if self.identity_scopes.is_empty() && elem_constraints.is_empty() {
            return;
        }

        let depth = self.state.element_stack.len();

        // Resolve the value-space kind of each present attribute once, so the
        // identity-constraint field values captured below can be canonicalized
        // (e.g. the xs:integer attributes "1" and "01" denote the same key).
        let attr_kinds: Vec<Option<PrimitiveKind>> = {
            let complex = self.current_element_complex_type();
            attributes
                .iter()
                .map(|&(name, _)| {
                    let local = name.rsplit(':').next().unwrap_or(name);
                    complex.and_then(|c| {
                        super::super::attributes::attribute_primitive_kind(&self.schema, c, local)
                    })
                })
                .collect()
        };

        // Path of local names for elements on the stack (depth 1..=depth).
        let local_names: Vec<&str> = self
            .state
            .element_stack
            .iter()
            .map(|ctx| ctx.name.rsplit(':').next().unwrap_or(ctx.name.as_ref()))
            .collect();

        for scope in &mut self.identity_scopes {
            // Selector match: relative path from just below the scope.
            if depth > scope.depth {
                let rel = &local_names[scope.depth..depth];
                if super::identity::selector_matches(&scope.selector, rel) {
                    let mut fields = vec![super::identity::FieldState::Unset; scope.fields.len()];
                    // Attribute fields on the selected node resolve now.
                    for (i, field) in scope.fields.iter().enumerate() {
                        if field.steps.is_empty()
                            && let Some(ref attr) = field.attr
                        {
                            let value = attributes.iter().enumerate().find_map(|(ai, &(n, v))| {
                                (n.rsplit(':').next().unwrap_or(n) == attr).then_some((ai, v))
                            });
                            if let Some((ai, v)) = value {
                                let canon = crate::schema::xsd::value_compare::canonical_value(
                                    attr_kinds[ai],
                                    v,
                                );
                                fields[i] = super::identity::FieldState::Set(canon);
                            }
                        }
                    }
                    scope
                        .selected
                        .push(super::identity::SelectedState { depth, fields });
                }
            }

            // Attribute fields on elements below a selected node.
            for selected in &mut scope.selected {
                if depth > selected.depth {
                    let rel = &local_names[selected.depth..depth];
                    for (i, field) in scope.fields.iter().enumerate() {
                        if let Some(ref attr) = field.attr
                            && super::identity::field_steps_match(field, rel)
                        {
                            let value = attributes.iter().enumerate().find_map(|(ai, &(n, v))| {
                                (n.rsplit(':').next().unwrap_or(n) == attr).then_some((ai, v))
                            });
                            if let Some((ai, v)) = value {
                                let canon = crate::schema::xsd::value_compare::canonical_value(
                                    attr_kinds[ai],
                                    v,
                                );
                                selected.fields[i] = match selected.fields[i] {
                                    super::identity::FieldState::Unset => {
                                        super::identity::FieldState::Set(canon)
                                    }
                                    _ => super::identity::FieldState::Multiple,
                                };
                            }
                        }
                    }
                }
            }
        }

        // Open new scopes for constraints declared on this element.
        for constraint in elem_constraints {
            if let Some(scope) = super::identity::ScopeState::new(constraint, depth) {
                self.identity_scopes.push(scope);
            }
        }
    }

    /// Handles identity-constraint bookkeeping at element end. `ended_depth`
    /// is the stack depth the element had; `ctx` is its popped context.
    fn identity_element_end(&mut self, ended_depth: usize, ctx: &ElementContext) {
        use crate::schema::types::CompiledConstraintType;
        use crate::schema::xsd::constraints::{ConstraintType, IdentityConstraint, KeyValue};

        // Canonicalize the ending element's text in its value space so a `.`
        // or descendant-text field key compares correctly (e.g. xs:integer
        // "01" and "1" denote the same key).
        let text_kind: Option<PrimitiveKind> = if let Some(tr) = ctx.type_ref.as_deref() {
            self.schema.get_type(tr).and_then(|td| {
                super::super::attributes::element_text_primitive_kind(&self.schema, td)
            })
        } else if let Some(ref it) = ctx.inline_type {
            super::super::attributes::element_text_primitive_kind(&self.schema, it)
        } else {
            None
        };
        let text =
            crate::schema::xsd::value_compare::canonical_value(text_kind, ctx.text_content.trim());
        let ended_local = ctx
            .name
            .rsplit(':')
            .next()
            .unwrap_or(ctx.name.as_ref())
            .to_string();

        // Local names of the still-open ancestors (depth 1..ended_depth).
        let local_names: Vec<String> = self
            .state
            .element_stack
            .iter()
            .map(|c| {
                c.name
                    .rsplit(':')
                    .next()
                    .unwrap_or(c.name.as_ref())
                    .to_string()
            })
            .collect();

        let mut errors: Vec<String> = Vec::new();

        for scope in &mut self.identity_scopes {
            // Element-text fields below a selected node.
            for selected in &mut scope.selected {
                if ended_depth > selected.depth {
                    let mut rel: Vec<&str> = local_names[selected.depth..ended_depth - 1]
                        .iter()
                        .map(|s| s.as_str())
                        .collect();
                    rel.push(&ended_local);
                    for (i, field) in scope.fields.iter().enumerate() {
                        if field.attr.is_none()
                            && !field.steps.is_empty()
                            && super::identity::field_steps_match(field, &rel)
                        {
                            selected.fields[i] = match selected.fields[i] {
                                super::identity::FieldState::Unset => {
                                    super::identity::FieldState::Set(text.clone())
                                }
                                _ => super::identity::FieldState::Multiple,
                            };
                        }
                    }
                }
            }

            // Finalize selected nodes that end here.
            let mut finished = Vec::new();
            scope.selected.retain(|selected| {
                if selected.depth == ended_depth {
                    finished.push(selected.fields.clone());
                    false
                } else {
                    true
                }
            });

            for mut fields in finished {
                // A `.` field takes the selected node's own text content.
                for (i, field) in scope.fields.iter().enumerate() {
                    if field.attr.is_none() && field.steps.is_empty() {
                        fields[i] = super::identity::FieldState::Set(text.clone());
                    }
                }
                if fields
                    .iter()
                    .any(|f| matches!(f, super::identity::FieldState::Multiple))
                {
                    errors.push(format!(
                        "{} '{}': a field matches more than one node",
                        match scope.constraint.constraint_type {
                            CompiledConstraintType::Key => "key",
                            CompiledConstraintType::Unique => "unique",
                            CompiledConstraintType::KeyRef => "keyref",
                        },
                        scope.constraint.name
                    ));
                    continue;
                }
                let values: Vec<String> = fields
                    .into_iter()
                    .map(|f| match f {
                        super::identity::FieldState::Set(v) => v,
                        _ => String::new(), // unset = null (empty per KeyValue)
                    })
                    .collect();
                let value = KeyValue::new(values);
                let ic = IdentityConstraint {
                    name: scope.constraint.name.clone(),
                    constraint_type: match scope.constraint.constraint_type {
                        CompiledConstraintType::Unique => ConstraintType::Unique,
                        CompiledConstraintType::Key => ConstraintType::Key,
                        CompiledConstraintType::KeyRef => ConstraintType::KeyRef,
                    },
                    selector: scope.constraint.selector_xpath.clone(),
                    fields: scope.constraint.field_xpaths.clone(),
                    refer: scope.constraint.refer.clone(),
                };
                if scope.is_keyref() {
                    self.constraint_validator.add_keyref_value(&ic, value);
                } else if value.has_null() {
                    // A key requires every field present; a unique tuple with a
                    // null/absent field simply does not participate in the
                    // uniqueness check (and is not recorded for keyrefs).
                    if scope.constraint.constraint_type == CompiledConstraintType::Key {
                        if let Some(idx) = value.values.iter().position(|v| v.is_empty()) {
                            errors.push(format!(
                                "null value in key field {} of constraint '{}'",
                                idx, scope.constraint.name
                            ));
                        }
                    }
                } else {
                    // Uniqueness is per scoping-element instance: check against
                    // this scope's own `seen` set, not the shared name-keyed
                    // table (that table exists only for cross-scope keyref
                    // resolution, into which the value is unioned).
                    if !scope.seen.insert(value.clone()) {
                        errors.push(format!(
                            "duplicate value {:?} in constraint '{}'",
                            value.values, scope.constraint.name
                        ));
                    }
                    self.constraint_validator.record_key_value(&ic, value);
                }
            }
        }

        // Close scopes whose scoping element ends here.
        self.identity_scopes
            .retain(|scope| scope.depth != ended_depth);

        for message in errors {
            let error = self
                .make_error(ValidationErrorType::IdentityConstraint, message)
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }
    }

    /// Returns the (memoized when named) collected attribute picture of the
    /// current element's complex type, or `None` when it has no complex type.
    ///
    /// Resolution mirrors the previous inline logic: explicit `type_ref`
    /// first, then an inline (anonymous) type on the context, then the
    /// element's inline type from the parent content model. Named types are
    /// cached by type name (C7); anonymous types build fresh.
    fn collected_element_attrs(
        &mut self,
        element_name: &Arc<str>,
    ) -> Option<Arc<super::super::attributes::CollectedAttrs>> {
        use super::super::attributes::CollectedAttrs;

        // Named type via the context's resolved type_ref.
        if let Some(type_ref) = self
            .state
            .current_element()
            .and_then(|ctx| ctx.type_ref.clone())
        {
            if let Some(cached) = self.attr_cache.get(type_ref.as_ref()) {
                return Some(Arc::clone(cached));
            }
            let schema = Arc::clone(&self.schema);
            let TypeDef::Complex(complex) = schema.get_type(&type_ref)? else {
                return None;
            };
            let built = Arc::new(CollectedAttrs::collect(&schema, complex));
            self.attr_cache
                .insert(type_ref.to_string(), Arc::clone(&built));
            return Some(built);
        }

        // Inline (anonymous) type captured on the context at element start.
        if let Some(ctx) = self.state.current_element()
            && let Some(TypeDef::Complex(complex)) = ctx.inline_type.as_ref()
        {
            return Some(Arc::new(CollectedAttrs::collect(&self.schema, complex)));
        }

        // Fallback: the element's inline type from the parent content model.
        let inline_owned = self.get_element_inline_type(element_name);
        if let Some(TypeDef::Complex(complex)) = inline_owned.as_ref() {
            return Some(Arc::new(CollectedAttrs::collect(&self.schema, complex)));
        }
        None
    }

    /// Validates attributes on an element against the attribute
    /// declarations of its complex type.
    pub(crate) fn validate_attributes(
        &mut self,
        element_name: &Arc<str>,
        attributes: &[(&str, &str)],
    ) {
        // C7: resolve the (memoized) collected attribute picture of the
        // element's complex type. Returns None when the element has no complex
        // type, i.e. no attributes to validate.
        let Some(collected) = self.collected_element_attrs(element_name) else {
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
                // count is maintained at element start (DOM parity —
                // non-matching children previously slipped through).
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
