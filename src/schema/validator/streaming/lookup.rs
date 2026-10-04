//! Element lookup and type resolution for streaming validation.

use std::sync::Arc;

use crate::schema::types::{
    ComplexType, ElementDef, FlattenedChildren, NsName, SimpleType, TypeDef,
};
use crate::schema::xsd::facets::FacetConstraints;

use super::OnePassSchemaValidator;

/// Memoized resolution of a child element within a parent's content model
/// (see [`OnePassSchemaValidator::get_inline_element_info`]).
#[derive(Default)]
pub(crate) struct InlineResolved {
    /// The child's declared named type, shared as an `Arc<str>`.
    pub type_ref: Option<Arc<str>>,
    /// The child's compile-time resolved `(namespace, local)` type identity,
    /// when the declaration carried one. Preferred over `type_ref` (which is
    /// namespace-blind) when binding the child's context.
    pub type_ns: Option<NsName>,
    /// The child's flattened children (for its own content model).
    pub flattened: Option<Arc<FlattenedChildren>>,
    /// The child's anonymous inline type, if any.
    pub inline_type: Option<TypeDef>,
    /// Whether a matching local element declaration was found at all (the
    /// value constraints below are meaningful only when this is true).
    pub found: bool,
    /// Whether the matched local declaration is a `ref=` to a global
    /// element (whose own declaration then governs).
    pub is_ref: bool,
    /// The local declaration's identity constraints.
    pub constraints: Vec<crate::schema::types::CompiledConstraint>,
    /// The local declaration's `default` value constraint, if any.
    pub default: Option<String>,
    /// The local declaration's `fixed` value constraint, if any.
    pub fixed: Option<String>,
    /// Whether the local declaration is nillable.
    pub nillable: bool,
    /// When the content model declares this name at multiple positions with
    /// *differing* value constraints (e.g. a sequence of two same-name
    /// elements with distinct `fixed` values), the per-declaration constraints
    /// in declaration order; the caller picks by the child's occurrence index.
    pub positional: Option<Arc<Vec<ValueConstraint>>>,
}

/// The value-constraint facet of one element declaration.
#[derive(Clone)]
pub(crate) struct ValueConstraint {
    pub default: Option<String>,
    pub fixed: Option<String>,
    pub nillable: bool,
}

impl OnePassSchemaValidator {
    /// Gets the pre-computed flattened children for an element from the schema cache.
    ///
    /// This uses the namespace-aware `ns_type_children_cache` as the primary lookup,
    /// which uses (namespace_uri, local_name) keys to avoid cross-namespace collisions.
    /// Falls back to runtime computation if not cached.
    pub(crate) fn get_flattened_children_for_element(
        &mut self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // the compile-time resolved (namespace, local) of the type
        // reference probes the owning-namespace-keyed cache directly — one
        // hash lookup, no allocation, immune to prefix-table poisoning.
        if let Some(ref type_ns) = elem.type_ns
            && let Some(cached) = self.schema.ns_type_children_cache.get(type_ns)
        {
            return Some(Arc::clone(cached));
        }
        // Try to get from type reference first (memoized: resolving the
        // type_ref otherwise allocates a two-String NsName per element).
        if let Some(ref type_ref) = elem.type_ref {
            return self.resolve_children_for_type_ref(type_ref);
        }

        // Anonymous type
        if let Some(TypeDef::Complex(complex)) = &elem.inline_type {
            return Some(self.runtime_children(complex));
        }

        None
    }

    /// Resolves (and memoizes) the [`FlattenedChildren`] for a named type
    /// reference: the namespace-aware `ns_type_children_cache` first, then a
    /// run-time flattening. Caching the result means the `NsName` allocation
    /// and the cache probe happen once per distinct type instead of once per
    /// element.
    pub(crate) fn resolve_children_for_type_ref(
        &mut self,
        type_ref: &str,
    ) -> Option<Arc<FlattenedChildren>> {
        if let Some(cached) = self.type_ref_children.get(type_ref) {
            return cached.clone();
        }
        let resolved = self.compute_children_for_type_ref(type_ref);
        // Debug-only: memoization must match a fresh resolution.
        #[cfg(test)]
        {
            let fresh = self.compute_children_for_type_ref(type_ref);
            debug_assert_eq!(
                resolved.as_deref().map(|f| f.ordered_elements.clone()),
                fresh.as_deref().map(|f| f.ordered_elements.clone()),
                "memoized children for type_ref {type_ref:?} differ from fresh lookup"
            );
        }
        self.type_ref_children
            .insert(type_ref.to_string(), resolved.clone());
        resolved
    }

    /// The uncached resolution used by [`Self::resolve_children_for_type_ref`].
    fn compute_children_for_type_ref(&mut self, type_ref: &str) -> Option<Arc<FlattenedChildren>> {
        // Namespace-aware cache lookup first.
        if let Some(ns_name) = self.schema.resolve_type_ref_to_ns(type_ref) {
            if let Some(cached) = self.schema.ns_type_children_cache.get(&ns_name) {
                return Some(Arc::clone(cached));
            }
        }
        // Fallback: flatten at run time.
        let schema = Arc::clone(&self.schema);
        if let Some(TypeDef::Complex(complex)) = schema.get_type(type_ref) {
            return Some(self.runtime_children(complex));
        }
        None
    }

    /// Flattened children (with content-model automaton) of a type outside
    /// the compile-time cache, memoized per type.
    pub(crate) fn runtime_children(&mut self, complex: &ComplexType) -> Arc<FlattenedChildren> {
        self.runtime_types.children(&self.schema, complex)
    }

    /// Returns (memoized) FacetConstraints for a SimpleType definition.
    pub(crate) fn create_facet_constraints(
        &mut self,
        simple: &SimpleType,
    ) -> std::sync::Arc<FacetConstraints> {
        self.facet_cache.get(&self.schema, simple)
    }

    /// Checks if an element is expected by its parent (defined in parent's content model).
    pub(crate) fn is_element_expected_by_parent(&self, name: &Arc<str>) -> bool {
        if self.state.element_stack.len() < 2 {
            return false;
        }
        let parent_idx = self.state.element_stack.len() - 2;
        if let Some(parent) = self.state.element_stack.get(parent_idx) {
            parent.expects_child(name.as_ref())
        } else {
            false
        }
    }

    /// Returns the (memoized) inheritance-flattened element list of a named
    /// complex type. Collecting it walks the base chain and clones every
    /// `ElementDef`, far too expensive to repeat per instance element.
    pub(crate) fn collect_elements_cached(
        &mut self,
        type_name: &str,
    ) -> Option<Arc<Vec<ElementDef>>> {
        if let Some(cached) = self.elements_cache.get(type_name) {
            return Some(Arc::clone(cached));
        }
        let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_name) else {
            return None;
        };
        let collected = Arc::new(super::super::decls::collect_elements(&self.schema, complex));
        self.elements_cache
            .insert(type_name.to_string(), Arc::clone(&collected));
        Some(collected)
    }

    /// Namespace-aware variant of [`collect_elements_cached`]: resolves the
    /// type through its compile-time `(namespace, local)` identity (collision
    /// free), memoized in [`elements_cache_ns`]. Falls back to the
    /// namespace-blind string path when no `type_ns` is available, or when the
    /// ns-qualified lookup misses (leniency: an unresolvable reference stays
    /// resolvable through the legacy string map).
    ///
    /// [`elements_cache_ns`]: OnePassSchemaValidator::elements_cache_ns
    pub(crate) fn collect_elements_cached_ns(
        &mut self,
        type_ns: Option<&NsName>,
        type_ref: &str,
    ) -> Option<Arc<Vec<ElementDef>>> {
        if let Some(ns) = type_ns {
            if let Some(cached) = self.elements_cache_ns.get(ns) {
                return Some(Arc::clone(cached));
            }
            if let Some(TypeDef::Complex(complex)) =
                self.schema.type_ns(&ns.namespace_uri, &ns.local_name)
            {
                let collected =
                    Arc::new(super::super::decls::collect_elements(&self.schema, complex));
                self.elements_cache_ns
                    .insert(ns.clone(), Arc::clone(&collected));
                return Some(collected);
            }
        }
        // Fallback: namespace-blind string resolution.
        self.collect_elements_cached(type_ref)
    }

    /// Gets type information for an inline element from the parent's content model.
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
    /// `child_local_sym` is the child's local-name symbol, used (with the
    /// parent's type symbol) to memoize the resolution across identical
    /// (parent-type, child) pairs.
    pub(crate) fn get_inline_element_info(
        &mut self,
        child_local_sym: u32,
        name: &str,
    ) -> Arc<InlineResolved> {
        // For inline elements, we need to look up the parent's type and find the child element definition
        if self.state.element_stack.len() < 2 {
            return Arc::new(InlineResolved::default());
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_ctx = match self.state.element_stack.get(parent_idx) {
            Some(p) => p,
            None => return Arc::new(InlineResolved::default()),
        };

        // Use the parent's resolved type identity from its ElementContext
        // directly (settled during the parent's validation). This avoids
        // issues with prefixed element names (e.g. brid:BridgePart vs
        // BridgePart). Fast path: the resolution depends only on the parent
        // type and the child's (local) name, so memoize it by (parent type
        // sym, child sym). `parent_type_sym` is derived from the parent's
        // *namespace-resolved* type identity, so same-local-name parent types
        // in different namespaces key distinctly.
        if let (Some(parent_type_sym), Some(type_ref)) =
            (parent_ctx.type_sym, parent_ctx.type_ref.clone())
        {
            let parent_type_ns = parent_ctx.type_ns.clone();
            let key = (parent_type_sym, child_local_sym);
            if let Some(cached) = self.inline_cache.get(&key) {
                return Arc::clone(cached);
            }
            // Prefer the namespace-resolved parent type; the bare `type_ref`
            // string is a namespace-blind fallback (used only when the parent
            // carried no resolved `type_ns`).
            let resolved = match self.collect_elements_cached_ns(parent_type_ns.as_ref(), &type_ref)
            {
                Some(elements) => self.inline_info_from_elements(name, &elements),
                None => InlineResolved::default(),
            };
            // Debug-only: the memoized picture must equal a fresh resolution.
            #[cfg(test)]
            {
                let fresh =
                    match self.collect_elements_cached_ns(parent_type_ns.as_ref(), &type_ref) {
                        Some(elements) => self.inline_info_from_elements(name, &elements),
                        None => InlineResolved::default(),
                    };
                debug_assert_eq!(
                    resolved.type_ref.as_deref(),
                    fresh.type_ref.as_deref(),
                    "memoized inline type_ref for child {name:?} differs from fresh lookup"
                );
                debug_assert_eq!(
                    resolved
                        .flattened
                        .as_deref()
                        .map(|f| f.ordered_elements.clone()),
                    fresh
                        .flattened
                        .as_deref()
                        .map(|f| f.ordered_elements.clone()),
                    "memoized inline children for child {name:?} differ from fresh lookup"
                );
            }
            let resolved = Arc::new(resolved);
            self.inline_cache.insert(key, Arc::clone(&resolved));
            return resolved;
        }
        // The parent has an anonymous type (no named-type identity): resolve
        // the child against that type's own content model — never against a
        // same-named global element, which may be a different declaration.
        let Some(TypeDef::Complex(complex)) = parent_ctx.inline_type.clone() else {
            return Arc::new(InlineResolved::default());
        };
        let schema = Arc::clone(&self.schema);
        let elements = self.runtime_types.elements(&schema, &complex);
        Arc::new(self.inline_info_from_elements(name, &elements))
    }

    /// Finds `name` in an inherited-element list and resolves its type info
    /// together with its value constraints (`default`/`fixed`/`nillable`).
    fn inline_info_from_elements(&mut self, name: &str, elements: &[ElementDef]) -> InlineResolved {
        // Search from the end to prioritize derived type's elements over base type's
        // This is important when an element is redefined in a derived type with a different type
        // (e.g., brid:boundedBy in AbstractBridgeType shadows gml:boundedBy in AbstractFeatureType)
        for elem in elements.iter().rev() {
            if elem.name == name {
                // Found the inline element - get its type info
                let type_ref: Option<Arc<str>> = elem.type_ref.as_deref().map(Arc::from);
                let type_ns = elem.type_ns.clone();
                let inline_type = elem.inline_type.clone();

                // Get flattened children for this inline element: the
                // compile-time resolved type_ns probes the owning-namespace
                // cache first, then the legacy memoized string path.
                let flattened_children = if let Some(cached) = elem
                    .type_ns
                    .as_ref()
                    .and_then(|tn| self.schema.ns_type_children_cache.get(tn))
                {
                    Some(Arc::clone(cached))
                } else if let Some(ref tr) = type_ref {
                    self.resolve_children_for_type_ref(tr)
                } else if let Some(TypeDef::Complex(child_complex)) = elem.inline_type.as_ref() {
                    Some(self.runtime_children(child_complex))
                } else {
                    None
                };

                // Value constraints are resolved by name, but a content model
                // may declare the same name at multiple positions with
                // *differing* value constraints (e.g. a sequence of two
                // same-name elements with distinct `fixed` values, or a base
                // `default` shadowed by an extension `fixed`). Then the
                // constraint is positional: expose every declaration's
                // constraints in declaration order and let the caller pick by
                // the child's occurrence index. Type resolution keeps its
                // derived-first pick.
                let ambiguous = elements.iter().any(|e| {
                    e.name == name && (e.default != elem.default || e.fixed != elem.fixed)
                });
                let positional = if ambiguous {
                    Some(Arc::new(
                        elements
                            .iter()
                            .filter(|e| e.name == name)
                            .map(|e| ValueConstraint {
                                default: e.default.clone(),
                                fixed: e.fixed.clone(),
                                nillable: e.nillable,
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                let (default, fixed, nillable) = if ambiguous {
                    (None, None, false)
                } else {
                    (elem.default.clone(), elem.fixed.clone(), elem.nillable)
                };

                return InlineResolved {
                    type_ref,
                    type_ns,
                    flattened: flattened_children,
                    inline_type,
                    found: true,
                    is_ref: elem.ref_ns.is_some(),
                    constraints: elem.constraints.clone(),
                    default,
                    fixed,
                    nillable,
                    positional,
                };
            }
        }

        InlineResolved::default()
    }
}
