//! Element lookup and type resolution for DOM validation.

use std::sync::Arc;

use crate::schema::types::{
    ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren, TypeDef,
};

use super::DomSchemaValidator;

impl DomSchemaValidator {
    /// Looks up an element definition in the schema.
    pub(crate) fn lookup_element(&self, name: &str, prefix: Option<&str>) -> Option<&ElementDef> {
        // Try local name first
        if let Some(elem) = self.schema.get_element(name) {
            return Some(elem);
        }

        // Try with prefix
        if let Some(p) = prefix {
            if !p.is_empty() {
                let qname = format!("{}:{}", p, name);
                if let Some(elem) = self.schema.get_element(&qname) {
                    return Some(elem);
                }
            }
        }

        None
    }

    /// Gets flattened children for an element from the schema cache.
    pub(crate) fn get_flattened_children_for_element(
        &self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // Try type reference first
        if let Some(ref type_ref) = elem.type_ref {
            if let Some(cached) = self.schema.type_children_cache.get(type_ref) {
                return Some(Arc::clone(cached));
            }

            // Try without prefix
            if let Some((_prefix, local)) = type_ref.split_once(':') {
                if let Some(cached) = self.schema.type_children_cache.get(local) {
                    return Some(Arc::clone(cached));
                }
            }

            // Compute at runtime if not cached
            if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        // Try inline type
        if let Some(ref inline_type) = elem.inline_type {
            if let TypeDef::Complex(complex) = inline_type {
                return Some(Arc::new(self.compute_flattened_children(complex)));
            }
        }

        None
    }

    /// Computes flattened children for a complex type.
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
        flattened.ordered_elements = std::sync::Arc::from(ordered);

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
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
}
