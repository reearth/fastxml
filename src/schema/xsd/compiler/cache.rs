//! Type children cache building for performance optimization.

use std::collections::HashSet;
use std::sync::Arc;

use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    NsName, TypeDef,
};

use super::XsdCompiler;

impl XsdCompiler {
    /// Builds the type children cache.
    ///
    /// This pre-computes the flattened child element constraints for each complex type,
    /// including elements inherited through type extension.
    ///
    /// The primary cache is `ns_type_children_cache` keyed by (namespace_uri, local_name),
    /// which is collision-free. The legacy `type_children_cache` (keyed by "prefix:local")
    /// is also populated for backward compatibility.
    pub(crate) fn build_type_children_cache(&self, schema: &mut CompiledSchema) {
        // Collect type names first to avoid borrowing issues
        let type_names: Vec<String> = schema.types.keys().cloned().collect();

        // Build cache for main schema types
        for type_name in &type_names {
            if let Some(TypeDef::Complex(complex)) = schema.types.get(type_name) {
                let flattened =
                    Arc::new(self.flatten_type_children_ns(complex, schema, type_name));

                // --- Namespace-aware cache (primary) ---
                if let Some(ns_name) = self.resolve_type_key_to_ns(type_name) {
                    schema
                        .ns_type_children_cache
                        .insert(ns_name, Arc::clone(&flattened));
                }

                // --- Legacy prefix-based cache ---
                schema
                    .type_children_cache
                    .insert(type_name.clone(), Arc::clone(&flattened));

                // Also insert with local name for fallback lookup (first-wins).
                let local_name = type_name
                    .split_once(':')
                    .map(|(_, local)| local)
                    .unwrap_or(type_name);
                schema
                    .type_children_cache
                    .entry(local_name.to_string())
                    .or_insert(Arc::clone(&flattened));
            }
        }

        // Build cache for imported schema types
        let import_types: Vec<(String, FlattenedChildren)> = schema
            .imports
            .values()
            .flat_map(|imported| {
                imported.types.iter().filter_map(|(type_name, type_def)| {
                    if let TypeDef::Complex(complex) = type_def {
                        let flattened = self.flatten_type_children_ns(complex, schema, type_name);
                        Some((type_name.clone(), flattened))
                    } else {
                        None
                    }
                })
            })
            .collect();

        for (type_name, flattened) in import_types {
            let flattened = Arc::new(flattened);

            // --- Namespace-aware cache ---
            if let Some(ns_name) = self.resolve_type_key_to_ns(&type_name) {
                schema
                    .ns_type_children_cache
                    .insert(ns_name, Arc::clone(&flattened));
            }

            // --- Legacy prefix-based cache ---
            schema
                .type_children_cache
                .insert(type_name.clone(), Arc::clone(&flattened));

            let local_name = type_name
                .split_once(':')
                .map(|(_, local)| local)
                .unwrap_or(&type_name);
            schema
                .type_children_cache
                .entry(local_name.to_string())
                .or_insert(Arc::clone(&flattened));
        }
    }

    /// Resolves a type key like "bldg:WallSurfaceType" to NsName using namespace_bindings.
    fn resolve_type_key_to_ns(&self, type_key: &str) -> Option<NsName> {
        if let Some((prefix, local)) = type_key.split_once(':') {
            let ns_uri = self.namespace_bindings.get(prefix)?;
            Some(NsName::new(ns_uri.clone(), local))
        } else {
            // No prefix - try current target namespace, then empty namespace
            let ns = self
                .current_target_ns
                .as_deref()
                .unwrap_or("")
                .to_string();
            Some(NsName::new(ns, type_key))
        }
    }

    /// Resolves a base_type reference to a namespace URI and local name.
    /// Used during flattening to find the correct base type across namespaces.
    fn resolve_base_type_ns(&self, base_type: &str) -> Option<NsName> {
        if let Some((prefix, local)) = base_type.split_once(':') {
            let ns_uri = self.namespace_bindings.get(prefix)?;
            Some(NsName::new(ns_uri.clone(), local))
        } else {
            // No prefix - could be from the current target namespace
            if let Some(ref ns) = self.current_target_ns {
                Some(NsName::new(ns.clone(), base_type))
            } else {
                Some(NsName::new("", base_type))
            }
        }
    }

    /// Looks up a type by namespace URI and local name.
    fn get_type_by_ns<'a>(
        &self,
        schema: &'a CompiledSchema,
        ns_name: &NsName,
    ) -> Option<&'a TypeDef> {
        // Use namespace_prefixes (uri → prefix) to construct the qualified key
        if let Some(prefix) = schema.namespace_prefixes.get(&ns_name.namespace_uri) {
            let qname = if prefix.is_empty() {
                ns_name.local_name.clone()
            } else {
                format!("{}:{}", prefix, ns_name.local_name)
            };
            if let Some(typ) = schema.types.get(&qname) {
                return Some(typ);
            }
        }
        // Fallback: try local name directly
        schema.types.get(&ns_name.local_name)
    }

    /// Flattens the child element constraints for a complex type.
    /// Uses namespace-aware base type resolution.
    fn flatten_type_children_ns(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
        _type_key: &str,
    ) -> FlattenedChildren {
        let mut visited = HashSet::new();
        let elements = self.collect_elements_with_inheritance_ns(complex, schema, &mut visited);

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
        for elem in elements {
            flattened
                .constraints
                .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
        }

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
    /// Uses namespace-aware base type resolution to avoid cross-namespace collisions.
    fn collect_elements_with_inheritance_ns(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
        visited: &mut HashSet<String>,
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
                // First, get elements from the base type (inherited elements)
                if !visited.contains(base_type.as_str()) {
                    visited.insert(base_type.clone());

                    // Resolve base type using namespace URI for correct cross-namespace lookup
                    let base_complex =
                        if let Some(ns_name) = self.resolve_base_type_ns(base_type) {
                            self.get_type_by_ns(schema, &ns_name)
                        } else {
                            None
                        };
                    // Fallback to legacy prefix-based lookup
                    let base_complex =
                        base_complex.or_else(|| schema.get_type(base_type.as_str()));

                    if let Some(TypeDef::Complex(base_complex)) = base_complex {
                        let base_elements = self.collect_elements_with_inheritance_ns(
                            base_complex,
                            schema,
                            visited,
                        );
                        elements.extend(base_elements);
                    }
                }
                // Then add the extension's own elements
                elements.extend(ext_elements.iter().cloned());
            }
            _ => {}
        }

        elements
    }
}
