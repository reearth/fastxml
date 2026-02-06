//! Type children cache building for performance optimization.

use std::collections::HashSet;
use std::sync::Arc;

use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    TypeDef,
};

use super::XsdCompiler;

impl XsdCompiler {
    /// Builds the type children cache.
    ///
    /// This pre-computes the flattened child element constraints for each complex type,
    /// including elements inherited through type extension.
    ///
    /// Types are stored with both their local name AND qualified names (with common prefixes)
    /// to ensure fast cache hits regardless of how the type is referenced.
    pub(crate) fn build_type_children_cache(&self, schema: &mut CompiledSchema) {
        // Collect type names first to avoid borrowing issues
        let type_names: Vec<String> = schema.types.keys().cloned().collect();

        // Build cache for main schema types
        for type_name in type_names {
            if let Some(TypeDef::Complex(complex)) = schema.types.get(&type_name) {
                let flattened = Arc::new(self.flatten_type_children(complex, schema));

                // Insert with the original key (may have prefix like "tran:RoadType")
                schema
                    .type_children_cache
                    .insert(type_name.clone(), Arc::clone(&flattened));

                // Extract local name (strip existing prefix if present)
                let local_name = type_name
                    .split_once(':')
                    .map(|(_, local)| local)
                    .unwrap_or(&type_name);

                // Insert with just the local name for fallback lookup
                schema
                    .type_children_cache
                    .insert(local_name.to_string(), Arc::clone(&flattened));

                // Also insert with common namespace prefixes to avoid split_once at runtime
                // Use the local_name to avoid double prefixes like "gml:tran:RoadType"
                for prefix in &[
                    "gml", "core", "xs", "xsd", "bldg", "dem", "tran", "urf", "luse", "fld", "uro",
                    "gen",
                ] {
                    let qualified = format!("{}:{}", prefix, local_name);
                    schema
                        .type_children_cache
                        .insert(qualified, Arc::clone(&flattened));
                }
            }
        }

        // Build cache for imported schema types
        // We need to collect these separately and then add them
        let import_types: Vec<(String, FlattenedChildren)> = schema
            .imports
            .values()
            .flat_map(|imported| {
                imported.types.iter().filter_map(|(type_name, type_def)| {
                    if let TypeDef::Complex(complex) = type_def {
                        let flattened = self.flatten_type_children(complex, schema);
                        Some((type_name.clone(), flattened))
                    } else {
                        None
                    }
                })
            })
            .collect();

        for (type_name, flattened) in import_types {
            let flattened = Arc::new(flattened);
            schema
                .type_children_cache
                .insert(type_name.clone(), Arc::clone(&flattened));

            // Extract local name (strip existing prefix if present)
            let local_name = type_name
                .split_once(':')
                .map(|(_, local)| local)
                .unwrap_or(&type_name);

            // Insert with just the local name for fallback lookup
            schema
                .type_children_cache
                .insert(local_name.to_string(), Arc::clone(&flattened));

            // Also insert with common namespace prefixes
            // Use the local_name to avoid double prefixes
            for prefix in &[
                "gml", "core", "xs", "xsd", "bldg", "dem", "tran", "urf", "luse", "fld", "uro",
                "gen",
            ] {
                let qualified = format!("{}:{}", prefix, local_name);
                schema
                    .type_children_cache
                    .insert(qualified, Arc::clone(&flattened));
            }
        }
    }

    /// Flattens the child element constraints for a complex type.
    fn flatten_type_children(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
    ) -> FlattenedChildren {
        let mut visited = HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, schema, &mut visited);

        // Determine content model type
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
    fn collect_elements_with_inheritance(
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
                    if let Some(TypeDef::Complex(base_complex)) =
                        schema.get_type(base_type.as_str())
                    {
                        let base_elements =
                            self.collect_elements_with_inheritance(base_complex, schema, visited);
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
