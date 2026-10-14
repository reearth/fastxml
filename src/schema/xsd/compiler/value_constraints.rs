//! Value constraints checked against the full simple type (Element and
//! Attribute Declaration Properties Correct, clause 2): a `default` or
//! `fixed` value must satisfy every facet of its type, enumeration and
//! pattern included.
//!
//! The facets are those the validator applies. A value is rejected only
//! when it fails both as written and whitespace-collapsed, so a whiteSpace
//! facet the compile-time view may miss cannot cause a rejection.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::types::{
    AttributeDef, CompiledSchema, ElementDef, Particle, SimpleType, TypeDef,
};
use crate::schema::xsd::facets::{FacetConstraints, FacetValidator};

/// Whether `value` definitely fails the facets of `simple`.
fn fails(schema: &CompiledSchema, simple: &SimpleType, value: &str) -> bool {
    let constraints = FacetConstraints::from_simple_type(schema, simple);
    let validator = FacetValidator::new(&constraints);
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    validator.validate(value).is_err() && validator.validate(&collapsed).is_err()
}

fn element_simple_type<'a>(
    schema: &'a CompiledSchema,
    e: &'a ElementDef,
) -> Option<&'a SimpleType> {
    match &e.inline_type {
        Some(TypeDef::Simple(st)) => Some(st),
        Some(TypeDef::Complex(_)) => None,
        None => match schema.type_by_ref(e.type_ns.as_ref(), e.type_ref.as_deref()?) {
            Some(TypeDef::Simple(st)) => Some(st),
            _ => None,
        },
    }
}

fn attribute_simple_type<'a>(
    schema: &'a CompiledSchema,
    a: &'a AttributeDef,
) -> Option<&'a SimpleType> {
    if let Some(inline) = &a.inline_type {
        return Some(inline);
    }
    match schema.type_by_ref(a.type_ns.as_ref(), a.type_ref.as_deref()?) {
        Some(TypeDef::Simple(st)) => Some(st),
        _ => None,
    }
}

fn check(
    what: &str,
    name: &str,
    value: Option<&str>,
    simple: Option<&SimpleType>,
    schema: &CompiledSchema,
) -> Result<()> {
    if let (Some(value), Some(simple)) = (value, simple)
        && fails(schema, simple, value)
    {
        return Err(SchemaError::InvalidSchema {
            message: format!(
                "{what} '{name}': default/fixed value '{value}' is not valid for its type"
            ),
        }
        .into());
    }
    Ok(())
}

fn check_element(e: &ElementDef, schema: &CompiledSchema) -> Result<()> {
    let value = e.default.as_deref().or(e.fixed.as_deref());
    if value.is_none() {
        return Ok(());
    }
    check(
        "element",
        &e.name,
        value,
        element_simple_type(schema, e),
        schema,
    )
}

fn check_attribute(a: &AttributeDef, schema: &CompiledSchema) -> Result<()> {
    let value = a.default.as_deref().or(a.fixed.as_deref());
    if value.is_none() {
        return Ok(());
    }
    // Attribute Use Correct (clause 2): a use of a declaration with a fixed
    // value can only fix that same value.
    if let (Some(fixed), Some(global)) = (
        a.fixed.as_deref(),
        a.ref_ns.as_ref().and_then(|r| schema.attributes_ns.get(r)),
    ) && let Some(global_fixed) = global.fixed.as_deref()
        && differs(schema, global, fixed, global_fixed)
    {
        return Err(SchemaError::InvalidSchema {
            message: format!(
                "attribute '{}': fixed value '{fixed}' differs from the declaration's '{global_fixed}'",
                a.name
            ),
        }
        .into());
    }
    check(
        "attribute",
        &a.name,
        value,
        attribute_simple_type(schema, a),
        schema,
    )
}

/// Whether two values of the type `attr` declares are definitely different.
fn differs(schema: &CompiledSchema, attr: &AttributeDef, a: &str, b: &str) -> bool {
    use crate::schema::xsd::primitive::PrimitiveKind;
    use crate::schema::xsd::value_compare::compare_values;
    let collapse = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapse(a) == collapse(b) {
        return false;
    }
    let kind = match attribute_simple_type(schema, attr) {
        // Lists and unions are left alone.
        Some(st) if st.item_type.is_some() || !st.member_types.is_empty() => return false,
        Some(st) => PrimitiveKind::resolve(schema, st),
        // Built-in types are not in the type table while compiling.
        None => match attr
            .type_ref
            .as_deref()
            .map(|t| t.rsplit(':').next().unwrap_or(t))
        {
            None | Some("string" | "normalizedString" | "token" | "anySimpleType") => None,
            Some(local) => match PrimitiveKind::from_type_name(local) {
                Some(kind) => Some(kind),
                None => return false,
            },
        },
    };
    match kind {
        Some(kind) => compare_values(Some(kind), a, b).is_some_and(|o| o.is_ne()),
        None => true,
    }
}

fn check_particle(p: &Particle, schema: &CompiledSchema) -> Result<()> {
    match p {
        Particle::Element(e) => check_element(e, schema),
        Particle::All { elements, .. } => {
            elements.iter().try_for_each(|e| check_element(e, schema))
        }
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } => {
            items.iter().try_for_each(|i| check_particle(i, schema))
        }
        Particle::Wildcard(_) => Ok(()),
    }
}

/// Checks the value constraints of every element and attribute declaration
/// in `schema`.
pub(crate) fn check_value_constraints(schema: &CompiledSchema) -> Result<()> {
    for e in schema.elements_ns.values() {
        check_element(e, schema)?;
    }
    for a in schema.attributes_ns.values() {
        check_attribute(a, schema)?;
    }
    super::restriction::for_each_complex_type(schema, |ct, what| {
        for (i, a) in ct.attributes.iter().enumerate() {
            check_attribute(a, schema)?;
            // Two uses of the same attribute, e.g. one declared locally and
            // one from an attribute group.
            let same = |b: &AttributeDef| {
                !b.prohibited
                    && b.name.rsplit(':').next() == a.name.rsplit(':').next()
                    && b.namespace == a.namespace
            };
            if !a.prohibited && ct.attributes[..i].iter().any(same) {
                return Err(SchemaError::InvalidSchema {
                    message: format!("{what} uses attribute '{}' twice", a.name),
                }
                .into());
            }
        }
        match ct.particle.as_deref() {
            Some(p) => check_particle(p, schema),
            None => Ok(()),
        }
    })
}
