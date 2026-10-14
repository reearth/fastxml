//! rcase-NameAndTypeOK: one element declaration restricting another.

use crate::schema::types::{CompiledSchema, ElementDef, TypeDef};

use super::Verdict::{self, Ok as VOk, Unknown, Violation};
use super::{builtin_parent, local_name};

/// rcase-NameAndTypeOK for a pair of element declarations.
pub(super) fn element_restrict(
    de: &ElementDef,
    be: &ElementDef,
    schema: &CompiledSchema,
) -> Verdict {
    if local_name(&de.name) != local_name(&be.name) {
        // The derived element may be a member of the base element's
        // substitution group; that resolution is out of subset.
        if let Some(members) = schema
            .transitive_substitution_groups
            .get(&be.name)
            .or_else(|| {
                schema
                    .transitive_substitution_groups
                    .get(local_name(&be.name))
            })
            && members
                .iter()
                .any(|m| local_name(m) == local_name(&de.name))
        {
            return Unknown;
        }
        return Violation(format!(
            "element '{}' does not match base element '{}'",
            de.name, be.name
        ));
    }
    if let (Some(dns), Some(bns)) = (known_namespace(de, schema), known_namespace(be, schema))
        && dns != bns
    {
        return Violation(format!(
            "element '{}' is in namespace '{dns}', the base element in '{bns}'",
            de.name
        ));
    }
    if de.min_occurs < be.min_occurs {
        return Violation(format!(
            "element '{}' minOccurs {} is below the base's {}",
            de.name, de.min_occurs, be.min_occurs
        ));
    }
    match (de.max_occurs, be.max_occurs) {
        (_, None) => {}
        (None, Some(_)) => {
            return Violation(format!(
                "element '{}' is unbounded but the base is not",
                de.name
            ));
        }
        (Some(dm), Some(bm)) if dm > bm => {
            return Violation(format!(
                "element '{}' maxOccurs {} exceeds the base's {}",
                de.name, dm, bm
            ));
        }
        _ => {}
    }
    // nillable: a restriction cannot become nillable when the base is not.
    if de.nillable && !be.nillable {
        return Violation(format!(
            "element '{}' is nillable but the base element is not",
            de.name
        ));
    }
    match fixed_ok(de, be, schema) {
        VOk => type_derivation_ok(de, be, schema),
        other => other,
    }
}

/// The namespace of an element declaration, when it is certain: a local
/// declaration's, or a reference's whose target is in this schema (an
/// unresolved reference has a guessed one).
pub(super) fn known_namespace<'a>(e: &'a ElementDef, schema: &CompiledSchema) -> Option<&'a str> {
    if let Some(r) = &e.ref_ns
        && !schema.elements_ns.contains_key(r)
    {
        return None;
    }
    e.namespace.as_deref()
}

/// rcase-NameAndTypeOK clause 4: when the base element has a fixed value,
/// the restriction must have the same one.
fn fixed_ok(de: &ElementDef, be: &ElementDef, schema: &CompiledSchema) -> Verdict {
    let Some(base_fixed) = &be.fixed else {
        return VOk;
    };
    let Some(derived_fixed) = &de.fixed else {
        return Violation(format!(
            "element '{}' drops the base element's fixed value",
            de.name
        ));
    };
    if derived_fixed == base_fixed {
        return VOk;
    }
    // Different literals can still be the same value: compare in the value
    // space of the element's simple type. String types compare by their
    // whitespace-collapsed forms, which differ for different values under
    // any whiteSpace. Complex types are out of subset.
    use crate::schema::xsd::primitive::PrimitiveKind;
    let simple = match be.type_ref.as_deref() {
        None => Some(None),
        Some(t) if matches!(local_name(t), "anyType" | "anySimpleType") => Some(None),
        Some(t) => match schema.get_type(t) {
            Some(TypeDef::Simple(st)) => Some(PrimitiveKind::resolve(schema, st)),
            Some(TypeDef::Complex(_)) => None,
            None => PrimitiveKind::from_type_name(local_name(t))
                .map(Some)
                .or_else(|| {
                    matches!(local_name(t), "string" | "normalizedString" | "token").then_some(None)
                }),
        },
    };
    let same = match simple {
        Some(Some(kind)) => {
            crate::schema::xsd::value_compare::compare_values(Some(kind), derived_fixed, base_fixed)
                .map(|o| o == std::cmp::Ordering::Equal)
        }
        Some(None) => {
            let collapse = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
            (collapse(derived_fixed) != collapse(base_fixed)).then_some(false)
        }
        None => None,
    };
    match same {
        Some(true) => VOk,
        Some(false) => Violation(format!(
            "element '{}' fixed value '{derived_fixed}' differs from the base's '{base_fixed}'",
            de.name
        )),
        None => Unknown,
    }
}

/// Whether the derived element's type is (transitively) derived from — or
/// identical to — the base element's type.
fn type_derivation_ok(de: &ElementDef, be: &ElementDef, schema: &CompiledSchema) -> Verdict {
    // Nothing can name the base element's anonymous type, so no other
    // type derives from it: the restriction's type is a different one.
    if be.inline_type.is_some() {
        return Violation(format!(
            "element '{}' cannot restrict the base element's anonymous type",
            de.name
        ));
    }
    let Some(base_type) = &be.type_ref else {
        return VOk; // base is anyType: everything derives from it
    };
    let base_local = local_name(base_type);
    if base_local == "anyType" || base_local == "anySimpleType" {
        return VOk;
    }
    // An anonymous derived type would need its base chain walked.
    if de.inline_type.is_some() {
        return Unknown;
    }
    let Some(derived_type) = &de.type_ref else {
        // Derived element falls back to anyType, which cannot restrict a
        // named base type.
        return Violation(format!(
            "element '{}' drops the base element's type '{}'",
            de.name, base_type
        ));
    };
    // A union base type admits its member types (and types derived from
    // them); that resolution is out of the implemented subset.
    if let Some(TypeDef::Simple(base_def)) = schema.get_type(base_type)
        && !base_def.member_types.is_empty()
    {
        return Unknown;
    }
    // Walk the derived type's base chain looking for the base type.
    // Compilation runs before built-ins are registered, so hops through the
    // built-in hierarchy use a static parent table.
    let mut current = derived_type.clone();
    for _ in 0..32 {
        if local_name(&current) == base_local {
            return VOk;
        }
        let next = match schema.get_type(&current) {
            // A list or union (recorded as a `list(…)`/`union(…)` marker)
            // derives from anySimpleType, a complex type without a
            // derivation from anyType.
            Some(TypeDef::Simple(s)) => match s.base_type.as_deref() {
                Some(b) if !b.starts_with("list(") && !b.starts_with("union(") => {
                    Some(b.to_string())
                }
                _ => Some("anySimpleType".to_string()),
            },
            Some(TypeDef::Complex(c)) => {
                Some(c.base_type.clone().unwrap_or_else(|| "anyType".to_string()))
            }
            None => builtin_parent(local_name(&current)).map(str::to_string),
        };
        match next {
            Some(n) => current = n,
            None if crate::schema::xsd::builtin::is_builtin_xsd_type_local(local_name(
                &current,
            )) || local_name(&current) == "anyType" =>
            {
                // Chain fully walked to the built-in root without meeting
                // the base type.
                return Violation(format!(
                    "element '{}' type '{}' is not derived from the base element's type '{}'",
                    de.name, derived_type, base_type
                ));
            }
            None => return Unknown,
        }
    }
    Unknown
}
