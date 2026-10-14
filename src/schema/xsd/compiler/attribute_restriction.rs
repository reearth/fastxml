//! Attribute uses under derivation by restriction (XSD 1.0
//! derivation-ok-restriction, clauses 2–4).
//!
//! A restriction cannot loosen a required attribute, change a fixed value,
//! add an attribute the base neither declares nor admits through its
//! wildcard, or widen the base's attribute wildcard. The base's attribute
//! uses are gathered along its derivation chain; when any part of that
//! chain is unknown, only the checks that do not need the whole set run.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::types::{
    AttributeDef, CompiledSchema, ComplexType, DerivationMethod, ProcessContents, TypeDef,
    WildcardConstraint,
};
use crate::schema::xsd::primitive::PrimitiveKind;
use crate::schema::xsd::value_compare::compare_values;

/// The attribute uses and wildcard a type has, with all of its base chain.
struct Effective {
    attributes: Vec<AttributeDef>,
    wildcard: Option<WildcardConstraint>,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Whether two attribute uses are for the same attribute.
fn same_attribute(a: &AttributeDef, b: &AttributeDef) -> bool {
    local(&a.name) == local(&b.name) && a.namespace == b.namespace
}

/// The attribute uses and wildcard of `ct` and its bases. `None` when part
/// of the chain is unknown (an unresolved base, or two wildcards whose
/// union is not computed).
fn effective(ct: &ComplexType, schema: &CompiledSchema, depth: usize) -> Option<Effective> {
    if depth > 16 {
        return None;
    }
    let own: Vec<AttributeDef> = ct
        .attributes
        .iter()
        .filter(|a| !a.prohibited)
        .cloned()
        .collect();
    let (Some(method), Some(base_name)) = (ct.derivation, ct.base_type.as_deref()) else {
        return Some(Effective {
            attributes: own,
            wildcard: ct.attr_wildcard.clone(),
        });
    };
    let base = match schema.type_by_ref(ct.base_ns.as_ref(), base_name) {
        Some(TypeDef::Complex(base)) if !std::ptr::eq(base, ct) => {
            effective(base, schema, depth + 1)?
        }
        // A simple base type has no attribute uses.
        Some(TypeDef::Simple(_)) => Effective {
            attributes: Vec::new(),
            wildcard: None,
        },
        _ if local(base_name) == "anyType" => Effective {
            attributes: Vec::new(),
            wildcard: None,
        },
        _ => return None,
    };
    let mut attributes: Vec<AttributeDef> = base
        .attributes
        .into_iter()
        .filter(|b| !ct.attributes.iter().any(|a| same_attribute(a, b)))
        .collect();
    attributes.extend(own);
    let wildcard = match method {
        DerivationMethod::Restriction => ct.attr_wildcard.clone(),
        DerivationMethod::Extension => match (&ct.attr_wildcard, base.wildcard) {
            (Some(_), Some(_)) => return None,
            (Some(w), None) => Some(w.clone()),
            (None, w) => w,
        },
    };
    Some(Effective {
        attributes,
        wildcard,
    })
}

/// Whether a reference's target is in this schema; otherwise its namespace
/// was guessed.
fn resolved(attr: &AttributeDef, schema: &CompiledSchema) -> bool {
    attr.ref_ns
        .as_ref()
        .is_none_or(|r| schema.attributes_ns.contains_key(r))
}

/// Whether two fixed values are the same value of `attr`'s type: `None`
/// when that cannot be told.
fn same_value(attr: &AttributeDef, a: &str, b: &str, schema: &CompiledSchema) -> Option<bool> {
    if a == b {
        return Some(true);
    }
    let kind = match attr.type_ref.as_deref() {
        None => None,
        Some(t) => match schema.get_type(t) {
            Some(TypeDef::Simple(st)) => PrimitiveKind::resolve(schema, st),
            Some(TypeDef::Complex(_)) => return None,
            None => match PrimitiveKind::from_type_name(local(t)) {
                Some(kind) => Some(kind),
                None if matches!(local(t), "string" | "normalizedString" | "token") => None,
                None => return None,
            },
        },
    };
    match kind {
        Some(kind) => compare_values(Some(kind), a, b).map(|o| o.is_eq()),
        None => {
            let collapse = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
            (collapse(a) != collapse(b)).then_some(false)
        }
    }
}

fn strength(p: ProcessContents) -> u8 {
    match p {
        ProcessContents::Skip => 0,
        ProcessContents::Lax => 1,
        ProcessContents::Strict => 2,
    }
}

fn invalid(what: &str, reason: String) -> Result<()> {
    Err(SchemaError::InvalidSchema {
        message: format!("{what} is not a valid restriction: {reason}"),
    }
    .into())
}

/// Checks the attribute uses of `ct`, described by `what`, against its
/// base when it is derived by restriction.
pub(crate) fn check_attribute_restriction(
    ct: &ComplexType,
    what: &str,
    schema: &CompiledSchema,
) -> Result<()> {
    if ct.derivation != Some(DerivationMethod::Restriction) {
        return Ok(());
    }
    let Some(base_name) = ct.base_type.as_deref() else {
        return Ok(());
    };
    if local(base_name) == "anyType" {
        return Ok(());
    }
    let Some(TypeDef::Complex(base)) = schema.type_by_ref(ct.base_ns.as_ref(), base_name) else {
        return Ok(());
    };
    if std::ptr::eq(base, ct) {
        return Ok(());
    }
    let Some(base) = effective(base, schema, 0) else {
        return Ok(());
    };

    for d in &ct.attributes {
        let name = local(&d.name);
        let Some(b) = base.attributes.iter().find(|b| same_attribute(d, b)) else {
            if d.prohibited || !resolved(d, schema) {
                continue;
            }
            let ns = d.namespace.as_deref().filter(|n| !n.is_empty());
            match &base.wildcard {
                Some(w) if w.matches(ns) => {}
                Some(_) => {
                    return invalid(
                        what,
                        format!("attribute '{name}' is not allowed by the base's wildcard"),
                    );
                }
                None => {
                    return invalid(what, format!("attribute '{name}' is not in the base type"));
                }
            }
            continue;
        };
        if b.required && (d.prohibited || !d.required) {
            return invalid(
                what,
                format!("attribute '{name}' is required in the base type"),
            );
        }
        if d.prohibited {
            continue;
        }
        if let Some(base_fixed) = &b.fixed {
            let same = match &d.fixed {
                None => Some(false),
                Some(derived_fixed) => same_value(b, derived_fixed, base_fixed, schema),
            };
            if same == Some(false) {
                return invalid(
                    what,
                    format!("attribute '{name}' must keep the base's fixed value '{base_fixed}'"),
                );
            }
        }
    }

    if let Some(dw) = &ct.attr_wildcard {
        let Some(bw) = &base.wildcard else {
            return invalid(what, "the base type has no attribute wildcard".to_string());
        };
        if strength(dw.process_contents) < strength(bw.process_contents) {
            return invalid(
                what,
                "the attribute wildcard's processContents is weaker than the base's".to_string(),
            );
        }
        use crate::schema::types::WildcardNamespace::*;
        let subset = match (&dw.namespace, &bw.namespace) {
            (_, Any) => Some(true),
            (Any, _) => Some(false),
            (Other, Other) => (dw.target_namespace == bw.target_namespace).then_some(true),
            (Other, List(_)) => Some(false),
            (List(uris), _) => Some(
                uris.iter()
                    .all(|u| bw.matches((!u.is_empty()).then_some(u.as_str()))),
            ),
        };
        if subset == Some(false) {
            return invalid(
                what,
                "the attribute wildcard allows namespaces the base's does not".to_string(),
            );
        }
    }
    Ok(())
}
