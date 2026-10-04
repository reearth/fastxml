//! Element lookup and type resolution for DOM validation.

use std::sync::Arc;

use crate::schema::types::{ComplexType, ElementDef, FlattenedChildren, TypeDef};

use super::DomSchemaValidator;

impl DomSchemaValidator {
    /// Looks up the global element declaration matching an instance
    /// element's expanded name (see [`ElementResolver::lookup`]).
    ///
    /// [`ElementResolver::lookup`]: super::super::decls::ElementResolver::lookup
    pub(crate) fn lookup_element(
        &self,
        name: &str,
        prefix: Option<&str>,
        namespace_uri: Option<&str>,
    ) -> Option<&ElementDef> {
        let qname = match prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, name),
            _ => name.to_string(),
        };
        self.resolver
            .lookup(&self.schema, name, prefix, &qname, namespace_uri)
    }

    /// Gets flattened children for an element from the schema cache.
    pub(crate) fn get_flattened_children_for_element(
        &self,
        elem: &ElementDef,
    ) -> Option<Arc<FlattenedChildren>> {
        // the compile-time resolved (namespace, local) of the type
        // reference probes the owning-namespace-keyed cache directly.
        if let Some(ref type_ns) = elem.type_ns
            && let Some(cached) = self.schema.ns_type_children_cache.get(type_ns)
        {
            return Some(Arc::clone(cached));
        }
        // String resolution of the reference into the ns cache.
        if let Some(ref type_ref) = elem.type_ref {
            if let Some(ns_name) = self.schema.resolve_type_ref_to_ns(type_ref)
                && let Some(cached) = self.schema.ns_type_children_cache.get(&ns_name)
            {
                return Some(Arc::clone(cached));
            }
            if let Some(TypeDef::Complex(complex)) = self.schema.get_type(type_ref) {
                return Some(self.runtime_children(complex));
            }
        }

        // Anonymous type
        if let Some(TypeDef::Complex(complex)) = &elem.inline_type {
            return Some(self.runtime_children(complex));
        }

        None
    }

    /// Flattened children (with content-model automaton) of a type outside
    /// the compile-time cache, memoized per type.
    pub(crate) fn runtime_children(&self, complex: &ComplexType) -> Arc<FlattenedChildren> {
        self.runtime_types
            .borrow_mut()
            .children(&self.schema, complex)
    }
}
