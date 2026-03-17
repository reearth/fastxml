//! Element lookup and type resolution for streaming validation.

use std::sync::Arc;

use crate::schema::types::{
    ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren, SimpleType, TypeDef,
};
use crate::schema::xsd::facets::FacetConstraints;

use super::OnePassSchemaValidator;

impl OnePassSchemaValidator {
    /// Optimized element lookup: tries qname first (when prefix present), then local name,
    /// then namespace URI.
    pub(crate) fn lookup_element_optimized(
        &self,
        name: &Arc<str>,
        prefix: Option<&Arc<str>>,
        namespace_uri: Option<&str>,
    ) -> Option<&ElementDef> {
        // If prefix exists, try qname FIRST to ensure correct namespace resolution.
        // This is critical when multiple namespaces define elements with the same local name
        // (e.g., bldg:WallSurface vs tun:WallSurface vs brid:WallSurface).
        if let Some(p) = prefix {
            if !p.is_empty() {
                let qname = format!("{}:{}", p.as_ref(), name.as_ref());
                if let Some(elem) = self.schema.get_element(&qname) {
                    return Some(elem);
                }
            }
        }

        // Try local name (for elements without prefix or as fallback)
        if let Some(elem) = self.schema.get_element(name.as_ref()) {
            return Some(elem);
        }

        // If namespace URI exists, try lookup by namespace URI + local name
        // This handles the case where XML uses different prefix than schema
        // (e.g., XML uses tr:Road but schema has tran:Road)
        if let Some(ns) = namespace_uri {
            if let Some(elem) = self.schema.get_element_by_ns(ns, name.as_ref()) {
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
        &self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // Try to get from type reference first
        if let Some(ref type_ref) = elem.type_ref {
            // Namespace-aware cache lookup
            if let Some(ns_name) = self.schema.resolve_type_ref_to_ns(type_ref) {
                if let Some(cached) = self.schema.ns_type_children_cache.get(&ns_name) {
                    return Some(Arc::clone(cached));
                }
            }

            // Fallback: compute at runtime
            if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        // Fall back to computing from inline type if present
        if let Some(ref inline_type) = elem.inline_type {
            if let TypeDef::Complex(complex) = inline_type {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
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

    /// Creates FacetConstraints from a SimpleType definition.
    pub(crate) fn create_facet_constraints(&self, simple: &SimpleType) -> FacetConstraints {
        let mut constraints = FacetConstraints::new();

        if let Some(min_len) = simple.min_length {
            constraints = constraints.with_min_length(min_len as usize);
        }
        if let Some(max_len) = simple.max_length {
            constraints = constraints.with_max_length(max_len as usize);
        }
        if let Some(ref min_inc) = simple.min_inclusive {
            constraints = constraints.with_min_inclusive(min_inc.clone());
        }
        if let Some(ref max_inc) = simple.max_inclusive {
            constraints = constraints.with_max_inclusive(max_inc.clone());
        }
        if !simple.enumeration.is_empty() {
            constraints = constraints.with_enumeration(simple.enumeration.clone());
        }
        if let Some(ref pattern) = simple.pattern {
            constraints = constraints.with_pattern(pattern.clone());
        }

        constraints
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

    /// Gets type information for an inline element from the parent's content model.
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
    pub(crate) fn get_inline_element_info(
        &self,
        name: &str,
    ) -> (Option<String>, Option<Arc<FlattenedChildren>>) {
        // For inline elements, we need to look up the parent's type and find the child element definition
        if self.state.element_stack.len() < 2 {
            return (None, None);
        }

        let parent_idx = self.state.element_stack.len() - 2;
        let parent_ctx = match self.state.element_stack.get(parent_idx) {
            Some(p) => p,
            None => return (None, None),
        };

        // Use parent's type_ref from ElementContext directly (already resolved during parent's validation)
        // This avoids issues with prefixed element names (e.g., brid:BridgePart vs BridgePart)
        let type_def = if let Some(ref type_ref) = parent_ctx.type_ref {
            self.schema.get_type(type_ref)
        } else {
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
            return (None, None);
        };

        // Collect all elements including inherited ones
        let mut visited = std::collections::HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, &mut visited);

        // Search from the end to prioritize derived type's elements over base type's
        // This is important when an element is redefined in a derived type with a different type
        // (e.g., brid:boundedBy in AbstractBridgeType shadows gml:boundedBy in AbstractFeatureType)
        for elem in elements.iter().rev() {
            if elem.name == name {
                // Found the inline element - get its type info
                let type_ref = elem.type_ref.clone();

                // Get flattened children for this inline element
                let flattened_children = if let Some(ref tr) = type_ref {
                    // Try namespace-aware cache first
                    if let Some(ns_name) = self.schema.resolve_type_ref_to_ns(tr) {
                        if let Some(cached) = self.schema.ns_type_children_cache.get(&ns_name) {
                            return (type_ref, Some(Arc::clone(cached)));
                        }
                    }
                    // Fallback: compute at runtime
                    if let Some(TypeDef::Complex(child_complex)) = self.schema.get_type(tr) {
                        Some(Arc::new(self.compute_flattened_children(child_complex)))
                    } else {
                        None
                    }
                } else if let Some(ref inline) = elem.inline_type {
                    if let TypeDef::Complex(child_complex) = inline {
                        Some(Arc::new(self.compute_flattened_children(child_complex)))
                    } else {
                        None
                    }
                } else {
                    None
                };

                return (type_ref, flattened_children);
            }
        }

        (None, None)
    }

    /// Gets inline type definition for an element (either global or from parent's content model).
    ///
    /// This searches through inherited elements as well when the parent type uses ComplexExtension.
    pub(crate) fn get_element_inline_type(&self, name: &str) -> Option<TypeDef> {
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
        let parent_name = &self.state.element_stack.get(parent_idx)?.name;

        let parent_elem = self.schema.get_element(parent_name.as_ref())?;
        let type_def = if let Some(ref type_ref) = parent_elem.type_ref {
            self.schema.get_type(type_ref)?
        } else {
            parent_elem.inline_type.as_ref()?
        };

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
