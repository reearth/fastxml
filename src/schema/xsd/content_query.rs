//! Read-only access to compiled content models for XML producers.
//!
//! A validator walks a content model child by child; a producer (a writer
//! that must emit an element's children in the order its schema declares)
//! instead asks where each child goes. These lookups hand out the
//! [`ContentAutomaton`] of a type or element so that
//! [`ContentAutomaton::position_of`] can be queried.

use std::sync::Arc;

use crate::schema::types::{CompiledSchema, ComplexType, NsName, NsNameRef, TypeDef};
use crate::schema::xsd::compiler::{build_complex_automaton, substitution_index};
use crate::schema::xsd::content_automaton::ContentAutomaton;

impl CompiledSchema {
    /// The content-model automaton of the global complex type
    /// `{namespace_uri}local_name` (`""` for no namespace), including the
    /// content inherited through extension.
    ///
    /// `None` when there is no such complex type, or its content model has
    /// no automaton: no element content, an `xs:all` group, or a base type
    /// that could not be resolved.
    ///
    /// Lookup is exact, with no chameleon fallback: the components of a
    /// chameleon-included document (one without a target namespace) are
    /// registered, and their positions matched, in no namespace.
    pub fn type_content_automaton(
        &self,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<Arc<ContentAutomaton>> {
        self.ns_type_children_cache
            .get(&NsName::new(namespace_uri, local_name))?
            .automaton
            .clone()
    }

    /// The content-model automaton of the global element
    /// `{namespace_uri}local_name`: that of its named type, or of its
    /// anonymous complex type. An element without a type of its own takes
    /// its substitution-group head's type, as XSD specifies.
    ///
    /// An anonymous type's automaton is not cached by the schema, so it is
    /// built on every call; keep the returned `Arc` when querying repeatedly.
    /// `None` under the same conditions as
    /// [`type_content_automaton`](Self::type_content_automaton), whose exact
    /// lookup this shares.
    pub fn element_content_automaton(
        &self,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<Arc<ContentAutomaton>> {
        let element = |namespace_uri: &str, local_name: &str| {
            self.elements_ns.get(&NsNameRef {
                namespace_uri,
                local_name,
            })
        };
        let mut elem = element(namespace_uri, local_name)?;
        // Walk up the substitution chain of an untyped element. Chains have
        // no depth limit, but an acyclic one visits each global element at
        // most once, so a longer walk means a cycle.
        for _ in 0..self.elements_ns.len() {
            if let Some(type_ns) = &elem.type_ns {
                return self.type_content_automaton(&type_ns.namespace_uri, &type_ns.local_name);
            }
            if let Some(type_ref) = &elem.type_ref {
                let type_ns = self.resolve_type_ref_to_ns(type_ref)?;
                return self.type_content_automaton(&type_ns.namespace_uri, &type_ns.local_name);
            }
            if let Some(inline) = &elem.inline_type {
                let TypeDef::Complex(complex) = inline else {
                    return None;
                };
                let subst_ns = substitution_index(self);
                let base_of = |c: &ComplexType| self.complex_base_def(c);
                return build_complex_automaton(complex, self, &subst_ns, &base_of).map(Arc::new);
            }
            let head = elem.substitution_ns.as_ref()?;
            elem = element(&head.namespace_uri, &head.local_name)?;
        }
        None
    }
}
