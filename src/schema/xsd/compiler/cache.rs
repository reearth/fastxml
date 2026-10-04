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

    /// Builds the content-model automaton for a complex type, if its
    /// content is automaton-friendly. Base types resolve through the type's
    /// compile-time `base_ns`, then the compiler's accumulated prefix
    /// bindings, then the legacy string lookup.
    fn build_type_automaton(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
    ) -> Option<crate::schema::xsd::content_automaton::ContentAutomaton> {
        let resolve_base = |c: &ComplexType, base_name: &str| -> Option<&TypeDef> {
            c.base_ns
                .as_ref()
                .and_then(|bn| schema.type_ns(&bn.namespace_uri, &bn.local_name))
                .or_else(|| {
                    self.resolve_to_ns(base_name)
                        .and_then(|ns| schema.get_type_by_ns(&ns.namespace_uri, &ns.local_name))
                })
                .or_else(|| schema.get_type(base_name))
        };
        build_type_automaton_with(complex, schema, &resolve_base)
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

        let mut flattened = FlattenedChildren::with_content_model(content_model_type(complex));
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

/// Computes the inheritance-merged particle tree for a complex type:
/// extensions append their own particle after the base chain's.
///
/// `Ok(None)` means "no element content"; `Err(())` means the chain cannot
/// be resolved faithfully (unknown base) so no automaton should be built.
fn effective_particle<'s>(
    complex: &ComplexType,
    resolve_base: &dyn Fn(&ComplexType, &str) -> Option<&'s TypeDef>,
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
            match resolve_base(complex, base_name) {
                Some(TypeDef::Complex(b)) => effective_particle(b, resolve_base, depth + 1)?,
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

/// Builds the content-model automaton for a complex type, if its content is
/// automaton-friendly. `resolve_base` maps a type and its base-type name to
/// the base definition. This is the single builder used both for the
/// compile-time named-type cache and for types the validators flatten at run
/// time (anonymous types, `xsi:type` substitutions).
pub(crate) fn build_type_automaton_with<'s>(
    complex: &ComplexType,
    schema: &'s CompiledSchema,
    resolve_base: &dyn Fn(&ComplexType, &str) -> Option<&'s TypeDef>,
) -> Option<crate::schema::xsd::content_automaton::ContentAutomaton> {
    let particle = effective_particle(complex, resolve_base, 0).ok()??;
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

/// Collects all child element declarations of a complex type, including
/// those inherited through extension, base first. Base types resolve ns-first
/// (compile-time `base_ns`) with the string lookup as fallback.
pub(crate) fn collect_elements_with_inheritance(
    complex: &ComplexType,
    schema: &CompiledSchema,
    visited: &mut HashSet<String>,
) -> Vec<ElementDef> {
    let mut elements = Vec::new();
    match &complex.content {
        ContentModel::Sequence(elems) | ContentModel::Choice(elems) | ContentModel::All(elems) => {
            elements.extend(elems.iter().cloned());
        }
        ContentModel::ComplexExtension {
            base_type,
            elements: ext_elements,
        } => {
            if !visited.contains(base_type.as_str()) {
                visited.insert(base_type.clone());
                // A no-namespace base whose local name collides with a type
                // in another (imported) namespace must not be resolved by
                // local name, hence the ns-first hop.
                if let Some(TypeDef::Complex(base_complex)) = schema.complex_base_def(complex) {
                    elements.extend(collect_elements_with_inheritance(
                        base_complex,
                        schema,
                        visited,
                    ));
                }
            }
            elements.extend(ext_elements.iter().cloned());
        }
        _ => {}
    }
    elements
}

/// Flattens a complex type's child-element constraints at validation time,
/// for types the compile-time cache does not cover (anonymous types, or a
/// named type reached only through a string key). Produces the same picture
/// as the compile-time cache, including the content-model automaton.
pub(crate) fn flatten_type_children(
    complex: &ComplexType,
    schema: &CompiledSchema,
) -> FlattenedChildren {
    let mut flattened = FlattenedChildren::with_content_model(content_model_type(complex));
    let mut visited = HashSet::new();
    let elements = collect_elements_with_inheritance(complex, schema, &mut visited);
    let mut ordered: Vec<String> = Vec::with_capacity(elements.len());
    for elem in &elements {
        flattened
            .constraints
            .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
        ordered.push(elem.name.clone());
    }
    flattened.ordered_elements = Arc::from(ordered);
    flattened.wildcard = inherited_wildcard(complex, schema);
    let resolve_base =
        |c: &ComplexType, base_name: &str| schema.type_by_ref(c.base_ns.as_ref(), base_name);
    flattened.automaton = build_type_automaton_with(complex, schema, &resolve_base).map(Arc::new);
    flattened
}

/// The coarse content-model kind of a complex type.
fn content_model_type(complex: &ComplexType) -> ContentModelType {
    match &complex.content {
        ContentModel::Sequence(_) => ContentModelType::Sequence,
        ContentModel::Choice(_) => ContentModelType::Choice,
        ContentModel::All(_) => ContentModelType::All,
        ContentModel::ComplexExtension { .. } => ContentModelType::Sequence,
        ContentModel::Empty => ContentModelType::Empty,
        ContentModel::SimpleContent { .. } => ContentModelType::Empty,
        ContentModel::Any { .. } => ContentModelType::Sequence,
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
