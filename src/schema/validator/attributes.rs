//! Shared attribute validation logic for the DOM and streaming validators.

use crate::schema::types::{
    AttributeDef, CompiledSchema, ComplexType, ContentModel, SimpleType, TypeDef,
};
use crate::schema::xsd::facets::{FacetCache, FacetConstraints, FacetValidator};
use crate::schema::xsd::primitive::PrimitiveKind;

/// The attribute picture of a complex type after walking its derivation
/// chain: the (shadow-resolved) declarations, the effective attribute
/// wildcard, and whether the type is `xs:anyType`.
///
/// C7: building this walks the base chain 4-6 levels deep and allocates a
/// Vec, far too expensive to repeat per element. The streaming validator
/// memoizes it per named type; anonymous types build it fresh.
pub(crate) struct CollectedAttrs {
    pub defs: Vec<AttributeDef>,
    pub wildcard: Option<crate::schema::types::WildcardConstraint>,
    pub is_any_type: bool,
}

impl CollectedAttrs {
    pub(crate) fn collect(schema: &CompiledSchema, complex: &ComplexType) -> Self {
        Self {
            defs: collect_attributes(schema, complex)
                .into_iter()
                .cloned()
                .collect(),
            wildcard: collect_attr_wildcard(schema, complex).cloned(),
            is_any_type: complex.name == "anyType",
        }
    }
}

/// Collects the attribute declarations of a complex type, walking the
/// derivation base chain. A declaration in a derived type shadows a base
/// declaration with the same name.
pub(crate) fn collect_attributes<'a>(
    schema: &'a CompiledSchema,
    complex: &'a ComplexType,
) -> Vec<&'a AttributeDef> {
    let mut out: Vec<&AttributeDef> = Vec::new();
    let mut current = complex;
    for _ in 0..16 {
        for attr in &current.attributes {
            if !out.iter().any(|a| a.name == attr.name) {
                out.push(attr);
            }
        }
        // C4: ns-first base hop (compile-time resolved base_ns), string
        // fallback inside complex_base_def.
        match schema.complex_base_def(current) {
            Some(TypeDef::Complex(c)) => current = c,
            _ => break,
        }
    }
    out
}

/// Returns the attribute wildcard (xs:anyAttribute) in effect for a complex
/// type, walking the derivation base chain (nearest declaration wins).
fn collect_attr_wildcard<'a>(
    schema: &'a CompiledSchema,
    complex: &'a ComplexType,
) -> Option<&'a crate::schema::types::WildcardConstraint> {
    let mut current = complex;
    for _ in 0..16 {
        if let Some(ref w) = current.attr_wildcard {
            return Some(w);
        }
        match schema.complex_base_def(current) {
            Some(TypeDef::Complex(c)) => current = c,
            _ => break,
        }
    }
    None
}

/// Resolves the attribute declaration an `AttributeDef` ultimately refers
/// to: a `ref` is followed to the global attribute declaration.
fn resolve_ref<'a>(schema: &'a CompiledSchema, attr: &'a AttributeDef) -> &'a AttributeDef {
    if !attr.is_ref {
        return attr;
    }
    // C4: the resolved reference namespace probes the collision-free map
    // first; the legacy string probes remain as fallback.
    if let Some(rn) = &attr.ref_ns
        && let Some(global) = schema.attribute_ns(&rn.namespace_uri, &rn.local_name)
    {
        return global;
    }
    if let Some(global) = schema.attributes.get(&attr.name) {
        return global;
    }
    // The ref may have been stored with a prefix in the global table.
    if let Some((_, found)) = schema
        .attributes
        .iter()
        .find(|(k, _)| k.rsplit(':').next() == Some(attr.name.as_str()))
    {
        return found;
    }
    attr
}

/// Resolves the simple type governing an attribute's value.
fn attribute_simple_type<'a>(
    schema: &'a CompiledSchema,
    attr: &'a AttributeDef,
) -> Option<&'a SimpleType> {
    if let Some(ref inline) = attr.inline_type {
        return Some(inline);
    }
    let type_ref = attr.type_ref.as_deref()?;
    match schema.type_by_ref(attr.type_ns.as_ref(), type_ref) {
        Some(TypeDef::Simple(s)) => Some(s),
        _ => None,
    }
}

/// Resolves the [`PrimitiveKind`] governing the value space of the attribute
/// named `attr_local` on `complex` (following the derivation chain), so
/// identity-constraint field values carried by attributes can be canonicalized
/// before comparison. Returns `None` for string-derived or undeclared
/// attributes, in which case lexical comparison is used.
pub(crate) fn attribute_primitive_kind(
    schema: &CompiledSchema,
    complex: &ComplexType,
    attr_local: &str,
) -> Option<PrimitiveKind> {
    let def = collect_attributes(schema, complex)
        .into_iter()
        .find(|d| d.name == attr_local || d.name.rsplit(':').next() == Some(attr_local))?;
    let def = resolve_ref(schema, def);
    let simple = attribute_simple_type(schema, def)?;
    PrimitiveKind::resolve(schema, simple)
}

/// Resolves the [`PrimitiveKind`] governing an element's text value: its
/// simple type, or the base of a `simpleContent` complex type. Returns `None`
/// when the element has no simple value space (element-only content).
pub(crate) fn element_text_primitive_kind(
    schema: &CompiledSchema,
    type_def: &TypeDef,
) -> Option<PrimitiveKind> {
    match type_def {
        TypeDef::Simple(simple) => PrimitiveKind::resolve(schema, simple),
        TypeDef::Complex(complex) => {
            if matches!(&complex.content, ContentModel::SimpleContent { .. })
                && let Some(TypeDef::Simple(simple)) = schema.complex_base_def(complex)
            {
                return PrimitiveKind::resolve(schema, simple);
            }
            None
        }
    }
}

/// Validates one attribute value against its declaration.
///
/// Returns an error message when the value is invalid, `None` when valid or
/// when the declaration carries no usable type information.
pub(crate) fn validate_attribute_value(
    schema: &CompiledSchema,
    attr: &AttributeDef,
    value: &str,
    cache: &mut FacetCache,
) -> Option<String> {
    let attr = resolve_ref(schema, attr);

    if let Some(ref fixed) = attr.fixed {
        if value != fixed {
            return Some(format!(
                "attribute '{}' must have the fixed value '{}', found '{}'",
                attr.name, fixed, value
            ));
        }
    }

    let simple = attribute_simple_type(schema, attr)?;

    let constraints = cache.get(schema, simple);
    let validator = FacetValidator::new(&constraints);
    if let Err(e) = validator.validate(value) {
        return Some(format!("attribute '{}': {}", attr.name, e));
    }

    if let Some(kind) = PrimitiveKind::resolve(schema, simple) {
        if let Err(e) = kind.validate(value) {
            return Some(format!("attribute '{}': {}", attr.name, e));
        }
    }

    None
}

/// True for attributes that are not subject to schema validation
/// (namespace declarations and `xsi:*` control attributes).
pub(crate) fn is_exempt_attribute(name: &str) -> bool {
    name == "xmlns"
        || name.starts_with("xmlns:")
        || name.starts_with("xsi:")
        || name.contains("schemaLocation")
}

/// Outcome of validating an element's attributes: error messages plus the
/// `xs:ID` / `xs:IDREF` values found, for document-level checking.
#[derive(Default)]
pub(crate) struct AttrValidation {
    pub errors: Vec<String>,
    pub ids: Vec<String>,
    pub idrefs: Vec<String>,
}

/// The XML namespace: `xml:lang`, `xml:space`, `xml:base`, `xml:id` are
/// always allowed without declaration.
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Validates an element's attributes against the attribute declarations of
/// its complex type. Returns error messages for invalid, undeclared, or
/// missing-required attributes, plus any ID/IDREF values for document-level
/// checks.
///
/// `attributes` yields `(name, namespace, value)` triples; the namespace is
/// used for `xs:anyAttribute` wildcard matching.
pub(crate) fn validate_element_attributes<'a>(
    schema: &CompiledSchema,
    collected: &CollectedAttrs,
    attributes: impl Iterator<Item = (&'a str, Option<&'a str>, &'a str)> + Clone,
    cache: &mut FacetCache,
) -> AttrValidation {
    let defs = &collected.defs;
    let wildcard = collected.wildcard.as_ref();
    let mut out = AttrValidation::default();

    // xs:anyType places no constraints on attributes.
    if collected.is_any_type {
        return out;
    }

    for (name, ns, value) in attributes.clone() {
        if is_exempt_attribute(name) || ns == Some(XML_NS) {
            continue;
        }
        let local = name.rsplit(':').next().unwrap_or(name);
        if let Some(def) = defs.iter().find(|d| d.name == local || d.name == name) {
            if let Some(msg) = validate_attribute_value(schema, def, value, cache) {
                out.errors.push(msg);
            }
            collect_id_values(schema, resolve_ref(schema, def), value, &mut out, cache);
            continue;
        }

        // Undeclared attribute: admitted only by a matching wildcard.
        match wildcard {
            Some(w) if w.matches(ns) => match w.process_contents {
                crate::schema::types::ProcessContents::Skip => {}
                crate::schema::types::ProcessContents::Lax
                | crate::schema::types::ProcessContents::Strict => {
                    let global = schema
                        .attributes
                        .get(name)
                        .or_else(|| schema.attributes.get(local));
                    match global {
                        Some(def) => {
                            if let Some(msg) = validate_attribute_value(schema, def, value, cache) {
                                out.errors.push(msg);
                            }
                            collect_id_values(schema, def, value, &mut out, cache);
                        }
                        None if w.process_contents
                            == crate::schema::types::ProcessContents::Strict =>
                        {
                            out.errors.push(format!(
                                "attribute '{}' matched a strict wildcard but is not declared",
                                name
                            ));
                        }
                        None => {}
                    }
                }
            },
            _ => {
                out.errors
                    .push(format!("attribute '{}' is not allowed", name));
            }
        }
    }

    for def in defs {
        if def.required
            && !attributes
                .clone()
                .any(|(n, _, _)| n.rsplit(':').next().unwrap_or(n) == def.name || n == def.name)
        {
            out.errors
                .push(format!("required attribute '{}' is missing", def.name));
        }
    }

    out
}

/// Records ID / IDREF / IDREFS values carried by an attribute.
fn collect_id_values(
    schema: &CompiledSchema,
    attr: &AttributeDef,
    value: &str,
    out: &mut AttrValidation,
    cache: &mut FacetCache,
) {
    let Some(simple) = attribute_simple_type(schema, attr) else {
        // No resolvable type, but xml:id-style direct refs are rare; also
        // cover the common case of `type="xs:ID"` on the def itself.
        let kind = attr
            .type_ref
            .as_deref()
            .and_then(PrimitiveKind::from_type_name);
        push_id_values(kind, None, false, value, out);
        return;
    };
    let constraints = cache.get(schema, simple);
    push_id_values_from_constraints(&constraints, value, out);
}

/// Records ID / IDREF / IDREFS values described by compiled facet
/// constraints (used for both attribute and element content values).
pub(crate) fn push_id_values_from_constraints(
    constraints: &FacetConstraints,
    value: &str,
    out: &mut AttrValidation,
) {
    push_id_values(
        constraints.value_kind,
        constraints.item_kind,
        constraints.is_list,
        value,
        out,
    );
}

fn push_id_values(
    kind: Option<PrimitiveKind>,
    item_kind: Option<PrimitiveKind>,
    is_list: bool,
    value: &str,
    out: &mut AttrValidation,
) {
    if is_list {
        match item_kind {
            Some(k) if k.is_idref() => out
                .idrefs
                .extend(value.split_whitespace().map(str::to_string)),
            Some(k) if k.is_id() => out.ids.extend(value.split_whitespace().map(str::to_string)),
            _ => {}
        }
        return;
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return; // empty values are already lexical errors for ID/IDREF
    }
    match kind {
        Some(k) if k.is_id() => out.ids.push(trimmed.to_string()),
        Some(k) if k.is_idref() => out.idrefs.push(trimmed.to_string()),
        _ => {}
    }
}
