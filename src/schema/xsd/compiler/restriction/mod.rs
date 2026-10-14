//! Particle-restriction checking (XSD 1.0 cos-particle-restrict, the
//! "rcase-*" rules) over the nested [`Particle`] trees.
//!
//! The engine is deliberately three-valued: a comparison yields `Ok`
//! (definitely a valid restriction), `Violation` (definitely invalid), or
//! `Unknown` (out of the implemented subset, or dependent on unresolved
//! information). Only a `Violation` untainted by any `Unknown` rejects the
//! schema — everything uncertain is accepted, so lenient real-world schema
//! sets keep compiling.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, DerivationMethod, ElementDef, Particle,
    ProcessContents, TypeDef, WildcardNamespace,
};

/// Outcome of one particle comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    /// Definitely a valid restriction.
    Ok,
    /// Definitely invalid (with a reason).
    Violation(String),
    /// Cannot be decided within the implemented subset — accept.
    Unknown,
}

use Verdict::{Ok as VOk, Unknown, Violation};

/// Checks every complex type derived by restriction against its base's
/// content model. Runs after type compilation (particle trees and the
/// substitution caches are available on `schema`).
pub(crate) fn check_particle_restrictions(schema: &CompiledSchema) -> Result<()> {
    for_each_complex_type(schema, |ct, what| check_one(ct, what, schema))
}

/// Runs `check` on every complex type of `schema`, named or anonymous, with
/// a description of it for messages.
pub(crate) fn for_each_complex_type(
    schema: &CompiledSchema,
    mut check: impl FnMut(&ComplexType, &str) -> Result<()>,
) -> Result<()> {
    let mut anonymous: Vec<(&str, &ComplexType)> = Vec::new();
    for (ns_name, type_def) in &schema.types_ns {
        if let TypeDef::Complex(ct) = type_def {
            check(ct, &format!("type '{}'", schema.display_name(ns_name)))?;
            collect_anonymous(ct.particle.as_deref(), &mut anonymous);
        }
    }
    for elem in schema.elements_ns.values() {
        if let Some(TypeDef::Complex(ct)) = &elem.inline_type {
            anonymous.push((&elem.name, ct));
        }
    }
    // Anonymous types nest: each one may hold further local elements.
    while let Some((element, ct)) = anonymous.pop() {
        check(ct, &format!("the type of element '{element}'"))?;
        collect_anonymous(ct.particle.as_deref(), &mut anonymous);
    }
    Ok(())
}

/// Rejects `ct`, described by `what`, when it is derived by restriction and
/// definitely not a valid one.
fn check_one(ct: &ComplexType, what: &str, schema: &CompiledSchema) -> Result<()> {
    if ct.derivation == Some(DerivationMethod::Restriction)
        && let Violation(reason) = check_restriction(ct, schema)
    {
        return Err(SchemaError::InvalidSchema {
            message: format!("{what} is not a valid restriction: {reason}"),
        }
        .into());
    }
    Ok(())
}

/// Collects the anonymous complex types of the local elements in `particle`.
fn collect_anonymous<'a>(
    particle: Option<&'a Particle>,
    out: &mut Vec<(&'a str, &'a ComplexType)>,
) {
    let Some(particle) = particle else {
        return;
    };
    let mut push = |e: &'a ElementDef| {
        if let Some(TypeDef::Complex(ct)) = &e.inline_type {
            out.push((&e.name, ct));
        }
    };
    match particle {
        Particle::Element(e) => push(e),
        Particle::All { elements, .. } => elements.iter().for_each(push),
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } => {
            for item in items {
                collect_anonymous(Some(item), out);
            }
        }
        Particle::Wildcard(_) => {}
    }
}

/// Checks one restriction-derived complex type. `Unknown` accepts.
fn check_restriction(ct: &ComplexType, schema: &CompiledSchema) -> Verdict {
    let Some(base_name) = &ct.base_type else {
        return Unknown;
    };
    let base_local = base_name
        .split_once(':')
        .map(|(_, l)| l)
        .unwrap_or(base_name);
    if base_local == "anyType" {
        return VOk; // everything restricts the ur-type
    }
    let Some(TypeDef::Complex(base)) = schema.get_type(base_name) else {
        return Unknown;
    };

    let base_particle = match effective_particle(base, schema, 0) {
        Some(p) => p,
        None => {
            // No (or unresolvable) base content: a derived particle with
            // required content cannot restrict an empty base.
            if matches!(base.content, ContentModel::Empty)
                && let Some(derived) = ct.particle.as_deref()
                && !emptiable(derived)
            {
                return Violation("base type has empty content".into());
            }
            return Unknown;
        }
    };
    let Some(derived_particle) = ct.particle.as_deref() else {
        // Empty derived content is fine iff the base is emptiable.
        if matches!(
            ct.content,
            ContentModel::Empty | ContentModel::SimpleContent { .. }
        ) && !emptiable(&base_particle)
        {
            return Violation("base content is not emptiable".into());
        }
        return Unknown;
    };

    let d = normalize(derived_particle.clone());
    let b = normalize(base_particle);
    // A derived particle that can never occur admits no content at all,
    // which restricts anything emptiable (and an unoccurrable base).
    if range(&d).1 == Some(0) && (emptiable(&b) || range(&b).1 == Some(0)) {
        return VOk;
    }
    restrict(&d, &b, schema)
}

/// Inheritance-merged particle of a complex type (extensions append their
/// own particle after the base chain's). `None` = empty or unresolvable.
fn effective_particle(ct: &ComplexType, schema: &CompiledSchema, depth: usize) -> Option<Particle> {
    if depth > 16 {
        return None;
    }
    if ct.derivation == Some(DerivationMethod::Extension)
        && let Some(base_name) = &ct.base_type
    {
        let base_local = base_name
            .split_once(':')
            .map(|(_, l)| l)
            .unwrap_or(base_name);
        let base_part = if base_local == "anyType" {
            None
        } else {
            match schema.get_type(base_name)? {
                TypeDef::Complex(b) => effective_particle(b, schema, depth + 1),
                TypeDef::Simple(_) => None,
            }
        };
        let own = ct.particle.as_deref().cloned();
        return match (base_part, own) {
            (None, None) => None,
            (Some(b), None) => Some(b),
            (None, Some(o)) => Some(o),
            (Some(b), Some(o)) => Some(Particle::Sequence {
                min: 1,
                max: Some(1),
                items: vec![b, o],
            }),
        };
    }
    ct.particle.as_deref().cloned()
}

/// Applies the "pointless particle" normalization of XSD 1.0 §3.9.6: a
/// sequence/choice with exactly one child and 1..1 occurrence is replaced by
/// its child; same-compositor 1..1 children are spliced into the parent.
fn normalize(p: Particle) -> Particle {
    match p {
        Particle::Sequence { min, max, items } => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let n = normalize(item);
                // A particle that can never occur contributes nothing.
                if range(&n).1 == Some(0) {
                    continue;
                }
                match n {
                    Particle::Sequence {
                        min: 1,
                        max: Some(1),
                        items: nested,
                    } => out.extend(nested),
                    other => out.push(other),
                }
            }
            if out.len() == 1 && min == 1 && max == Some(1) {
                return out.into_iter().next().expect("one item");
            }
            // A group reference with occurrence bounds compiles to a
            // sequence wrapping the referenced 1..1 group: the bounds
            // belong to that group.
            if out.len() == 1 {
                match out.pop().expect("one item") {
                    Particle::Choice {
                        min: 1,
                        max: Some(1),
                        items,
                    } => return Particle::Choice { min, max, items },
                    Particle::Sequence {
                        min: 1,
                        max: Some(1),
                        items,
                    } => return Particle::Sequence { min, max, items },
                    other => out.push(other),
                }
            }
            Particle::Sequence {
                min,
                max,
                items: out,
            }
        }
        Particle::Choice { min, max, items } => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let n = normalize(item);
                // A particle that can never occur contributes nothing.
                if range(&n).1 == Some(0) {
                    continue;
                }
                match n {
                    Particle::Choice {
                        min: 1,
                        max: Some(1),
                        items: nested,
                    } => out.extend(nested),
                    other => out.push(other),
                }
            }
            if out.len() == 1 && min == 1 && max == Some(1) {
                return out.into_iter().next().expect("one item");
            }
            Particle::Choice {
                min,
                max,
                items: out,
            }
        }
        other => other,
    }
}

/// The occurrence range of a particle.
fn range(p: &Particle) -> (u32, Option<u32>) {
    match p {
        Particle::Element(e) => (e.min_occurs, e.max_occurs),
        Particle::Wildcard(w) => (w.min_occurs, w.max_occurs),
        Particle::Sequence { min, max, .. } | Particle::Choice { min, max, .. } => (*min, *max),
        Particle::All { min, .. } => (*min, Some(1)),
    }
}

/// occurrence-range-OK: the derived range must lie within the base range.
fn range_ok(d: &Particle, b: &Particle) -> bool {
    let (dmin, dmax) = range(d);
    let (bmin, bmax) = range(b);
    if dmin < bmin {
        return false;
    }
    match (dmax, bmax) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(dm), Some(bm)) => dm <= bm,
    }
}

/// particle-emptiable: whether the particle admits zero elements.
fn emptiable(p: &Particle) -> bool {
    match p {
        Particle::Element(e) => e.min_occurs == 0,
        Particle::Wildcard(w) => w.min_occurs == 0,
        Particle::Sequence { min, items, .. } => *min == 0 || items.iter().all(emptiable),
        Particle::Choice { min, items, .. } => {
            *min == 0 || items.is_empty() || items.iter().any(emptiable)
        }
        Particle::All { min, elements } => *min == 0 || elements.iter().all(|e| e.min_occurs == 0),
    }
}

/// Local name of a possibly prefixed element name.
fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The base type of a built-in XSD datatype (local names). Compilation runs
/// before `register_builtin_types`, so the built-in hierarchy is not in the
/// schema's type table yet.
fn builtin_parent(local: &str) -> Option<&'static str> {
    Some(match local {
        "normalizedString" => "string",
        "token" => "normalizedString",
        "language" | "NMTOKEN" | "Name" => "token",
        "NCName" => "Name",
        "ID" | "IDREF" | "ENTITY" => "NCName",
        "integer" => "decimal",
        "nonPositiveInteger" | "nonNegativeInteger" | "long" => "integer",
        "negativeInteger" => "nonPositiveInteger",
        "int" => "long",
        "short" => "int",
        "byte" => "short",
        "unsignedLong" | "positiveInteger" => "nonNegativeInteger",
        "unsignedInt" => "unsignedLong",
        "unsignedShort" => "unsignedInt",
        "unsignedByte" => "unsignedShort",
        "dateTimeStamp" => "dateTime",
        "dayTimeDuration" | "yearMonthDuration" => "duration",
        // Primitives and the built-in list types derive from anySimpleType.
        "string" | "boolean" | "decimal" | "float" | "double" | "duration" | "dateTime"
        | "time" | "date" | "gYearMonth" | "gYear" | "gMonthDay" | "gDay" | "gMonth"
        | "hexBinary" | "base64Binary" | "anyURI" | "QName" | "NOTATION" | "NMTOKENS"
        | "IDREFS" | "ENTITIES" | "anyAtomicType" => "anySimpleType",
        "anySimpleType" => "anyType",
        _ => return None,
    })
}

/// Downgrades a definite violation to Unknown when any uncertainty was
/// involved in reaching it.
fn definite(v: Verdict, unknown_seen: bool) -> Verdict {
    match v {
        Violation(_) if unknown_seen => Unknown,
        other => other,
    }
}

/// The rcase-* dispatch over one derived/base particle pair.
fn restrict(d: &Particle, b: &Particle, schema: &CompiledSchema) -> Verdict {
    match (d, b) {
        // rcase-NameAndTypeOK
        (Particle::Element(de), Particle::Element(be)) => element_restrict(de, be, schema),
        // rcase-NSCompat
        (Particle::Element(de), Particle::Wildcard(bw)) => {
            if !range_ok(d, b) {
                return Violation(format!(
                    "element '{}' occurrence range exceeds the base wildcard's",
                    de.name
                ));
            }
            match (&bw.namespace, known_namespace(de, schema)) {
                (WildcardNamespace::Any, _) => VOk,
                (_, Some(ns)) if bw.matches((!ns.is_empty()).then_some(ns)) => VOk,
                (_, Some(ns)) => Violation(format!(
                    "element '{}' in namespace '{ns}' is not allowed by the base wildcard",
                    de.name
                )),
                (_, None) => Unknown,
            }
        }
        // A wildcard cannot restrict an element declaration.
        (Particle::Wildcard(_), Particle::Element(be)) => {
            Violation(format!("a wildcard cannot restrict element '{}'", be.name))
        }
        // rcase-NSSubset
        (Particle::Wildcard(dw), Particle::Wildcard(bw)) => {
            if !range_ok(d, b) {
                return Violation("wildcard occurrence range exceeds the base wildcard's".into());
            }
            if strength(dw.process_contents) < strength(bw.process_contents) {
                return Violation(
                    "wildcard processContents is weaker than the base wildcard's".into(),
                );
            }
            match (&dw.namespace, &bw.namespace) {
                (_, WildcardNamespace::Any) => VOk,
                (WildcardNamespace::Any, _) => {
                    Violation("##any is not a subset of the base wildcard's namespaces".into())
                }
                (WildcardNamespace::Other, WildcardNamespace::Other)
                    if dw.target_namespace == bw.target_namespace =>
                {
                    VOk
                }
                (WildcardNamespace::Other, WildcardNamespace::List(_)) => {
                    Violation("##other is not a subset of the base wildcard's namespaces".into())
                }
                (WildcardNamespace::List(uris), _) => {
                    if uris
                        .iter()
                        .all(|u| bw.matches((!u.is_empty()).then_some(u.as_str())))
                    {
                        VOk
                    } else {
                        Violation(
                            "the wildcard allows a namespace the base wildcard does not".into(),
                        )
                    }
                }
                _ => Unknown,
            }
        }
        // rcase-Recurse (sequence within sequence)
        (Particle::Sequence { items: ditems, .. }, Particle::Sequence { items: bitems, .. }) => {
            if !range_ok(d, b) {
                return Violation("sequence occurrence range exceeds the base's".into());
            }
            recurse_ordered(ditems, bitems, schema, /* lax */ false)
        }
        // rcase-RecurseLax (choice within choice)
        (Particle::Choice { items: ditems, .. }, Particle::Choice { items: bitems, .. }) => {
            if !range_ok(d, b) {
                return Violation("choice occurrence range exceeds the base's".into());
            }
            recurse_ordered(ditems, bitems, schema, /* lax */ true)
        }
        // rcase-Recurse (all within all), matched by element name.
        (
            Particle::All {
                elements: delems, ..
            },
            Particle::All {
                elements: belems, ..
            },
        ) => {
            if !range_ok(d, b) {
                return Violation("all-group occurrence range exceeds the base's".into());
            }
            let mut unknown_seen = false;
            let mut matched = vec![false; belems.len()];
            for de in delems {
                let Some((i, be)) = belems
                    .iter()
                    .enumerate()
                    .find(|(i, be)| !matched[*i] && local_name(&be.name) == local_name(&de.name))
                else {
                    return definite(
                        Violation(format!(
                            "element '{}' is not in the base all-group",
                            de.name
                        )),
                        unknown_seen,
                    );
                };
                matched[i] = true;
                match element_restrict(de, be, schema) {
                    VOk => {}
                    v @ Violation(_) => return definite(v, unknown_seen),
                    Unknown => unknown_seen = true,
                }
            }
            for (i, be) in belems.iter().enumerate() {
                if !matched[i] && be.min_occurs > 0 {
                    return definite(
                        Violation(format!(
                            "required base element '{}' is missing from the restriction",
                            be.name
                        )),
                        unknown_seen,
                    );
                }
            }
            if unknown_seen { Unknown } else { VOk }
        }
        // rcase-RecurseAsIfGroup: an element restricting a group. Two
        // readings circulate among implementations (and the W3C suite
        // reflects both): (a) wrap the element in a singleton group carrying
        // its occurrence range; (b) map the element directly into the base
        // group's particles. Accept when either reading passes; reject only
        // when both definitively fail.
        (Particle::Element(de), Particle::Sequence { items: bitems, .. }) => {
            let mut inner = de.clone();
            inner.min_occurs = 1;
            inner.max_occurs = Some(1);
            let wrapped = Particle::Sequence {
                min: de.min_occurs,
                max: de.max_occurs,
                items: vec![Particle::Element(inner)],
            };
            let a = restrict(&wrapped, b, schema);
            if a == VOk {
                return VOk;
            }
            // Mapped directly, the element is one pass through the base
            // group, which falls short of a group required more than once.
            let direct = if range(b).0 > 1 {
                Violation("the base group is required more than once".into())
            } else {
                recurse_ordered(
                    std::slice::from_ref(d),
                    bitems,
                    schema,
                    /* lax */ false,
                )
            };
            combine_readings(a, direct)
        }
        (Particle::Element(de), Particle::Choice { items: bitems, .. }) => {
            let mut inner = de.clone();
            inner.min_occurs = 1;
            inner.max_occurs = Some(1);
            let wrapped = Particle::Choice {
                min: de.min_occurs,
                max: de.max_occurs,
                items: vec![Particle::Element(inner)],
            };
            let a = restrict(&wrapped, b, schema);
            if a == VOk {
                return VOk;
            }
            let mut direct = Violation("no base alternative admits the element".into());
            // As above: one pass cannot satisfy a choice required more
            // than once.
            let bitems: &[Particle] = if range(b).0 > 1 {
                direct = Violation("the base choice is required more than once".into());
                &[]
            } else {
                bitems
            };
            for alt in bitems {
                match restrict(d, alt, schema) {
                    VOk => {
                        direct = VOk;
                        break;
                    }
                    Unknown => direct = Unknown,
                    Violation(_) => {}
                }
            }
            combine_readings(a, direct)
        }
        // Base collapsed to a bare element by normalization: compare the
        // derived group against a singleton group of its own variety.
        (Particle::Sequence { .. }, Particle::Element(_)) => {
            let wrapped = Particle::Sequence {
                min: 1,
                max: Some(1),
                items: vec![b.clone()],
            };
            restrict(d, &wrapped, schema)
        }
        (Particle::Choice { .. }, Particle::Element(_)) => {
            let wrapped = Particle::Choice {
                min: 1,
                max: Some(1),
                items: vec![b.clone()],
            };
            restrict(d, &wrapped, schema)
        }
        // rcase-NSRecurseCheckCardinality: a group restricting a wildcard.
        (
            Particle::Sequence { .. } | Particle::Choice { .. } | Particle::All { .. },
            Particle::Wildcard(_),
        ) => {
            let (min, max) = effective_total_range(d);
            let (bmin, bmax) = range(b);
            let within = min >= bmin
                && match (max, bmax) {
                    (_, None) => true,
                    (None, Some(_)) => false,
                    (Some(m), Some(bm)) => m <= bm,
                };
            if !within {
                return Violation(
                    "the group's total occurrence range exceeds the base wildcard's".into(),
                );
            }
            // Members are compared by namespace only; occurrence was
            // checked for the group as a whole above.
            let Particle::Wildcard(bw) = b else {
                unreachable!("matched a wildcard base")
            };
            let mut any_count = bw.clone();
            any_count.min_occurs = 0;
            any_count.max_occurs = None;
            let any_count = Particle::Wildcard(any_count);
            let mut unknown_seen = false;
            for member in group_members(d) {
                match restrict(&member, &any_count, schema) {
                    VOk => {}
                    v @ Violation(_) => return definite(v, unknown_seen),
                    Unknown => unknown_seen = true,
                }
            }
            if unknown_seen { Unknown } else { VOk }
        }
        // Forbidden: a wildcard cannot restrict a model group.
        (
            Particle::Wildcard(_),
            Particle::Sequence { .. } | Particle::Choice { .. } | Particle::All { .. },
        ) => Violation("a wildcard cannot restrict a model group".into()),
        // Forbidden: an all-group restricting a choice or sequence, and a
        // choice restricting an all-group or sequence.
        (Particle::All { .. }, Particle::Choice { .. } | Particle::Sequence { .. })
        | (Particle::Choice { .. }, Particle::All { .. } | Particle::Sequence { .. }) => {
            Violation("this kind of model group cannot restrict the base's".into())
        }
        // rcase-MapAndSum: a sequence restricting a choice. Every member
        // must restrict some alternative, and the sequence's occurrence
        // times its length must fit the choice's range.
        (
            Particle::Sequence {
                min,
                max,
                items: ditems,
            },
            Particle::Choice { items: bitems, .. },
        ) => {
            let n = ditems.len() as u32;
            let total = Particle::Sequence {
                min: min.saturating_mul(n),
                max: max.and_then(|m| m.checked_mul(n)),
                items: Vec::new(),
            };
            if !range_ok(&total, b) {
                return Violation("the sequence's total range exceeds the base choice's".into());
            }
            let mut unknown_seen = false;
            for member in ditems {
                let mut verdict = Violation("a member restricts no base alternative".into());
                for alt in bitems {
                    match restrict(member, alt, schema) {
                        VOk => {
                            verdict = VOk;
                            break;
                        }
                        Unknown => verdict = Unknown,
                        Violation(_) => {}
                    }
                }
                match verdict {
                    VOk => {}
                    v @ Violation(_) => return definite(v, unknown_seen),
                    Unknown => unknown_seen = true,
                }
            }
            if unknown_seen { Unknown } else { VOk }
        }
        // rcase-RecurseUnordered: a sequence restricting an all-group. Each
        // member maps to a distinct base element it restricts; unmapped
        // base elements must be emptiable.
        (
            Particle::Sequence { items: ditems, .. },
            Particle::All {
                elements: belems, ..
            },
        ) => {
            if !range_ok(d, b) {
                return Violation("sequence occurrence range exceeds the base all-group's".into());
            }
            let mut unknown_seen = false;
            let mut matched = vec![false; belems.len()];
            for member in ditems {
                let Particle::Element(de) = member else {
                    return definite(
                        Violation("only elements can restrict an all-group's members".into()),
                        unknown_seen,
                    );
                };
                let Some(i) = (0..belems.len())
                    .find(|i| !matched[*i] && local_name(&belems[*i].name) == local_name(&de.name))
                else {
                    return definite(
                        Violation(format!(
                            "element '{}' is not in the base all-group",
                            de.name
                        )),
                        unknown_seen,
                    );
                };
                matched[i] = true;
                match element_restrict(de, &belems[i], schema) {
                    VOk => {}
                    v @ Violation(_) => return definite(v, unknown_seen),
                    Unknown => unknown_seen = true,
                }
            }
            if let Some(i) = (0..belems.len()).find(|i| !matched[*i] && belems[*i].min_occurs > 0) {
                return definite(
                    Violation(format!(
                        "required base element '{}' is missing from the restriction",
                        belems[i].name
                    )),
                    unknown_seen,
                );
            }
            if unknown_seen { Unknown } else { VOk }
        }
        _ => Unknown,
    }
}

/// How strictly a wildcard's matches are validated (skip < lax < strict).
fn strength(p: ProcessContents) -> u8 {
    match p {
        ProcessContents::Skip => 0,
        ProcessContents::Lax => 1,
        ProcessContents::Strict => 2,
    }
}

/// The members of a model group, each with its own occurrence range.
fn group_members(p: &Particle) -> Vec<Particle> {
    match p {
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } => items.clone(),
        Particle::All { elements, .. } => elements.iter().cloned().map(Particle::Element).collect(),
        other => vec![other.clone()],
    }
}

/// The effective total range of a particle (§3.8.6): how many elements it
/// can match at least and at most (`None` for unbounded).
fn effective_total_range(p: &Particle) -> (u32, Option<u32>) {
    let mul = |a: Option<u32>, b: Option<u32>| match (a, b) {
        (Some(0), _) | (_, Some(0)) => Some(0),
        (Some(a), Some(b)) => a.checked_mul(b),
        _ => None,
    };
    match p {
        Particle::Element(_) | Particle::Wildcard(_) => range(p),
        Particle::Sequence { min, .. } | Particle::All { min, .. } => {
            let max = range(p).1;
            let members: Vec<_> = group_members(p).iter().map(effective_total_range).collect();
            let sum_min = members.iter().map(|r| r.0).fold(0u32, u32::saturating_add);
            let sum_max = members
                .iter()
                .try_fold(0u32, |acc, r| r.1.and_then(|m| acc.checked_add(m)));
            (min.saturating_mul(sum_min), mul(max, sum_max))
        }
        Particle::Choice { min, max, items } => {
            let members: Vec<_> = items.iter().map(effective_total_range).collect();
            let least = members.iter().map(|r| r.0).min().unwrap_or(0);
            let most = members
                .iter()
                .try_fold(0u32, |acc, r| r.1.map(|m| acc.max(m)));
            (min.saturating_mul(least), mul(*max, most))
        }
    }
}

/// Combines the two RecurseAsIfGroup readings: either passing accepts;
/// both definitively failing rejects; anything else is uncertain.
fn combine_readings(a: Verdict, b: Verdict) -> Verdict {
    match (a, b) {
        (VOk, _) | (_, VOk) => VOk,
        (v @ Violation(_), Violation(_)) => v,
        _ => Unknown,
    }
}

/// Order-preserving mapping of derived items onto base items. In strict
/// (sequence) mode, skipped base items must be emptiable and leftover base
/// items must be emptiable; in lax (choice) mode they need not be.
fn recurse_ordered(
    ditems: &[Particle],
    bitems: &[Particle],
    schema: &CompiledSchema,
    lax: bool,
) -> Verdict {
    let mut unknown_seen = false;
    let mut j = 0;
    'derived: for d in ditems {
        while j < bitems.len() {
            let b = &bitems[j];
            match restrict(d, b, schema) {
                VOk => {
                    j += 1;
                    continue 'derived;
                }
                Unknown => {
                    unknown_seen = true;
                    j += 1;
                    continue 'derived;
                }
                Violation(reason) => {
                    if lax || emptiable(b) {
                        j += 1;
                        continue;
                    }
                    return definite(Violation(reason), unknown_seen);
                }
            }
        }
        return definite(
            Violation("the restriction has content the base does not allow".into()),
            unknown_seen,
        );
    }
    if !lax {
        for b in &bitems[j..] {
            if !emptiable(b) {
                return definite(
                    Violation("a required base particle is missing from the restriction".into()),
                    unknown_seen,
                );
            }
        }
    }
    if unknown_seen { Unknown } else { VOk }
}

mod elements;

use elements::{element_restrict, known_namespace};

#[cfg(test)]
mod tests;
