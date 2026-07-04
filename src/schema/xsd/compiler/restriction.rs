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
    CompiledSchema, ComplexType, ContentModel, DerivationMethod, ElementDef, Particle, TypeDef,
    WildcardNamespace,
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
    for (name, type_def) in &schema.types {
        // Prefixed keys only: every type is also stored under a prefixed
        // alias when a namespace prefix exists, and checking the same type
        // twice is wasted work. Types without any colon are no-namespace.
        if let TypeDef::Complex(ct) = type_def
            && ct.derivation == Some(DerivationMethod::Restriction)
            && let Violation(reason) = check_restriction(ct, schema)
        {
            return Err(SchemaError::InvalidSchema {
                message: format!("type '{}' is not a valid restriction: {}", name, reason),
            }
            .into());
        }
    }
    Ok(())
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
            match bw.namespace {
                WildcardNamespace::Any => VOk,
                // Other namespace constraints need the element's resolved
                // namespace, which the compiled tree does not carry.
                _ => Unknown,
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
            match (&dw.namespace, &bw.namespace) {
                (_, WildcardNamespace::Any) => VOk,
                (WildcardNamespace::Other, WildcardNamespace::Other) => VOk,
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
            let direct = recurse_ordered(
                std::slice::from_ref(d),
                bitems,
                schema,
                /* lax */ false,
            );
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
        // rcase-MapAndSum (choice restricting… ) and the remaining mixed
        // compositor pairs are out of the implemented subset.
        _ => Unknown,
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

/// rcase-NameAndTypeOK for a pair of element declarations.
fn element_restrict(de: &ElementDef, be: &ElementDef, schema: &CompiledSchema) -> Verdict {
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
    type_derivation_ok(de, be, schema)
}

/// Whether the derived element's type is (transitively) derived from — or
/// identical to — the base element's type.
fn type_derivation_ok(de: &ElementDef, be: &ElementDef, schema: &CompiledSchema) -> Verdict {
    let Some(base_type) = &be.type_ref else {
        return VOk; // base is anyType: everything derives from it
    };
    let base_local = local_name(base_type);
    if base_local == "anyType" || base_local == "anySimpleType" {
        return VOk;
    }
    // Inline types would need structural comparison.
    if be.inline_type.is_some() || de.inline_type.is_some() {
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
            Some(TypeDef::Simple(s)) => s.base_type.clone(),
            Some(TypeDef::Complex(c)) => c.base_type.clone(),
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

#[cfg(test)]
mod tests {
    use crate::schema::Schema;

    #[test]
    fn widening_element_type_rejected() {
        // Derived c1 uses xs:string, which is not derived from the base's
        // restricted string type (particlesIk shape).
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:simpleType name="rString">
                <xs:restriction base="xs:string">
                    <xs:maxLength value="4"/>
                </xs:restriction>
            </xs:simpleType>
            <xs:complexType name="B">
                <xs:choice>
                    <xs:element name="c1" type="x:rString"/>
                </xs:choice>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:choice>
                            <xs:element name="c1" type="xs:string"/>
                        </xs:choice>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("widening type");
        assert!(err.to_string().contains("restriction"), "got: {err}");
    }

    #[test]
    fn narrowing_element_type_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:simpleType name="rString">
                <xs:restriction base="xs:string">
                    <xs:maxLength value="4"/>
                </xs:restriction>
            </xs:simpleType>
            <xs:complexType name="B">
                <xs:choice>
                    <xs:element name="c1" type="xs:string"/>
                </xs:choice>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:choice>
                            <xs:element name="c1" type="x:rString"/>
                        </xs:choice>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("narrowing restriction must compile");
    }

    #[test]
    fn occurrence_widening_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:element name="e" type="xs:string" maxOccurs="2"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:element name="e" type="xs:string" maxOccurs="5"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("occurrence widening");
        assert!(err.to_string().contains("restriction"), "got: {err}");
    }

    #[test]
    fn dropping_required_base_element_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:element name="a" type="xs:string"/>
                    <xs:element name="b" type="xs:string"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:element name="a" type="xs:string"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("dropping required element");
        assert!(err.to_string().contains("restriction"), "got: {err}");
    }

    #[test]
    fn dropping_optional_base_element_accepted() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:element name="a" type="xs:string"/>
                    <xs:element name="b" type="xs:string" minOccurs="0"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:element name="a" type="xs:string"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        Schema::from_xsd(xsd.as_bytes()).expect("dropping optional content must compile");
    }

    #[test]
    fn extra_derived_content_rejected() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:element name="a" type="xs:string"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:element name="a" type="xs:string"/>
                            <xs:element name="zz" type="xs:string"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("extra content");
        assert!(err.to_string().contains("restriction"), "got: {err}");
    }

    #[test]
    fn wildcard_restricting_element_rejected() {
        let xsd = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:element name="a" type="xs:string"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:any namespace="##any"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"###;
        let err = Schema::from_xsd(xsd.as_bytes()).expect_err("wildcard vs element");
        assert!(err.to_string().contains("restriction"), "got: {err}");
    }

    #[test]
    fn element_within_wildcard_accepted() {
        let xsd = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                targetNamespace="http://t" xmlns:x="http://t">
            <xs:complexType name="B">
                <xs:sequence>
                    <xs:any namespace="##any"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="R">
                <xs:complexContent>
                    <xs:restriction base="x:B">
                        <xs:sequence>
                            <xs:element name="a" type="xs:string"/>
                        </xs:sequence>
                    </xs:restriction>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"###;
        Schema::from_xsd(xsd.as_bytes()).expect("element within wildcard must compile");
    }
}
