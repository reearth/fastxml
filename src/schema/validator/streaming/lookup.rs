//! Element lookup and type resolution for streaming validation.

use std::sync::Arc;

use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    SimpleType, TypeDef,
};
use crate::schema::xsd::facets::FacetConstraints;

use super::OnePassSchemaValidator;

/// Memoized resolution of a child element within a parent's content model
/// (see [`OnePassSchemaValidator::get_inline_element_info`]).
pub(crate) struct InlineResolved {
    /// The child's declared named type, shared as an `Arc<str>`.
    pub type_ref: Option<Arc<str>>,
    /// The child's flattened children (for its own content model).
    pub flattened: Option<Arc<FlattenedChildren>>,
    /// The child's anonymous inline type, if any.
    pub inline_type: Option<TypeDef>,
}

impl OnePassSchemaValidator {
    /// Optimized element lookup: tries qname first (when prefix present), then local name,
    /// then namespace URI.
    pub(crate) fn lookup_element_optimized<'s>(
        &self,
        schema: &'s CompiledSchema,
        name: &Arc<str>,
        prefix: Option<&str>,
        qname: &str,
        namespace_uri: Option<&str>,
    ) -> Option<&'s ElementDef> {
        // C3: `schema` is a locally-held clone of `self.schema`'s Arc, so the
        // returned &ElementDef is decoupled from `self` and can be held across
        // &mut self calls — letting the caller pass element constraints as a
        // slice instead of cloning them per element.
        //
        // C4: collision-free namespace-qualified lookup FIRST. The instance
        // element's namespace was resolved from its in-scope declarations
        // (state.resolve_element_namespace), so `(namespace, local)` is the
        // authoritative identity — prefix spelling differences and bare-key
        // collisions between same-local-name globals in different namespaces
        // (wildG031 class) cannot mislead it. `None` means the element is in
        // no namespace, so probe the "" key. The legacy string paths below
        // remain as fallback for schemas whose components were registered
        // under shapes the ns map does not cover.
        match namespace_uri {
            Some(ns) => {
                if let Some(elem) = schema.element_ns(ns, name.as_ref()) {
                    return Some(elem);
                }
            }
            None => {
                if let Some(elem) = schema.element_ns("", name.as_ref()) {
                    return Some(elem);
                }
            }
        }

        // If prefix exists, try qname FIRST to ensure correct namespace resolution.
        // This is critical when multiple namespaces define elements with the same local name
        // (e.g., bldg:WallSurface vs tun:WallSurface vs brid:WallSurface).
        // C1: `qname` is the interned qualified name threaded from the tag
        // boundary, so no `format!` is needed here.
        if let Some(p) = prefix {
            if !p.is_empty() {
                if let Some(elem) = schema.get_element(qname) {
                    return Some(elem);
                }
            }
        }

        // Try local name (for elements without prefix or as fallback)
        if let Some(elem) = schema.get_element(name.as_ref()) {
            return Some(elem);
        }

        // If namespace URI exists, try lookup by namespace URI + local name
        // This handles the case where XML uses different prefix than schema
        // (e.g., XML uses tr:Road but schema has tran:Road)
        if let Some(ns) = namespace_uri {
            if let Some(elem) = schema.get_element_by_ns(ns, name.as_ref()) {
                return Some(elem);
            }
        }

        None
    }

    /// Gets the pre-computed flattened children for an element from the schema cache.
    ///
    /// This uses the namespace-aware `ns_type_children_cache` as the primary lookup,
    /// which uses (namespace_uri, local_name) keys to avoid cross-namespace collisions.
    /// Falls back to runtime computation if not cached.
    pub(crate) fn get_flattened_children_for_element(
        &mut self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // C4: the compile-time resolved (namespace, local) of the type
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

        // Fall back to computing from inline type if present
        if let Some(ref inline_type) = elem.inline_type {
            if let TypeDef::Complex(complex) = inline_type {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        None
    }

    /// Resolves (and memoizes) the [`FlattenedChildren`] for a named type
    /// reference. Replays the previous per-element resolution verbatim — the
    /// namespace-aware `ns_type_children_cache` first, then a runtime compute
    /// — but caches the result so the `NsName` allocation and the cache probe
    /// happen once per distinct type instead of once per element.
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
    fn compute_children_for_type_ref(&self, type_ref: &str) -> Option<Arc<FlattenedChildren>> {
        // Namespace-aware cache lookup first.
        if let Some(ns_name) = self.schema.resolve_type_ref_to_ns(type_ref) {
            if let Some(cached) = self.schema.ns_type_children_cache.get(&ns_name) {
                return Some(Arc::clone(cached));
            }
        }
        // Fallback: compute at runtime.
        if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
            return Some(Arc::new(self.compute_flattened_children(complex)));
        }
        None
    }

    /// Computes flattened children for inline types (fallback when not in cache).
    pub(crate) fn compute_flattened_children(&self, complex: &ComplexType) -> FlattenedChildren {
        let content_model_type = match &complex.content {
            ContentModel::Sequence(_) => ContentModelType::Sequence,
            ContentModel::Choice(_) => ContentModelType::Choice,
            ContentModel::All(_) => ContentModelType::All,
            ContentModel::ComplexExtension { .. } => ContentModelType::Sequence,
            ContentModel::Empty => ContentModelType::Empty,
            ContentModel::SimpleContent { .. } => ContentModelType::Empty,
            ContentModel::Any { .. } => ContentModelType::Sequence,
        };

        let mut flattened = FlattenedChildren::with_content_model(content_model_type);

        // Collect elements from content model
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        // Collect ordered elements into a temporary Vec, then convert to Arc<[String]>
        let mut ordered: Vec<String> = Vec::with_capacity(elements.len());
        for elem in &elements {
            flattened
                .constraints
                .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
            // Store element order for sequence validation
            ordered.push(elem.name.clone());
        }
        flattened.ordered_elements = Arc::from(ordered);
        flattened.wildcard =
            crate::schema::xsd::compiler::inherited_wildcard(complex, &self.schema);

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
    /// (Used only as fallback for inline types not in cache)
    pub(crate) fn collect_elements_with_inheritance(
        &self,
        complex: &ComplexType,
        visited: &mut std::collections::HashSet<String>,
    ) -> Vec<ElementDef> {
        let mut elements = Vec::new();

        match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => {
                elements.extend(elems.iter().cloned());
            }
            ContentModel::ComplexExtension {
                base_type,
                elements: ext_elements,
            } => {
                if !visited.contains(base_type.as_str()) {
                    visited.insert(base_type.clone());
                    if let Some(TypeDef::Complex(base_complex)) =
                        self.schema.get_type(base_type.as_str())
                    {
                        let base_elements =
                            self.collect_elements_with_inheritance(base_complex, visited);
                        elements.extend(base_elements);
                    }
                }
                elements.extend(ext_elements.iter().cloned());
            }
            _ => {}
        }

        elements
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
        let mut visited = std::collections::HashSet::new();
        let collected = Arc::new(self.collect_elements_with_inheritance(complex, &mut visited));
        self.elements_cache
            .insert(type_name.to_string(), Arc::clone(&collected));
        Some(collected)
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
    ) -> (
        Option<Arc<str>>,
        Option<Arc<FlattenedChildren>>,
        Option<TypeDef>,
    ) {
        // For inline elements, we need to look up the parent's type and find the child element definition
        if self.state.element_stack.len() < 2 {
            return (None, None, None);
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_ctx = match self.state.element_stack.get(parent_idx) {
            Some(p) => p,
            None => return (None, None, None),
        };

        // Use parent's type_ref from ElementContext directly (already resolved during parent's validation)
        // This avoids issues with prefixed element names (e.g., brid:BridgePart vs BridgePart)
        // Fast path: the resolution depends only on the parent type and the
        // child's (local) name, so memoize it by (parent type sym, child sym).
        if let (Some(parent_type_sym), Some(type_ref)) =
            (parent_ctx.type_sym, parent_ctx.type_ref.clone())
        {
            let key = (parent_type_sym, child_local_sym);
            if let Some(cached) = self.inline_cache.get(&key) {
                return (
                    cached.type_ref.clone(),
                    cached.flattened.clone(),
                    cached.inline_type.clone(),
                );
            }
            let resolved = match self.collect_elements_cached(&type_ref) {
                Some(elements) => {
                    let (tr, fl, it) = self.inline_info_from_elements(name, &elements);
                    InlineResolved {
                        type_ref: tr,
                        flattened: fl,
                        inline_type: it,
                    }
                }
                None => InlineResolved {
                    type_ref: None,
                    flattened: None,
                    inline_type: None,
                },
            };
            // Debug-only: the memoized picture must equal a fresh resolution.
            #[cfg(test)]
            {
                let fresh = match self.collect_elements_cached(&type_ref) {
                    Some(elements) => self.inline_info_from_elements(name, &elements),
                    None => (None, None, None),
                };
                debug_assert_eq!(
                    resolved.type_ref.as_deref(),
                    fresh.0.as_deref(),
                    "memoized inline type_ref for child {name:?} differs from fresh lookup"
                );
                debug_assert_eq!(
                    resolved
                        .flattened
                        .as_deref()
                        .map(|f| f.ordered_elements.clone()),
                    fresh.1.as_deref().map(|f| f.ordered_elements.clone()),
                    "memoized inline children for child {name:?} differ from fresh lookup"
                );
            }
            let resolved = Arc::new(resolved);
            self.inline_cache.insert(key, Arc::clone(&resolved));
            return (
                resolved.type_ref.clone(),
                resolved.flattened.clone(),
                resolved.inline_type.clone(),
            );
        }
        let type_def = {
            // Fallback: try to look up parent element from schema
            let parent_name = &parent_ctx.name;
            let parent_elem = self.schema.get_element(parent_name.as_ref());
            if let Some(elem) = parent_elem {
                if let Some(ref type_ref) = elem.type_ref {
                    self.schema.get_type(type_ref)
                } else {
                    elem.inline_type.as_ref()
                }
            } else {
                // Try without prefix
                let local_name = parent_name
                    .split(':')
                    .next_back()
                    .unwrap_or(parent_name.as_ref());
                if let Some(elem) = self.schema.get_element(local_name) {
                    if let Some(ref type_ref) = elem.type_ref {
                        self.schema.get_type(type_ref)
                    } else {
                        elem.inline_type.as_ref()
                    }
                } else {
                    None
                }
            }
        };

        let Some(TypeDef::Complex(complex)) = type_def else {
            return (None, None, None);
        };

        // Collect all elements including inherited ones
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);
        self.inline_info_from_elements(name, &elements)
    }

    /// Finds `name` in an inherited-element list and resolves its type info.
    fn inline_info_from_elements(
        &mut self,
        name: &str,
        elements: &[ElementDef],
    ) -> (
        Option<Arc<str>>,
        Option<Arc<FlattenedChildren>>,
        Option<TypeDef>,
    ) {
        // Search from the end to prioritize derived type's elements over base type's
        // This is important when an element is redefined in a derived type with a different type
        // (e.g., brid:boundedBy in AbstractBridgeType shadows gml:boundedBy in AbstractFeatureType)
        for elem in elements.iter().rev() {
            if elem.name == name {
                // Found the inline element - get its type info
                let type_ref: Option<Arc<str>> = elem.type_ref.as_deref().map(Arc::from);
                let inline_type = elem.inline_type.clone();

                // Get flattened children for this inline element: the
                // compile-time resolved type_ns probes the owning-namespace
                // cache first (C4), then the legacy memoized string path.
                let flattened_children = if let Some(cached) = elem
                    .type_ns
                    .as_ref()
                    .and_then(|tn| self.schema.ns_type_children_cache.get(tn))
                {
                    Some(Arc::clone(cached))
                } else if let Some(ref tr) = type_ref {
                    self.resolve_children_for_type_ref(tr)
                } else if let Some(TypeDef::Complex(child_complex)) = elem.inline_type.as_ref() {
                    Some(Arc::new(self.compute_flattened_children(child_complex)))
                } else {
                    None
                };

                return (type_ref, flattened_children, inline_type);
            }
        }

        (None, None, None)
    }

    /// Gets inline type definition for an element (either global or from parent's content model).
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
    pub(crate) fn get_element_inline_type(&mut self, name: &str) -> Option<TypeDef> {
        // First try global element
        if let Some(elem) = self.schema.get_element(name) {
            if let Some(ref inline) = elem.inline_type {
                return Some(inline.clone());
            }
        }

        // Try to find inline type from parent's content model
        if self.state.element_stack.len() < 2 {
            return None;
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_name = self.state.element_stack.get(parent_idx)?.name.clone();

        let parent_elem = self.schema.get_element(parent_name.as_ref())?;
        // Fast path: memoized inherited-element list by parent type name.
        if let Some(type_ref) = parent_elem.type_ref.clone() {
            let elements = self.collect_elements_cached(&type_ref)?;
            return elements
                .iter()
                .rev()
                .find(|e| e.name == name)
                .and_then(|e| e.inline_type.clone());
        }
        let type_def = parent_elem.inline_type.as_ref()?;

        let TypeDef::Complex(complex) = type_def else {
            return None;
        };

        // Collect all elements including inherited ones
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        // Search from the end to prioritize derived type's elements over base type's
        for elem in elements.iter().rev() {
            if elem.name == name {
                return elem.inline_type.clone();
            }
        }

        None
    }
}
