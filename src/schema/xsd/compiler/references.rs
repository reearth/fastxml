//! Reference-integrity checking over the XSD AST.
//!
//! After all schema documents of a set are known, every QName reference
//! (`type=`, `base=`, `ref=`, `itemType=`, `memberTypes=`, `substitutionGroup=`,
//! keyref `refer=`) must resolve to a declared component — but only when the
//! referenced namespace is *strict*: fully present in the compiled set. A
//! namespace whose imports/includes could not be resolved (or were never
//! fetched, as in single-document compilation) is *poisoned* and stays lenient,
//! so real-world schema sets with partial dependencies keep compiling.

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::schema::error::SchemaError;

use super::super::types::*;

/// The XSD schema namespace.
const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
/// The XML namespace (implicitly bound to the `xml` prefix).
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The XML Schema instance namespace.
const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

use crate::schema::xsd::builtin::is_builtin_xsd_type_local;

/// Namespaces that are always lenient because fastxml provides (partial)
/// built-in definitions for them rather than compiled schema documents.
fn is_always_lenient(ns: &str) -> bool {
    ns == XML_NS
        || ns == XSI_NS
        || ns == crate::schema::xsd::builtin::GML_NAMESPACE
        || ns == crate::schema::xsd::builtin::GML31_NAMESPACE
}

/// Which namespaces of a schema set are strict (fully present) vs poisoned.
///
/// Must be computed *before* `xs:redefine` application drains the redefine
/// declarations from the AST.
#[derive(Debug, Default)]
pub(crate) struct Strictness {
    /// Target namespaces defined by schemas in the set ("" = no namespace).
    defined: HashSet<String>,
    /// Namespaces with unresolved imports/includes targeting them.
    poisoned: HashSet<String>,
}

impl Strictness {
    /// Computes strictness from the full AST set.
    ///
    /// A namespace is poisoned when:
    /// - an `xs:import` for it has no resolved document in the set, or
    /// - a schema of that namespace has `xs:include`/`xs:redefine` but no other
    ///   document of the same namespace is present (the include was never
    ///   fetched), or
    /// - a schema of that namespace includes documents while a chameleon
    ///   (no-targetNamespace) document is in the set (its components are not
    ///   re-homed into the includer's namespace).
    pub(crate) fn compute(schemas: &[XsdSchema]) -> Self {
        let mut defined: HashSet<String> = HashSet::new();
        for schema in schemas {
            defined.insert(tns_key(schema));
        }

        let has_chameleon_doc = schemas
            .iter()
            .any(|s| s.target_namespace.is_none() && schemas.len() > 1);

        let mut poisoned: HashSet<String> = HashSet::new();
        for schema in schemas {
            for import in &schema.imports {
                let ns = import.namespace.clone().unwrap_or_default();
                if !defined.contains(&ns) {
                    poisoned.insert(ns);
                }
            }
            if !schema.includes.is_empty() || !schema.redefines.is_empty() {
                let own = tns_key(schema);
                let satisfied = schemas
                    .iter()
                    .any(|other| !std::ptr::eq(other, schema) && tns_key(other) == own);
                if !satisfied || (has_chameleon_doc && schema.target_namespace.is_some()) {
                    poisoned.insert(own);
                }
            }
        }

        Self { defined, poisoned }
    }

    /// Whether references into `ns` must resolve.
    fn is_strict(&self, ns: &str) -> bool {
        if self.poisoned.contains(ns) || is_always_lenient(ns) {
            return false;
        }
        // The XSD namespace resolves against built-ins even when no schema
        // document for it is present.
        ns == XSD_NS || self.defined.contains(ns)
    }
}

fn tns_key(schema: &XsdSchema) -> String {
    schema.target_namespace.clone().unwrap_or_default()
}

/// What a keyref `refer=` may point at.
#[derive(Debug, Clone, Copy)]
struct KeyInfo {
    /// Whether the constraint is a key/unique (a legal refer target).
    is_key_or_unique: bool,
    /// Number of field expressions.
    field_count: usize,
}

/// What is known about a top-level type definition.
#[derive(Debug, Clone, Copy, Default)]
struct TypeInfo {
    /// Whether the type is a complex type.
    is_complex: bool,
    /// final blocks derivation by extension.
    final_extension: bool,
    /// final blocks derivation by restriction.
    final_restriction: bool,
    /// final blocks use as a list item type.
    final_list: bool,
    /// final blocks use as a union member type.
    final_union: bool,
}

/// Expands a final/finalDefault control into per-method flags.
fn final_flags(control: Option<&DerivationControl>) -> (bool, bool, bool, bool) {
    match control {
        None => (false, false, false, false),
        Some(DerivationControl::All) => (true, true, true, true),
        Some(DerivationControl::List(types)) => (
            types.contains(&DerivationType::Extension),
            types.contains(&DerivationType::Restriction),
            types.contains(&DerivationType::List),
            types.contains(&DerivationType::Union),
        ),
    }
}

/// Symbol tables of top-level component names, keyed by namespace.
#[derive(Debug, Default)]
struct SymbolTables {
    /// Type names with their kind and derivation controls.
    types: HashMap<String, HashMap<String, TypeInfo>>,
    elements: HashMap<String, HashSet<String>>,
    attributes: HashMap<String, HashSet<String>>,
    groups: HashMap<String, HashSet<String>>,
    attribute_groups: HashMap<String, HashSet<String>>,
    /// Identity-constraint names (for keyref refer=) with their metadata.
    keys: HashMap<String, HashMap<String, KeyInfo>>,
}

impl SymbolTables {
    fn build(schemas: &[XsdSchema]) -> Self {
        let mut tables = Self::default();
        for schema in schemas {
            let ns = tns_key(schema);
            for type_def in &schema.types {
                if let Some(name) = type_def.name() {
                    // Effective final: the type's own final, else the
                    // schema's finalDefault.
                    let control = match type_def {
                        XsdTypeDef::Complex(ct) => ct.final_.as_ref(),
                        XsdTypeDef::Simple(st) => st.final_.as_ref(),
                    }
                    .or(schema.final_default.as_ref());
                    let (ext, restr, list, union) = final_flags(control);
                    let info = if type_def.is_complex() {
                        TypeInfo {
                            is_complex: true,
                            final_extension: ext,
                            final_restriction: restr,
                            // list/union do not apply to complex types.
                            final_list: false,
                            final_union: false,
                        }
                    } else {
                        TypeInfo {
                            is_complex: false,
                            // extension does not apply to simple-type final.
                            final_extension: false,
                            final_restriction: restr,
                            final_list: list,
                            final_union: union,
                        }
                    };
                    tables
                        .types
                        .entry(ns.clone())
                        .or_default()
                        .insert(name.to_string(), info);
                }
            }
            for element in &schema.elements {
                tables
                    .elements
                    .entry(ns.clone())
                    .or_default()
                    .insert(element.name.clone());
                collect_constraint_names(element, &ns, &mut tables.keys);
            }
            for attr in &schema.attributes {
                if let Some(name) = &attr.name {
                    tables
                        .attributes
                        .entry(ns.clone())
                        .or_default()
                        .insert(name.clone());
                }
            }
            for group in &schema.groups {
                if let Some(name) = &group.name {
                    tables
                        .groups
                        .entry(ns.clone())
                        .or_default()
                        .insert(name.clone());
                }
            }
            for ag in &schema.attribute_groups {
                if let Some(name) = &ag.name {
                    tables
                        .attribute_groups
                        .entry(ns.clone())
                        .or_default()
                        .insert(name.clone());
                }
            }
            // Identity constraints can sit on elements nested inside types.
            for type_def in &schema.types {
                collect_constraints_in_type(type_def, &ns, &mut tables.keys);
            }
        }
        tables
    }

    /// Looks up keyref-refer metadata, with the same chameleon no-namespace
    /// fallback as [`Self::contains`].
    fn lookup_key(&self, ns: &str, local: &str) -> Option<KeyInfo> {
        if let Some(info) = self.keys.get(ns).and_then(|m| m.get(local)) {
            return Some(*info);
        }
        if !ns.is_empty() {
            return self.keys.get("").and_then(|m| m.get(local)).copied();
        }
        None
    }

    /// Looks up what is known about a type reference target, with the same
    /// fallbacks as [`Self::contains`].
    fn lookup_type_info(&self, ns: &str, local: &str) -> Option<TypeInfo> {
        if let Some(info) = self.types.get(ns).and_then(|m| m.get(local)) {
            return Some(*info);
        }
        if !ns.is_empty()
            && let Some(info) = self.types.get("").and_then(|m| m.get(local))
        {
            return Some(*info);
        }
        if (ns == XSD_NS || ns.is_empty()) && is_builtin_xsd_type_local(local) {
            // Every built-in datatype is simple except the ur-type; none
            // carries a final constraint.
            return Some(TypeInfo {
                is_complex: local == "anyType",
                ..TypeInfo::default()
            });
        }
        None
    }

    fn contains(&self, kind: RefKind, ns: &str, local: &str) -> bool {
        if kind == RefKind::Key {
            return self.lookup_key(ns, local).is_some();
        }
        if kind == RefKind::Type {
            return self.lookup_type_info(ns, local).is_some();
        }
        let table = match kind {
            RefKind::Element => &self.elements,
            RefKind::Attribute => &self.attributes,
            RefKind::Group => &self.groups,
            RefKind::AttributeGroup => &self.attribute_groups,
            RefKind::Type | RefKind::Key => unreachable!("handled above"),
        };
        if table.get(ns).is_some_and(|set| set.contains(local)) {
            return true;
        }
        // Components from chameleon (no-targetNamespace) documents are merged
        // by the compiler under their local names; let them satisfy references
        // into any namespace so we never reject what the compiler resolves.
        !ns.is_empty() && table.get("").is_some_and(|set| set.contains(local))
    }
}

/// Collects key/unique constraint names declared on an element (and its
/// nested content) into the per-namespace key table.
fn collect_constraint_names(
    element: &XsdElement,
    ns: &str,
    keys: &mut HashMap<String, HashMap<String, KeyInfo>>,
) {
    for ic in &element.identity_constraints {
        // keyref names share the symbol space; record them too so refer
        // resolution can distinguish "missing" from "wrong kind".
        keys.entry(ns.to_string()).or_default().insert(
            ic.name.clone(),
            KeyInfo {
                is_key_or_unique: ic.constraint_type != XsdConstraintType::KeyRef,
                field_count: ic.fields.len(),
            },
        );
    }
    if let Some(inline) = &element.inline_type {
        collect_constraints_in_type(inline, ns, keys);
    }
}

fn collect_constraints_in_type(
    type_def: &XsdTypeDef,
    ns: &str,
    keys: &mut HashMap<String, HashMap<String, KeyInfo>>,
) {
    let XsdTypeDef::Complex(ct) = type_def else {
        return;
    };
    let mut walk_particle = |particle: &XsdParticle| {
        collect_constraints_in_particle(particle, ns, keys);
    };
    match &ct.content {
        XsdComplexContent::Particle(p) => walk_particle(p),
        XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
            XsdComplexContentDerivation::Extension(ext) => {
                if let Some(p) = &ext.particle {
                    walk_particle(p);
                }
            }
            XsdComplexContentDerivation::Restriction(r) => {
                if let Some(p) = &r.particle {
                    walk_particle(p);
                }
            }
        },
        _ => {}
    }
}

fn collect_constraints_in_particle(
    particle: &XsdParticle,
    ns: &str,
    keys: &mut HashMap<String, HashMap<String, KeyInfo>>,
) {
    match particle {
        XsdParticle::Sequence(seq) => {
            for item in &seq.particles {
                collect_constraints_in_item(item, ns, keys);
            }
        }
        XsdParticle::Choice(choice) => {
            for item in &choice.particles {
                collect_constraints_in_item(item, ns, keys);
            }
        }
        XsdParticle::All(all) => {
            for element in &all.elements {
                collect_constraint_names(element, ns, keys);
            }
        }
        XsdParticle::GroupRef(_) | XsdParticle::Any(_) => {}
    }
}

fn collect_constraints_in_item(
    item: &XsdParticleItem,
    ns: &str,
    keys: &mut HashMap<String, HashMap<String, KeyInfo>>,
) {
    match item {
        XsdParticleItem::Element(element) => collect_constraint_names(element, ns, keys),
        XsdParticleItem::Sequence(seq) => {
            for nested in &seq.particles {
                collect_constraints_in_item(nested, ns, keys);
            }
        }
        XsdParticleItem::Choice(choice) => {
            for nested in &choice.particles {
                collect_constraints_in_item(nested, ns, keys);
            }
        }
        XsdParticleItem::GroupRef(_) | XsdParticleItem::Any(_) => {}
    }
}

/// The kind of component a reference points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefKind {
    Type,
    Element,
    Attribute,
    Group,
    AttributeGroup,
    Key,
}

impl RefKind {
    fn as_str(self) -> &'static str {
        match self {
            RefKind::Type => "type",
            RefKind::Element => "element",
            RefKind::Attribute => "attribute",
            RefKind::Group => "group",
            RefKind::AttributeGroup => "attribute group",
            RefKind::Key => "identity constraint",
        }
    }
}

/// Checks every QName reference in the schema set. Call after
/// `apply_redefines` (so redefined self-references point at the surviving
/// synthetic names) with a [`Strictness`] computed before it.
pub(crate) fn check_references(schemas: &[XsdSchema], strictness: &Strictness) -> Result<()> {
    let tables = SymbolTables::build(schemas);
    check_definition_cycles(schemas)?;
    for schema in schemas {
        let checker = Checker {
            schema,
            strictness,
            tables: &tables,
        };
        checker.check_schema()?;
    }
    Ok(())
}

/// Resolves a reference QName to a `(namespace, local)` key using the owning
/// schema's bindings, mirroring [`Checker::check_ref`]'s fallbacks. Only
/// returns keys present in `known`.
fn resolve_def_key(
    schema: &XsdSchema,
    qname: &QName,
    known: &HashSet<(String, String)>,
) -> Option<(String, String)> {
    let local = qname.local.trim().to_string();
    let ns = match qname.prefix.as_deref().map(str::trim) {
        Some("xml") => return None,
        Some(p) => schema.namespace_bindings.get(p)?.clone(),
        None => schema
            .namespace_bindings
            .get("")
            .cloned()
            .unwrap_or_default(),
    };
    let key = (ns, local.clone());
    if known.contains(&key) {
        return Some(key);
    }
    if !key.0.is_empty() {
        let chameleon = (String::new(), local.clone());
        if known.contains(&chameleon) {
            return Some(chameleon);
        }
    }
    if qname.prefix.is_none()
        && let Some(own) = &schema.target_namespace
    {
        let own_key = (own.clone(), local);
        if known.contains(&own_key) {
            return Some(own_key);
        }
    }
    None
}

/// Collects the group references appearing in a particle tree.
fn group_refs_in_particle<'a>(particle: &'a XsdParticle, out: &mut Vec<&'a QName>) {
    match particle {
        XsdParticle::Sequence(seq) => {
            for item in &seq.particles {
                group_refs_in_item(item, out);
            }
        }
        XsdParticle::Choice(choice) => {
            for item in &choice.particles {
                group_refs_in_item(item, out);
            }
        }
        XsdParticle::GroupRef(group_ref) => out.push(&group_ref.name),
        XsdParticle::All(_) | XsdParticle::Any(_) => {}
    }
}

fn group_refs_in_item<'a>(item: &'a XsdParticleItem, out: &mut Vec<&'a QName>) {
    match item {
        XsdParticleItem::Sequence(seq) => {
            for nested in &seq.particles {
                group_refs_in_item(nested, out);
            }
        }
        XsdParticleItem::Choice(choice) => {
            for nested in &choice.particles {
                group_refs_in_item(nested, out);
            }
        }
        XsdParticleItem::GroupRef(group_ref) => out.push(&group_ref.name),
        XsdParticleItem::Element(_) | XsdParticleItem::Any(_) => {}
    }
}

/// Collects the derivation-base references of a type definition (restriction
/// / extension bases, list item types, union member types), including those
/// of nested anonymous simple types.
fn type_base_refs<'a>(type_def: &'a XsdTypeDef, out: &mut Vec<&'a QName>) {
    match type_def {
        XsdTypeDef::Complex(ct) => match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => out.push(&ext.base),
                XsdSimpleContentDerivation::Restriction(r) => out.push(&r.base),
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => out.push(&ext.base),
                XsdComplexContentDerivation::Restriction(r) => out.push(&r.base),
            },
            XsdComplexContent::Empty | XsdComplexContent::Particle(_) => {}
        },
        XsdTypeDef::Simple(st) => simple_base_refs(st, out),
    }
}

fn simple_base_refs<'a>(st: &'a XsdSimpleType, out: &mut Vec<&'a QName>) {
    match &st.content {
        XsdSimpleTypeContent::Restriction(r) => {
            if let Some(base) = &r.base {
                out.push(base);
            }
            if let Some(inline) = &r.inline_base {
                simple_base_refs(inline, out);
            }
        }
        XsdSimpleTypeContent::List(list) => {
            if let Some(item) = &list.item_type {
                out.push(item);
            }
            if let Some(inline) = &list.inline_type {
                simple_base_refs(inline, out);
            }
        }
        XsdSimpleTypeContent::Union(union) => {
            // Union membership is not part of the derivation-cycle graph:
            // the W3C suite treats mutually-referencing unions as valid
            // (msData/simpleType/stE110).
            for inline in &union.inline_types {
                simple_base_refs(inline, out);
            }
        }
    }
}

/// Detects circular `xs:group` / `xs:attributeGroup` references and circular
/// type derivations (src-model_group / src-attribute_group /
/// st-props-correct / ct-props-correct circularity rules).
fn check_definition_cycles(schemas: &[XsdSchema]) -> Result<()> {
    // (namespace, name) -> (owning schema index, outgoing references)
    let mut groups: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    let mut attr_groups: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    let mut types: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    for (si, schema) in schemas.iter().enumerate() {
        let ns = tns_key(schema);
        for group in &schema.groups {
            if let Some(name) = &group.name {
                let mut refs = Vec::new();
                if let Some(particle) = &group.particle {
                    group_refs_in_particle(particle, &mut refs);
                }
                groups.insert((ns.clone(), name.clone()), (si, refs));
            }
        }
        for ag in &schema.attribute_groups {
            if let Some(name) = &ag.name {
                let mut refs: Vec<&QName> = ag.attribute_groups.iter().collect();
                if let Some(r) = &ag.ref_ {
                    refs.push(r);
                }
                attr_groups.insert((ns.clone(), name.clone()), (si, refs));
            }
        }
        for type_def in &schema.types {
            if let Some(name) = type_def.name() {
                let mut refs = Vec::new();
                type_base_refs(type_def, &mut refs);
                types.insert((ns.clone(), name.to_string()), (si, refs));
            }
        }
    }
    detect_cycles("group", schemas, &groups)?;
    detect_cycles("attributeGroup", schemas, &attr_groups)?;
    detect_cycles("type derivation", schemas, &types)?;
    Ok(())
}

/// DFS cycle detection over a definition graph.
fn detect_cycles(
    kind: &str,
    schemas: &[XsdSchema],
    graph: &HashMap<(String, String), (usize, Vec<&QName>)>,
) -> Result<()> {
    let known: HashSet<(String, String)> = graph.keys().cloned().collect();
    // 1 = visiting, 2 = done
    let mut state: HashMap<&(String, String), u8> = HashMap::new();

    fn visit<'a>(
        kind: &str,
        key: &'a (String, String),
        schemas: &[XsdSchema],
        graph: &'a HashMap<(String, String), (usize, Vec<&QName>)>,
        known: &HashSet<(String, String)>,
        state: &mut HashMap<&'a (String, String), u8>,
    ) -> Result<()> {
        match state.get(key) {
            Some(1) => {
                return Err(SchemaError::InvalidSchema {
                    message: format!("circular {} reference involving '{}'", kind, key.1),
                }
                .into());
            }
            Some(_) => return Ok(()),
            None => {}
        }
        state.insert(key, 1);
        let (si, refs) = &graph[key];
        for qname in refs {
            if let Some(target) = resolve_def_key(&schemas[*si], qname, known)
                && let Some((target_key, _)) = graph.get_key_value(&target)
            {
                visit(kind, target_key, schemas, graph, known, state)?;
            }
        }
        state.insert(key, 2);
        Ok(())
    }

    for key in graph.keys() {
        visit(kind, key, schemas, graph, &known, &mut state)?;
    }
    Ok(())
}

/// Per-schema reference checker (QName prefixes resolve against the owning
/// document's namespace bindings).
struct Checker<'a> {
    schema: &'a XsdSchema,
    strictness: &'a Strictness,
    tables: &'a SymbolTables,
}

impl Checker<'_> {
    /// Resolves a QName's namespace using this schema's bindings.
    /// Returns `None` when the prefix is undeclared. QName attribute values
    /// are whitespace-collapsed, so stray whitespace is trimmed first.
    fn resolve_ns(&self, prefix: Option<&str>) -> Option<String> {
        match prefix {
            Some("xml") => Some(XML_NS.to_string()),
            Some(p) => self.schema.namespace_bindings.get(p).cloned(),
            None => Some(
                self.schema
                    .namespace_bindings
                    .get("")
                    .cloned()
                    .unwrap_or_default(),
            ),
        }
    }

    /// Checks one reference; errors when it points into a strict namespace
    /// but no matching declaration exists.
    fn check_ref(&self, kind: RefKind, qname: &QName, from: &str) -> Result<()> {
        self.check_ref_inner(kind, qname, from, true)
    }

    /// Lazily checks a reference: per the resolution rules the W3C test suite
    /// expects (saxonData/Missing), a dangling `type=` on an unused top-level
    /// element, `substitutionGroup=`, `itemType=` or `memberTypes=` is only an
    /// error when the component is needed for validation — except when the
    /// QName can *never* resolve (undeclared prefix, or a miss in the closed
    /// built-in XSD namespace).
    fn check_ref_lazy(&self, kind: RefKind, qname: &QName, from: &str) -> Result<()> {
        self.check_ref_inner(kind, qname, from, false)
    }

    fn check_ref_inner(
        &self,
        kind: RefKind,
        qname: &QName,
        from: &str,
        require_existence: bool,
    ) -> Result<()> {
        // QName values are whitespace-collapsed before resolution.
        let prefix = qname.prefix.as_deref().map(str::trim);
        let local = qname.local.trim();
        let Some(ns) = self.resolve_ns(prefix) else {
            // Undeclared prefix: the QName cannot denote any component.
            return Err(SchemaError::DanglingReference {
                kind: kind.as_str(),
                name: qname.to_string_full(),
                referenced_from: format!("{} (undeclared namespace prefix)", from),
            }
            .into());
        };
        if self.tables.contains(kind, &ns, local) {
            return Ok(());
        }
        // Mirror the compiler's leniency: an unprefixed reference falls back
        // to the schema's own target namespace.
        if prefix.is_none()
            && let Some(own) = &self.schema.target_namespace
            && *own != ns
            && self.tables.contains(kind, own, local)
        {
            return Ok(());
        }
        // The XSD namespace is closed: a miss there can never be satisfied
        // later, even under lazy resolution.
        let must_resolve = self.strictness.is_strict(&ns) && (require_existence || ns == XSD_NS);
        if !must_resolve {
            return Ok(());
        }
        Err(SchemaError::DanglingReference {
            kind: kind.as_str(),
            name: qname.to_string_full(),
            referenced_from: from.to_string(),
        }
        .into())
    }

    fn check_schema(&self) -> Result<()> {
        for element in &self.schema.elements {
            self.check_element_at(element, "top-level element", true)?;
        }
        for type_def in &self.schema.types {
            self.check_type_def(type_def)?;
        }
        for attr in &self.schema.attributes {
            self.check_attribute(attr, "top-level attribute")?;
        }
        for group in &self.schema.groups {
            if let Some(particle) = &group.particle {
                let from = format!("group '{}'", group.name.as_deref().unwrap_or("(anonymous)"));
                self.check_particle(particle, &from)?;
            }
        }
        for ag in &self.schema.attribute_groups {
            let from = format!(
                "attribute group '{}'",
                ag.name.as_deref().unwrap_or("(anonymous)")
            );
            self.check_attribute_group_body(ag, &from)?;
        }
        Ok(())
    }

    fn check_element(&self, element: &XsdElement, from: &str) -> Result<()> {
        self.check_element_at(element, from, false)
    }

    fn check_element_at(&self, element: &XsdElement, from: &str, top_level: bool) -> Result<()> {
        if let Some(r) = &element.ref_ {
            self.check_ref(RefKind::Element, r, from)?;
        }
        if let Some(t) = &element.type_ref {
            // A dangling type on an unused top-level element declaration is
            // tolerated under lazy component resolution (saxonData/Missing).
            if top_level {
                self.check_ref_lazy(RefKind::Type, t, from)?;
            } else {
                self.check_ref(RefKind::Type, t, from)?;
            }
        }
        if let Some(sg) = &element.substitution_group {
            self.check_ref_lazy(RefKind::Element, sg, from)?;
        }
        if let Some(inline) = &element.inline_type {
            self.check_type_def(inline)?;
        }
        for ic in &element.identity_constraints {
            if let Some(refer) = &ic.refer {
                let from = format!("keyref '{}'", ic.name);
                self.check_ref(RefKind::Key, refer, &from)?;
                self.check_keyref_target(ic, refer)?;
            }
        }
        Ok(())
    }

    /// Resolves what is known about a type reference target with the same
    /// resolution order as [`Self::check_ref`].
    fn resolve_type_info(&self, qname: &QName) -> Option<TypeInfo> {
        let prefix = qname.prefix.as_deref().map(str::trim);
        let local = qname.local.trim();
        let ns = self.resolve_ns(prefix)?;
        if let Some(info) = self.tables.lookup_type_info(&ns, local) {
            return Some(info);
        }
        if prefix.is_none()
            && let Some(own) = &self.schema.target_namespace
            && *own != ns
        {
            return self.tables.lookup_type_info(own, local);
        }
        None
    }

    /// Errors when a type reference resolves to a complex type in a context
    /// that requires a simple type.
    fn require_simple_type(&self, qname: &QName, from: &str) -> Result<()> {
        if self.resolve_type_info(qname).is_some_and(|i| i.is_complex) {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "{} requires a simple type, but '{}' is complex",
                    from, qname
                ),
            }
            .into());
        }
        Ok(())
    }

    /// Errors when a complexContent base resolves to a simple type.
    fn require_complex_type(&self, qname: &QName, from: &str) -> Result<()> {
        if self.resolve_type_info(qname).is_some_and(|i| !i.is_complex) {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "{} requires a complex type, but '{}' is simple",
                    from, qname
                ),
            }
            .into());
        }
        Ok(())
    }

    /// Errors when the base type's final constraint blocks this derivation
    /// or use (`method` selects which final flag applies).
    fn check_final(&self, qname: &QName, method: DerivationType, from: &str) -> Result<()> {
        let Some(info) = self.resolve_type_info(qname) else {
            return Ok(());
        };
        let blocked = match method {
            DerivationType::Extension => info.final_extension,
            DerivationType::Restriction => info.final_restriction,
            DerivationType::List => info.final_list,
            DerivationType::Union => info.final_union,
            DerivationType::Substitution => false,
        };
        if blocked {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "type '{}' is final and cannot be used for {} ({})",
                    qname,
                    match method {
                        DerivationType::Extension => "derivation by extension",
                        DerivationType::Restriction => "derivation by restriction",
                        DerivationType::List => "a list item type",
                        DerivationType::Union => "a union member type",
                        DerivationType::Substitution => "substitution",
                    },
                    from
                ),
            }
            .into());
        }
        Ok(())
    }

    /// When a keyref's refer target resolves, it must be a key/unique (not
    /// another keyref) and its field count must match the keyref's.
    fn check_keyref_target(&self, ic: &XsdIdentityConstraint, refer: &QName) -> Result<()> {
        let prefix = refer.prefix.as_deref().map(str::trim);
        let local = refer.local.trim();
        let Some(ns) = self.resolve_ns(prefix) else {
            return Ok(()); // already reported by check_ref
        };
        let Some(info) = self.tables.lookup_key(&ns, local) else {
            return Ok(()); // missing target is handled by check_ref
        };
        if !info.is_key_or_unique {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "keyref '{}' must refer to a key or unique constraint, but '{}' is a keyref",
                    ic.name, refer
                ),
            }
            .into());
        }
        if info.field_count != ic.fields.len() {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "keyref '{}' has {} field(s) but referred constraint '{}' has {}",
                    ic.name,
                    ic.fields.len(),
                    refer,
                    info.field_count
                ),
            }
            .into());
        }
        Ok(())
    }

    fn check_attribute(&self, attr: &XsdAttribute, from: &str) -> Result<()> {
        if let Some(r) = &attr.ref_ {
            self.check_ref(RefKind::Attribute, r, from)?;
        }
        if let Some(t) = &attr.type_ref {
            self.check_ref(RefKind::Type, t, from)?;
            self.require_simple_type(t, "attribute type")?;
        }
        if let Some(inline) = &attr.inline_type {
            self.check_simple_type(inline)?;
        }
        Ok(())
    }

    fn check_type_def(&self, type_def: &XsdTypeDef) -> Result<()> {
        match type_def {
            XsdTypeDef::Simple(st) => self.check_simple_type(st),
            XsdTypeDef::Complex(ct) => self.check_complex_type(ct),
        }
    }

    fn check_simple_type(&self, st: &XsdSimpleType) -> Result<()> {
        let from = format!(
            "simple type '{}'",
            st.name.as_deref().unwrap_or("(anonymous)")
        );
        match &st.content {
            XsdSimpleTypeContent::Restriction(r) => {
                if let Some(base) = &r.base {
                    self.check_ref(RefKind::Type, base, &from)?;
                    self.require_simple_type(base, "simple type restriction base")?;
                    self.check_final(base, DerivationType::Restriction, &from)?;
                }
                if let Some(inline) = &r.inline_base {
                    self.check_simple_type(inline)?;
                }
            }
            XsdSimpleTypeContent::List(list) => {
                if let Some(item) = &list.item_type {
                    self.check_ref_lazy(RefKind::Type, item, &from)?;
                    self.require_simple_type(item, "list item type")?;
                    self.check_final(item, DerivationType::List, &from)?;
                }
                if let Some(inline) = &list.inline_type {
                    self.check_simple_type(inline)?;
                }
            }
            XsdSimpleTypeContent::Union(union) => {
                for member in &union.member_types {
                    self.check_ref_lazy(RefKind::Type, member, &from)?;
                    self.require_simple_type(member, "union member type")?;
                    self.check_final(member, DerivationType::Union, &from)?;
                }
                for inline in &union.inline_types {
                    self.check_simple_type(inline)?;
                }
            }
        }
        Ok(())
    }

    fn check_complex_type(&self, ct: &XsdComplexType) -> Result<()> {
        let from = format!(
            "complex type '{}'",
            ct.name.as_deref().unwrap_or("(anonymous)")
        );
        match &ct.content {
            XsdComplexContent::Empty => {}
            XsdComplexContent::Particle(p) => self.check_particle(p, &from)?,
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => {
                    self.check_ref(RefKind::Type, &ext.base, &from)?;
                    self.check_final(&ext.base, DerivationType::Extension, &from)?;
                    for attr in &ext.attributes {
                        self.check_attribute(attr, &from)?;
                    }
                    for ag in &ext.attribute_groups {
                        self.check_ref(RefKind::AttributeGroup, ag, &from)?;
                    }
                }
                XsdSimpleContentDerivation::Restriction(r) => {
                    self.check_ref(RefKind::Type, &r.base, &from)?;
                    self.check_final(&r.base, DerivationType::Restriction, &from)?;
                    for attr in &r.attributes {
                        self.check_attribute(attr, &from)?;
                    }
                    for ag in &r.attribute_groups {
                        self.check_ref(RefKind::AttributeGroup, ag, &from)?;
                    }
                }
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => {
                    self.check_ref(RefKind::Type, &ext.base, &from)?;
                    self.require_complex_type(&ext.base, "complexContent extension base")?;
                    self.check_final(&ext.base, DerivationType::Extension, &from)?;
                    if let Some(p) = &ext.particle {
                        self.check_particle(p, &from)?;
                    }
                    for attr in &ext.attributes {
                        self.check_attribute(attr, &from)?;
                    }
                    for ag in &ext.attribute_groups {
                        self.check_ref(RefKind::AttributeGroup, ag, &from)?;
                    }
                }
                XsdComplexContentDerivation::Restriction(r) => {
                    self.check_ref(RefKind::Type, &r.base, &from)?;
                    self.require_complex_type(&r.base, "complexContent restriction base")?;
                    self.check_final(&r.base, DerivationType::Restriction, &from)?;
                    if let Some(p) = &r.particle {
                        self.check_particle(p, &from)?;
                    }
                    for attr in &r.attributes {
                        self.check_attribute(attr, &from)?;
                    }
                    for ag in &r.attribute_groups {
                        self.check_ref(RefKind::AttributeGroup, ag, &from)?;
                    }
                }
            },
        }
        for attr in &ct.attributes {
            self.check_attribute(attr, &from)?;
        }
        for ag in &ct.attribute_groups {
            self.check_ref(RefKind::AttributeGroup, ag, &from)?;
        }
        Ok(())
    }

    fn check_attribute_group_body(&self, ag: &XsdAttributeGroup, from: &str) -> Result<()> {
        if let Some(r) = &ag.ref_ {
            self.check_ref(RefKind::AttributeGroup, r, from)?;
        }
        for attr in &ag.attributes {
            self.check_attribute(attr, from)?;
        }
        for nested in &ag.attribute_groups {
            self.check_ref(RefKind::AttributeGroup, nested, from)?;
        }
        Ok(())
    }

    fn check_particle(&self, particle: &XsdParticle, from: &str) -> Result<()> {
        match particle {
            XsdParticle::Sequence(seq) => {
                for item in &seq.particles {
                    self.check_item(item, from)?;
                }
            }
            XsdParticle::Choice(choice) => {
                for item in &choice.particles {
                    self.check_item(item, from)?;
                }
            }
            XsdParticle::All(all) => {
                for element in &all.elements {
                    self.check_element(element, from)?;
                }
            }
            XsdParticle::GroupRef(group_ref) => {
                self.check_ref(RefKind::Group, &group_ref.name, from)?;
            }
            XsdParticle::Any(_) => {}
        }
        Ok(())
    }

    fn check_item(&self, item: &XsdParticleItem, from: &str) -> Result<()> {
        match item {
            XsdParticleItem::Element(element) => self.check_element(element, from),
            XsdParticleItem::Sequence(seq) => {
                for nested in &seq.particles {
                    self.check_item(nested, from)?;
                }
                Ok(())
            }
            XsdParticleItem::Choice(choice) => {
                for nested in &choice.particles {
                    self.check_item(nested, from)?;
                }
                Ok(())
            }
            XsdParticleItem::GroupRef(group_ref) => {
                self.check_ref(RefKind::Group, &group_ref.name, from)
            }
            XsdParticleItem::Any(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::Schema;

    fn assert_dangling(xsd: &str, needle: &str) {
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("schema should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("undeclared") || msg.contains("reference"),
            "expected dangling-reference error, got: {msg}"
        );
        assert!(
            msg.contains(needle),
            "error should mention '{needle}': {msg}"
        );
    }

    #[test]
    fn dangling_group_ref_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:group ref="noSuchGroup"/>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchGroup");
    }

    #[test]
    fn dangling_element_ref_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element ref="noSuchElement"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchElement");
    }

    #[test]
    fn dangling_local_type_ref_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="child" type="NoSuchType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;
        assert_dangling(xsd, "NoSuchType");
    }

    #[test]
    fn top_level_dangling_type_tolerated() {
        // Lazy component resolution: a dangling type on a top-level element
        // is only an error when the element is used (saxonData/Missing).
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="unused" type="NoSuchType"/>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("lazy resolution tolerates unused dangling type");
    }

    #[test]
    fn dangling_xsd_builtin_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root" type="xs:noSuchBuiltin"/>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchBuiltin");
    }

    #[test]
    fn dangling_base_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="t">
                <xs:restriction base="NoSuchBase"/>
            </xs:simpleType>
        </xs:schema>"#;
        assert_dangling(xsd, "NoSuchBase");
    }

    #[test]
    fn dangling_item_type_tolerated_but_xsd_ns_rejected() {
        // Lazy: a dangling itemType is tolerated (saxonData/Missing/missing006)…
        let lazy = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="t">
                <xs:list itemType="NoSuchItem"/>
            </xs:simpleType>
        </xs:schema>"#;
        Schema::from_xsd(lazy.as_bytes()).expect("lazy resolution tolerates dangling itemType");
        // …but a miss in the closed XSD namespace can never resolve.
        let impossible = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="t">
                <xs:list itemType="xs:noSuchItem"/>
            </xs:simpleType>
        </xs:schema>"#;
        assert_dangling(impossible, "noSuchItem");
    }

    #[test]
    fn dangling_member_types_in_xsd_ns_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="t">
                <xs:union memberTypes="xs:string xs:timeDuration"/>
            </xs:simpleType>
        </xs:schema>"#;
        assert_dangling(xsd, "timeDuration");
    }

    #[test]
    fn dangling_attribute_group_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="t">
                <xs:attributeGroup ref="noSuchAttrGroup"/>
            </xs:complexType>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchAttrGroup");
    }

    #[test]
    fn dangling_attribute_ref_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="t">
                <xs:attribute ref="noSuchAttr"/>
            </xs:complexType>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchAttr");
    }

    #[test]
    fn dangling_substitution_group_tolerated() {
        // Lazy resolution (saxonData/Missing/missing002): only an error when
        // the head is needed for validation.
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="e" substitutionGroup="noSuchHead" type="xs:string"/>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("lazy resolution tolerates dangling head");
    }

    #[test]
    fn whitespace_in_qname_is_collapsed() {
        // QName attribute values are whitespace-collapsed (msData addB106).
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="t">
                <xs:restriction base="   xs:string "/>
            </xs:simpleType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("whitespace around QNames is not significant");
    }

    #[test]
    fn dangling_keyref_refer_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root" type="xs:string">
                <xs:keyref name="kr" refer="noSuchKey">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@b"/>
                </xs:keyref>
            </xs:element>
        </xs:schema>"#;
        assert_dangling(xsd, "noSuchKey");
    }

    #[test]
    fn keyref_field_count_mismatch_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:complexType>
                <xs:key name="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                    <xs:field xpath="@y"/>
                </xs:key>
                <xs:keyref name="kr" refer="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:keyref>
            </xs:element>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("field counts must match");
        assert!(err.to_string().contains("field"), "got: {err}");
    }

    #[test]
    fn keyref_referring_to_keyref_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:complexType>
                <xs:key name="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:key>
                <xs:keyref name="kr1" refer="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:keyref>
                <xs:keyref name="kr2" refer="kr1">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:keyref>
            </xs:element>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("refer to keyref is invalid");
        assert!(err.to_string().contains("keyref"), "got: {err}");
    }

    #[test]
    fn matching_keyref_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:complexType>
                <xs:key name="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:key>
                <xs:keyref name="kr" refer="k">
                    <xs:selector xpath="a"/>
                    <xs:field xpath="@x"/>
                </xs:keyref>
            </xs:element>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("valid keyref must compile");
    }

    #[test]
    fn final_restriction_blocks_simple_restriction() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="base" final="restriction">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
            <xs:simpleType name="derived">
                <xs:restriction base="base"/>
            </xs:simpleType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("final restriction");
        assert!(err.to_string().contains("final"), "got: {err}");
    }

    #[test]
    fn final_extension_blocks_complex_extension() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="base" final="extension">
                <xs:sequence/>
            </xs:complexType>
            <xs:complexType name="derived">
                <xs:complexContent>
                    <xs:extension base="base">
                        <xs:sequence/>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("final extension");
        assert!(err.to_string().contains("final"), "got: {err}");
    }

    #[test]
    fn final_all_blocks_list_and_union() {
        let list = r##"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="item" final="#all">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
            <xs:simpleType name="l">
                <xs:list itemType="item"/>
            </xs:simpleType>
        </xs:schema>"##;
        assert!(Schema::from_xsd(list.as_bytes()).is_err(), "final list");

        let union = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="member" final="union">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
            <xs:simpleType name="u">
                <xs:union memberTypes="member xs:int"/>
            </xs:simpleType>
        </xs:schema>"#;
        assert!(Schema::from_xsd(union.as_bytes()).is_err(), "final union");
    }

    #[test]
    fn final_does_not_block_other_derivations() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="base" final="restriction">
                <xs:sequence/>
            </xs:complexType>
            <xs:complexType name="derived">
                <xs:complexContent>
                    <xs:extension base="base">
                        <xs:sequence/>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
            <xs:simpleType name="s" final="list union">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
            <xs:simpleType name="s2">
                <xs:restriction base="s"/>
            </xs:simpleType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("unblocked derivations must compile");
    }

    #[test]
    fn circular_type_derivation_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="a">
                <xs:complexContent>
                    <xs:extension base="b">
                        <xs:sequence/>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
            <xs:complexType name="b">
                <xs:complexContent>
                    <xs:extension base="a">
                        <xs:sequence/>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular derivation");
        assert!(err.to_string().contains("circular"), "got: {err}");
    }

    #[test]
    fn recursive_content_is_not_a_derivation_cycle() {
        // An element of its own containing type is legal recursion.
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="node">
                <xs:sequence>
                    <xs:element name="child" type="node" minOccurs="0"/>
                </xs:sequence>
            </xs:complexType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("recursive content must compile");
    }

    #[test]
    fn circular_attribute_group_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:attributeGroup name="a">
                <xs:attributeGroup ref="b"/>
            </xs:attributeGroup>
            <xs:attributeGroup name="b">
                <xs:attributeGroup ref="a"/>
            </xs:attributeGroup>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular attributeGroup");
        assert!(err.to_string().contains("circular"), "got: {err}");
    }

    #[test]
    fn circular_group_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:group name="g1">
                <xs:sequence>
                    <xs:group ref="g2"/>
                </xs:sequence>
            </xs:group>
            <xs:group name="g2">
                <xs:sequence>
                    <xs:group ref="g1"/>
                </xs:sequence>
            </xs:group>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular group");
        assert!(err.to_string().contains("circular"), "got: {err}");
    }

    #[test]
    fn acyclic_groups_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:group name="g1">
                <xs:sequence>
                    <xs:group ref="g2"/>
                </xs:sequence>
            </xs:group>
            <xs:group name="g2">
                <xs:sequence>
                    <xs:element name="e" type="xs:string"/>
                </xs:sequence>
            </xs:group>
            <xs:attributeGroup name="a">
                <xs:attributeGroup ref="b"/>
            </xs:attributeGroup>
            <xs:attributeGroup name="b">
                <xs:attribute name="x" type="xs:string"/>
            </xs:attributeGroup>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("acyclic groups must compile");
    }

    #[test]
    fn attribute_with_complex_type_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="CT">
                <xs:sequence/>
            </xs:complexType>
            <xs:attribute name="a" type="CT"/>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("attribute type must be simple");
        assert!(err.to_string().contains("simple"), "got: {err}");
    }

    #[test]
    fn simple_restriction_of_complex_type_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="CT">
                <xs:sequence/>
            </xs:complexType>
            <xs:simpleType name="st">
                <xs:restriction base="CT"/>
            </xs:simpleType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("simple restriction of complex");
        assert!(err.to_string().contains("simple"), "got: {err}");
    }

    #[test]
    fn complex_content_base_must_be_complex() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="CT">
                <xs:complexContent>
                    <xs:extension base="xs:string">
                        <xs:sequence/>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("complexContent base simple");
        assert!(err.to_string().contains("complex"), "got: {err}");
    }

    #[test]
    fn undeclared_prefix_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="root" type="undeclared:Type"/>
        </xs:schema>"#;
        assert_dangling(xsd, "undeclared:Type");
    }

    #[test]
    fn ref_into_unloaded_import_accepted() {
        // The import has no schemaLocation, so its namespace is poisoned and
        // references into it stay lenient (real-world partial schema sets).
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:other="http://example.com/other">
            <xs:import namespace="http://example.com/other"/>
            <xs:element name="root" type="other:SomeType"/>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("lenient toward unresolved imports");
    }

    #[test]
    fn ref_into_own_ns_with_include_accepted() {
        // xs:include is present but its document was never fetched (single-doc
        // compilation): the schema's own namespace is poisoned.
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:include schemaLocation="other.xsd"/>
            <xs:element name="root" type="TypeFromInclude"/>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("lenient when includes are unresolved");
    }

    #[test]
    fn valid_same_ns_refs_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:tns="http://example.com/t" targetNamespace="http://example.com/t">
            <xs:element name="root" type="tns:RootType"/>
            <xs:complexType name="RootType">
                <xs:sequence>
                    <xs:element ref="tns:child"/>
                    <xs:group ref="tns:g"/>
                </xs:sequence>
                <xs:attributeGroup ref="tns:ag"/>
            </xs:complexType>
            <xs:element name="child" type="xs:string"/>
            <xs:group name="g">
                <xs:sequence>
                    <xs:element name="x" type="xs:string"/>
                </xs:sequence>
            </xs:group>
            <xs:attributeGroup name="ag">
                <xs:attribute name="a" type="xs:string"/>
            </xs:attributeGroup>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("valid schema must compile");
    }

    #[test]
    fn unprefixed_ref_falls_back_to_target_namespace() {
        // Mirrors the compiler's leniency: unprefixed references resolve
        // against the target namespace even without a default xmlns.
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://example.com/t">
            <xs:element name="root" type="RootType"/>
            <xs:complexType name="RootType">
                <xs:sequence/>
            </xs:complexType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("unprefixed same-ns ref must compile");
    }

    #[test]
    fn xml_namespace_refs_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="t">
                <xs:attribute ref="xml:lang"/>
            </xs:complexType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("xml namespace is always lenient");
    }
}
