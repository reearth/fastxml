//! XSD Compiler - transforms XSD AST into CompiledSchema.
//!
//! This module compiles parsed XSD schemas into the runtime validation
//! representation (CompiledSchema).

mod cache;
mod cycles;
mod facet_checks;
mod validity;
pub(crate) use cache::inherited_wildcard;
mod particles;
mod redefine;
mod references;
#[cfg(test)]
mod references_tests;
mod restriction;
mod substitution;
mod types;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::schema::types::{CompiledSchema, NsName};

use super::types::*;

/// XSD Compiler that transforms AST into CompiledSchema.
pub struct XsdCompiler {
    /// Type cache for resolving references
    pub(crate) type_cache: HashMap<String, crate::schema::types::TypeDef>,
    /// Substitution group index (head -> members)
    pub(crate) substitution_groups: HashMap<String, Vec<String>>,
    /// Namespace bindings for resolving prefixes
    pub(crate) namespace_bindings: HashMap<String, String>,
    /// Named model group definitions, keyed by (namespace URI, local name), for
    /// resolving `<xs:group ref="...">` during particle compilation.
    pub(crate) groups: HashMap<NsName, XsdParticle>,
    /// Named attribute group definitions for `<xs:attributeGroup ref>` resolution.
    pub(crate) attribute_groups: HashMap<NsName, XsdAttributeGroup>,
    /// blockDefault of the schema currently being compiled
    pub(crate) current_block_default: Option<DerivationControl>,
    /// Group names currently being expanded, used to break cyclic group refs.
    pub(crate) group_expansion: HashSet<NsName>,
    /// Current target namespace
    pub(crate) current_target_ns: Option<String>,
    /// Current target namespace prefix (from the schema being processed)
    pub(crate) current_target_prefix: Option<String>,
    /// Namespace bindings of the document currently being compiled (prefix ->
    /// URI). Unlike `namespace_bindings`, this is NOT accumulated across
    /// documents, so QName references can be resolved against the owning
    /// document exactly as the reference checker does.
    pub(crate) current_doc_bindings: HashMap<String, String>,
}

impl XsdCompiler {
    /// Creates a new compiler.
    pub fn new() -> Self {
        Self {
            type_cache: HashMap::new(),
            substitution_groups: HashMap::new(),
            namespace_bindings: HashMap::new(),
            groups: HashMap::new(),
            attribute_groups: HashMap::new(),
            current_block_default: None,
            group_expansion: HashSet::new(),
            current_target_ns: None,
            current_target_prefix: None,
            current_doc_bindings: HashMap::new(),
        }
    }

    /// Compiles multiple XSD schemas into a single CompiledSchema.
    ///
    /// Schemas should be provided in dependency order (dependencies first).
    /// Multiple schemas with the same targetNamespace (e.g., via xs:include) are all
    /// compiled and their types/elements are merged. If the same type/element is
    /// defined multiple times, the last definition wins.
    pub fn compile(&mut self, schemas: Vec<XsdSchema>) -> Result<CompiledSchema> {
        let mut schemas = schemas;
        let single_document = schemas.len() == 1;
        // Namespace strictness must be computed while import/include/redefine
        // declarations are still present in the AST.
        let strictness = references::Strictness::compute(&schemas);
        // Apply xs:redefine before anything is registered: originals are
        // renamed and redefinitions take their place.
        redefine::apply_redefines(&mut schemas);

        // Every QName reference into a fully-present (strict) namespace must
        // resolve to a declared component.
        references::check_references(&schemas, &strictness)?;

        let mut result = CompiledSchema::new();

        // Note: We intentionally do NOT deduplicate by targetNamespace here.
        //
        // XSD allows multiple schema files to share the same targetNamespace via xs:include.
        // For example, GML 3.1.1 has gml.xsd which includes feature.xsd, geometryBasic0d1d.xsd,
        // geometryBasic2d.xsd, etc. - all with targetNamespace="http://www.opengis.net/gml".
        //
        // If the same schema is resolved multiple times (e.g., through overlapping dependencies),
        // its types/elements will simply be registered again (last definition wins), which is
        // harmless since they're identical content.
        let deduplicated_schemas = schemas;

        // First pass: accumulate ALL namespace bindings from ALL schemas
        // This must happen before type registration so that cross-referenced
        // prefixes are available (e.g., main schema defines prefix for imported schema's namespace)
        for schema in &deduplicated_schemas {
            for (prefix, uri) in &schema.namespace_bindings {
                self.namespace_bindings.insert(prefix.clone(), uri.clone());
            }
        }

        // Second pass: register all types for forward reference resolution
        for schema in &deduplicated_schemas {
            self.register_types(schema)?;
        }

        // Register named model groups so `<xs:group ref>` can be expanded while
        // compiling particles in the next pass.
        for schema in &deduplicated_schemas {
            self.register_groups(schema);
        }

        // Third pass: compile each schema
        for schema in deduplicated_schemas {
            self.compile_schema(schema, &mut result)?;
        }

        // Build substitution group index
        self.build_substitution_groups(&mut result);

        // Build performance optimization caches
        self.build_transitive_substitution_groups(&mut result);
        self.build_type_children_cache(&mut result);

        // Unique Particle Attribution: ambiguity discovered while building
        // the content-model automata makes the schema invalid. (C5: iterate
        // the ns-keyed cache; the diagnostic renders prefix:local via the
        // schema's namespace-to-prefix table.)
        for (ns_name, fc) in &result.ns_type_children_cache {
            if let Some(automaton) = &fc.automaton
                && let Some(violation) = &automaton.upa_violation
            {
                return Err(crate::schema::error::SchemaError::InvalidSchema {
                    message: format!("type '{}': {}", result.display_name(ns_name), violation),
                }
                .into());
            }
        }

        // Particle-restriction legality (rcase-*) over the nested trees.
        // Only for single-document compilation: the compiled type table keys
        // types by prefix-qualified name, and multi-document sets can bind
        // the same prefix to different namespaces, making base-type
        // resolution ambiguous (and rejection unsafe).
        if single_document {
            restriction::check_particle_restrictions(&result)?;
        }

        validity::check_schema_validity(&result)?;

        Ok(result)
    }

    /// Registers types from a schema for forward reference resolution.
    fn register_types(&mut self, schema: &XsdSchema) -> Result<()> {
        use crate::schema::types::{SimpleType, TypeDef};

        // Note: namespace bindings are already accumulated in compile() before this is called

        // Find the prefix for THIS schema's target namespace.
        // First try the schema's OWN bindings (deterministic for each schema),
        // then fall back to accumulated bindings if needed (for schemas that don't
        // define a prefix for their own namespace, e.g., imported schemas)
        let ns_prefix = schema.target_namespace.as_ref().and_then(|ns| {
            // First: try schema's own bindings (non-empty prefix only)
            schema
                .namespace_bindings
                .iter()
                .find(|(k, v)| !k.is_empty() && *v == ns)
                .map(|(k, _)| k.clone())
                // Second: fall back to accumulated bindings (already populated)
                .or_else(|| {
                    self.namespace_bindings
                        .iter()
                        .find(|(k, v)| !k.is_empty() && *v == ns)
                        .map(|(k, _)| k.clone())
                })
        });

        for type_def in &schema.types {
            if let Some(name) = type_def.name() {
                let qname = match &ns_prefix {
                    Some(p) => format!("{}:{}", p, name),
                    None => name.to_string(),
                };

                // Pre-register as placeholder
                let placeholder = match type_def {
                    XsdTypeDef::Simple(_) => TypeDef::Simple(SimpleType::new(name)),
                    XsdTypeDef::Complex(_) => {
                        TypeDef::Complex(crate::schema::types::ComplexType::new(name))
                    }
                };
                self.type_cache.insert(qname.clone(), placeholder);

                // Also register with just the local name for cross-namespace lookup
                let local_placeholder = match type_def {
                    XsdTypeDef::Simple(_) => TypeDef::Simple(SimpleType::new(name)),
                    XsdTypeDef::Complex(_) => {
                        TypeDef::Complex(crate::schema::types::ComplexType::new(name))
                    }
                };
                self.type_cache
                    .entry(name.to_string())
                    .or_insert(local_placeholder);
            }
        }

        Ok(())
    }

    /// Registers named model group definitions for `<xs:group ref>` resolution.
    ///
    /// Groups are keyed by (namespace URI, local name) so that references across
    /// namespaces resolve unambiguously, mirroring the namespace-aware type cache.
    fn register_groups(&mut self, schema: &XsdSchema) {
        let ns = schema.target_namespace.clone().unwrap_or_default();
        for grp in &schema.groups {
            if let (Some(name), Some(particle)) = (&grp.name, &grp.particle) {
                let key = NsName::new(ns.clone(), name.clone());
                self.groups.insert(key, particle.clone());
            }
        }
        for ag in &schema.attribute_groups {
            if let Some(name) = &ag.name {
                let key = NsName::new(ns.clone(), name.clone());
                self.attribute_groups.insert(key, ag.clone());
            }
        }
    }

    /// Compiles a single schema into the result.
    fn compile_schema(&mut self, schema: XsdSchema, result: &mut CompiledSchema) -> Result<()> {
        self.current_target_ns = schema.target_namespace.clone();
        self.current_block_default = schema.block_default.clone();
        // Snapshot the owning document's own bindings (not accumulated) so
        // QName references can be resolved per-document (mirrors the reference
        // checker), independent of the last-wins accumulated prefix table.
        self.current_doc_bindings = schema.namespace_bindings.clone();

        // Find the prefix for THIS schema's target namespace.
        // First try the schema's OWN bindings (deterministic for each schema),
        // then fall back to accumulated bindings if needed (for schemas that don't
        // define a prefix for their own namespace, e.g., imported schemas)
        self.current_target_prefix = schema.target_namespace.as_ref().and_then(|ns| {
            // First: try schema's own bindings (non-empty prefix only)
            schema
                .namespace_bindings
                .iter()
                .find(|(k, v)| !k.is_empty() && *v == ns)
                .map(|(k, _)| k.clone())
                // Second: fall back to accumulated bindings
                .or_else(|| {
                    self.namespace_bindings
                        .iter()
                        .find(|(k, v)| !k.is_empty() && *v == ns)
                        .map(|(k, _)| k.clone())
                })
        });

        // Set target namespace if this is the first schema with one
        if result.target_namespace.is_none() && schema.target_namespace.is_some() {
            result.target_namespace = schema.target_namespace.clone();
        }

        // Store namespace bindings in result for runtime lookup
        for (prefix, uri) in &self.namespace_bindings {
            if !prefix.is_empty() {
                // uri -> prefix (first prefix wins)
                result
                    .namespace_prefixes
                    .entry(uri.clone())
                    .or_insert_with(|| prefix.clone());
                // prefix -> uri (always overwrite to latest)
                result.prefix_namespaces.insert(prefix.clone(), uri.clone());
            }
        }

        // Compile types. Components are registered under the OWNING
        // document's target namespace (unambiguous here) — the C5 cutover
        // removed the legacy prefix/bare-local string keys entirely.
        for type_def in schema.types {
            let compiled = self.compile_type(&type_def)?;
            if let Some(name) = type_def.name() {
                let ns_name = NsName::new(
                    self.current_target_ns.clone().unwrap_or_default(),
                    name.to_string(),
                );
                result.types_ns.insert(ns_name, compiled.clone());

                // The compiler-internal type cache keeps prefix-qualified
                // string keys: resolve_qname's requalification probes it
                // with "prefix:local" while compiling references.
                self.type_cache.insert(self.make_qname(name), compiled);
            }
        }

        // Compile elements: global top-level elements are always qualified
        // in the target namespace regardless of elementFormDefault.
        for element in schema.elements {
            let compiled = self.compile_element(&element)?;
            let ns_name = NsName::new(
                self.current_target_ns.clone().unwrap_or_default(),
                element.name.clone(),
            );
            result.elements_ns.insert(ns_name, compiled);
        }

        // Compile top-level attributes
        for attr in schema.attributes {
            if let Some(name) = &attr.name {
                let compiled = self.compile_attribute(&attr)?;
                let ns_name = NsName::new(
                    self.current_target_ns.clone().unwrap_or_default(),
                    name.to_string(),
                );
                result.attributes_ns.insert(ns_name, compiled);
            }
        }

        Ok(())
    }

    /// Makes a qualified name using current namespace prefix (used for the
    /// compiler-internal `type_cache` keys consumed by `resolve_qname`).
    pub(crate) fn make_qname(&self, local: &str) -> String {
        // Use the schema's own prefix for its target namespace (set in compile_schema)
        // This ensures deterministic prefix selection
        if let Some(prefix) = &self.current_target_prefix {
            return format!("{}:{}", prefix, local);
        }
        local.to_string()
    }

    /// Resolves a QName to its full qualified name.
    ///
    /// When the QName has no prefix (e.g., from a schema that uses default namespace),
    /// this method tries to qualify it using the current target namespace prefix.
    /// This ensures that type references like `base="AbstractBoundarySurfaceType"`
    /// in building.xsd are resolved to `"bldg:AbstractBoundarySurfaceType"` when
    /// the `bldg` prefix is available from accumulated namespace bindings.
    pub(crate) fn resolve_qname(&self, qname: &QName) -> String {
        if qname.prefix.is_none() {
            if let Some(ref prefix) = self.current_target_prefix {
                let qualified = format!("{}:{}", prefix, qname.local);
                if self.type_cache.contains_key(&qualified) {
                    return qualified;
                }
            }
        }
        qname.to_string_full()
    }

    /// Resolves a type reference to its definition.
    pub fn resolve_type(&self, type_ref: &str) -> Option<&crate::schema::types::TypeDef> {
        self.type_cache.get(type_ref)
    }

    /// Resolves a [`QName`] (as written in the owning schema document) to a
    /// namespace-qualified [`NsName`], mirroring the reference checker's rules
    /// (`references.rs::resolve_ns`):
    ///
    /// - the `xml` prefix maps to the XML namespace;
    /// - a declared prefix resolves against the owning document's bindings
    ///   (returns `None` for an undeclared prefix);
    /// - an unprefixed name takes the document's default namespace when one is
    ///   bound, otherwise the owning document's target namespace (the same
    ///   leniency [`resolve_qname`](Self::resolve_qname) applies when it
    ///   requalifies an unprefixed reference), falling back to the
    ///   no-namespace `""`.
    ///
    /// This is the per-document counterpart of [`resolve_qname`], which keys
    /// off the accumulated, last-wins prefix table and therefore mis-resolves
    /// when documents bind the same prefix to different URIs. The string form
    /// of every reference is still kept verbatim for message stability.
    pub(crate) fn resolve_qname_ns(&self, qname: &QName) -> Option<NsName> {
        let local = qname.local.trim();
        let ns: std::sync::Arc<str> = match qname.prefix.as_deref().map(str::trim) {
            Some("xml") => crate::namespace::common::XML_NS.into(),
            Some(p) => self.current_doc_bindings.get(p)?.as_str().into(),
            None => {
                let default_ns = self
                    .current_doc_bindings
                    .get("")
                    .map(String::as_str)
                    .filter(|d| !d.is_empty());
                match default_ns {
                    Some(d) => d.into(),
                    None => self.current_target_ns.as_deref().unwrap_or("").into(),
                }
            }
        };
        Some(NsName::new(ns, local))
    }
}

impl Default for XsdCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiles XSD AST schemas into a CompiledSchema.
pub fn compile_schemas(schemas: Vec<XsdSchema>) -> Result<CompiledSchema> {
    let mut compiler = XsdCompiler::new();
    compiler.compile(schemas)
}
