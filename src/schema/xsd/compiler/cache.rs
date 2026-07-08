//! Type children cache building for performance optimization.

use std::collections::HashSet;
use std::sync::Arc;

use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef, FlattenedChildren,
    NsName, Particle, TypeDef,
};

use super::XsdCompiler;

impl XsdCompiler {
    /// Builds the type children cache.
    ///
    /// This pre-computes the flattened child element constraints for each complex type,
    /// including elements inherited through type extension.
    ///
    /// The cache is `ns_type_children_cache`, keyed straight off `types_ns`,
    /// whose keys carry the OWNING document's target namespace recorded at
    /// registration time — collision-free and immune to the accumulated
    /// (last-document-wins) prefix bindings (errA002 class).
    pub(crate) fn build_type_children_cache(&self, schema: &mut CompiledSchema) {
        let ns_keys: Vec<NsName> = schema.types_ns.keys().cloned().collect();
        for ns_name in ns_keys {
            if let Some(TypeDef::Complex(complex)) = schema.types_ns.get(&ns_name) {
                let flattened = Arc::new(self.flatten_type_children_ns(complex, schema));
                schema.ns_type_children_cache.insert(ns_name, flattened);
            }
        }
    }

    /// Resolves a prefixed or unprefixed type/base-type key to NsName using namespace_bindings.
    fn resolve_to_ns(&self, key: &str) -> Option<NsName> {
        if let Some((prefix, local)) = key.split_once(':') {
            let ns_uri = self.namespace_bindings.get(prefix)?;
            Some(NsName::new(ns_uri.clone(), local))
        } else {
            let ns = self.current_target_ns.as_deref().unwrap_or("").to_string();
            Some(NsName::new(ns, key))
        }
    }

    /// Computes the inheritance-merged particle tree for a complex type:
    /// extensions append their own particle after the base chain's.
    ///
    /// `Ok(None)` means "no element content"; `Err(())` means the chain
    /// cannot be resolved faithfully (unknown base) so no automaton should
    /// be built.
    #[allow(clippy::result_unit_err)]
    fn effective_particle(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
        depth: usize,
    ) -> Result<Option<Particle>, ()> {
        if depth > 16 {
            return Err(());
        }
        use crate::schema::types::DerivationMethod;

        if complex.derivation == Some(DerivationMethod::Extension)
            && let Some(base_name) = &complex.base_type
        {
            // xs:anyType as base contributes nothing.
            let base_local = base_name
                .split_once(':')
                .map(|(_, l)| l)
                .unwrap_or(base_name);
            let base_part = if base_local == "anyType" {
                None
            } else {
                // C4: the type's own compile-time resolved base_ns first,
                // then the legacy accumulated-bindings resolution.
                let base = complex
                    .base_ns
                    .as_ref()
                    .and_then(|bn| schema.type_ns(&bn.namespace_uri, &bn.local_name))
                    .or_else(|| {
                        self.resolve_to_ns(base_name)
                            .and_then(|ns| schema.get_type_by_ns(&ns.namespace_uri, &ns.local_name))
                    })
                    .or_else(|| schema.get_type(base_name));
                match base {
                    Some(TypeDef::Complex(b)) => self.effective_particle(b, schema, depth + 1)?,
                    Some(_) => None, // simple base: no element content
                    None => return Err(()),
                }
            };
            let own = complex.particle.as_deref().cloned();
            return Ok(match (base_part, own) {
                (None, None) => None,
                (Some(b), None) => Some(b),
                (None, Some(o)) => Some(o),
                (Some(b), Some(o)) => Some(Particle::Sequence {
                    min: 1,
                    max: Some(1),
                    items: vec![b, o],
                }),
            });
        }

        // Restrictions and underived types: the own particle is the whole
        // content.
        Ok(complex.particle.as_deref().cloned())
    }

    /// Builds the content-model automaton for a complex type, if its
    /// content is automaton-friendly.
    fn build_type_automaton(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
    ) -> Option<crate::schema::xsd::content_automaton::ContentAutomaton> {
        let particle = self.effective_particle(complex, schema, 0).ok()??;
        let subst = |head: &str| -> Vec<String> {
            if let Some(members) = schema.transitive_substitution_groups.get(head) {
                return (**members).clone();
            }
            if let Some((_, local)) = head.split_once(':')
                && let Some(members) = schema.transitive_substitution_groups.get(local)
            {
                return (**members).clone();
            }
            Vec::new()
        };
        crate::schema::xsd::content_automaton::build_automaton(&particle, &subst)
    }

    /// Flattens the child element constraints for a complex type.
    /// Uses namespace-aware base type resolution.
    fn flatten_type_children_ns(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
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
        flattened.wildcard = inherited_wildcard(complex, schema);
        flattened.automaton = self.build_type_automaton(complex, schema).map(Arc::new);

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

                    // C4: the type's own compile-time resolved base_ns first
                    // (per owning document), then the legacy resolutions.
                    let base_complex = complex
                        .base_ns
                        .as_ref()
                        .and_then(|bn| schema.type_ns(&bn.namespace_uri, &bn.local_name))
                        .or_else(|| {
                            self.resolve_to_ns(base_type).and_then(|ns_name| {
                                schema.get_type_by_ns(&ns_name.namespace_uri, &ns_name.local_name)
                            })
                        });
                    // Fallback to legacy prefix-based lookup
                    let base_complex = base_complex.or_else(|| schema.get_type(base_type.as_str()));

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

/// Returns the complex type's element wildcard, inheriting from base types.
pub(crate) fn inherited_wildcard(
    complex: &crate::schema::types::ComplexType,
    schema: &CompiledSchema,
) -> Option<crate::schema::types::WildcardConstraint> {
    if complex.wildcard.is_some() {
        return complex.wildcard.clone();
    }
    // C4: ns-first base hops (compile-time resolved base_ns, string
    // fallback inside simple/complex base resolution).
    let mut current = complex;
    for _ in 0..16 {
        if current.base_type.is_none() {
            break;
        }
        let base = current
            .base_ns
            .as_ref()
            .and_then(|bn| schema.type_ns(&bn.namespace_uri, &bn.local_name))
            .or_else(|| {
                current
                    .base_type
                    .as_deref()
                    .and_then(|b| schema.get_type(b))
            });
        match base {
            Some(crate::schema::types::TypeDef::Complex(c)) => {
                if c.wildcard.is_some() {
                    return c.wildcard.clone();
                }
                current = c;
            }
            _ => break,
        }
    }
    None
}
