//! Declaration resolution shared by the DOM and streaming validators: global
//! element lookup by expanded name, the "does this schema declare anything"
//! test that gates undeclared-element errors, and the run-time flattening of
//! complex types the compile-time cache does not cover.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use crate::schema::types::{
    CompiledSchema, ComplexType, DerivationMethod, ElementDef, FlattenedChildren, NsName, NsNameRef,
};

/// Global-element resolution for one schema.
///
/// An instance element is matched to a global declaration by its expanded
/// name `(namespace, local)`. The only fallback is the chameleon-include one:
/// an element in a namespace the schema defines components in may match a
/// component registered under no namespace (a chameleon document merged into
/// that namespace). An element in a namespace the schema knows nothing about
/// never matches a declaration by local name alone.
pub(crate) struct ElementResolver {
    /// Namespaces the schema defines at least one component in.
    known_namespaces: HashSet<Arc<str>>,
    /// Whether undeclared elements are reported (see [`declares_components`]).
    pub(crate) reports_undeclared: bool,
}

impl ElementResolver {
    pub(crate) fn new(schema: &CompiledSchema) -> Self {
        let mut known_namespaces: HashSet<Arc<str>> = HashSet::new();
        let keys = schema
            .elements_ns
            .keys()
            .chain(schema.types_ns.keys())
            .chain(schema.attributes_ns.keys());
        for key in keys {
            if !key.namespace_uri.is_empty() && !known_namespaces.contains(&key.namespace_uri) {
                known_namespaces.insert(Arc::clone(&key.namespace_uri));
            }
        }
        if let Some(tns) = schema.target_namespace.as_deref()
            && !tns.is_empty()
        {
            known_namespaces.insert(Arc::from(tns));
        }
        Self {
            known_namespaces,
            reports_undeclared: declares_components(schema),
        }
    }

    /// Looks up the global element declaration for an instance element.
    ///
    /// `namespace` is the element's namespace URI resolved from its in-scope
    /// declarations (`None` = no namespace). `prefix`/`qname` are consulted
    /// only when the element carries a prefix that did not resolve to any
    /// namespace, where the expanded name is unknown and the legacy
    /// string-keyed lookup is the best available guess.
    pub(crate) fn lookup<'s>(
        &self,
        schema: &'s CompiledSchema,
        local: &str,
        prefix: Option<&str>,
        qname: &str,
        namespace: Option<&str>,
    ) -> Option<&'s ElementDef> {
        let probe = |namespace_uri: &str| {
            schema.elements_ns.get(&NsNameRef {
                namespace_uri,
                local_name: local,
            })
        };
        match namespace {
            Some(ns) => probe(ns).or_else(|| {
                // Chameleon include: a no-namespace component adopted into
                // a namespace the schema defines.
                if self.known_namespaces.contains(ns) {
                    probe("")
                } else {
                    None
                }
            }),
            None => match prefix {
                Some(p) if !p.is_empty() => schema.get_element(qname),
                _ => probe(""),
            },
        }
    }
}

/// Whether the schema declares any component of its own beyond the built-in
/// XSD/GML types every compiled schema carries.
///
/// A schema that does is a user schema, and an element it does not declare
/// is an error in strict mode — even when the schema declares only types.
/// A schema with only the built-in types is what auto-detection falls back
/// to when no schema could be loaded; it has no element declarations to
/// check against, so undeclared elements are not reported for it.
fn declares_components(schema: &CompiledSchema) -> bool {
    static BUILTIN_TYPES: OnceLock<HashSet<NsName>> = OnceLock::new();
    if !schema.elements_ns.is_empty() || !schema.attributes_ns.is_empty() {
        return true;
    }
    let builtin = BUILTIN_TYPES.get_or_init(|| {
        crate::schema::xsd::create_builtin_schema()
            .types_ns
            .keys()
            .cloned()
            .collect()
    });
    schema.types_ns.keys().any(|k| !builtin.contains(k))
}

/// Memoized run-time flattening of complex types that are not in the
/// compile-time `ns_type_children_cache` (anonymous types, and named types
/// reached only through a string key).
///
/// Entries are keyed by the address of the type's `particle` `Arc`: clones of
/// an `ElementDef` / `ComplexType` share that `Arc`, and the schema keeps the
/// original alive for the validator's lifetime, so the address identifies the
/// content model. The base reference is stored alongside and compared on
/// every hit, so a key collision can never hand out another type's picture.
/// Types without a particle are flattened afresh each time.
#[derive(Default)]
pub(crate) struct RuntimeTypeCache {
    entries: rustc_hash::FxHashMap<usize, RuntimeEntry>,
}

struct RuntimeEntry {
    base_type: Option<String>,
    derivation: Option<DerivationMethod>,
    children: Arc<FlattenedChildren>,
    elements: Arc<Vec<ElementDef>>,
}

impl RuntimeTypeCache {
    fn entry(&mut self, schema: &CompiledSchema, complex: &ComplexType) -> Option<&RuntimeEntry> {
        let key = complex.particle.as_ref().map(|p| Arc::as_ptr(p) as usize)?;
        let fresh = |complex: &ComplexType| RuntimeEntry {
            base_type: complex.base_type.clone(),
            derivation: complex.derivation,
            children: Arc::new(crate::schema::xsd::compiler::flatten_type_children(
                complex, schema,
            )),
            elements: Arc::new(collect_elements(schema, complex)),
        };
        let matches = |e: &RuntimeEntry| {
            e.base_type == complex.base_type && e.derivation == complex.derivation
        };
        match self.entries.get(&key) {
            Some(e) if matches(e) => {}
            Some(_) => return None,
            None => {
                self.entries.insert(key, fresh(complex));
            }
        }
        self.entries.get(&key)
    }

    /// The flattened child constraints (with automaton) of `complex`.
    pub(crate) fn children(
        &mut self,
        schema: &CompiledSchema,
        complex: &ComplexType,
    ) -> Arc<FlattenedChildren> {
        match self.entry(schema, complex) {
            Some(e) => Arc::clone(&e.children),
            None => Arc::new(crate::schema::xsd::compiler::flatten_type_children(
                complex, schema,
            )),
        }
    }

    /// The inheritance-flattened child element declarations of `complex`.
    pub(crate) fn elements(
        &mut self,
        schema: &CompiledSchema,
        complex: &ComplexType,
    ) -> Arc<Vec<ElementDef>> {
        match self.entry(schema, complex) {
            Some(e) => Arc::clone(&e.elements),
            None => Arc::new(collect_elements(schema, complex)),
        }
    }
}

/// The inheritance-flattened child element declarations of a complex type.
pub(crate) fn collect_elements(schema: &CompiledSchema, complex: &ComplexType) -> Vec<ElementDef> {
    let mut visited = HashSet::new();
    crate::schema::xsd::compiler::collect_elements_with_inheritance(complex, schema, &mut visited)
}
