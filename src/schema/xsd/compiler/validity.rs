//! Post-compilation schema validity checks (schema-for-schemas constraints
//! that need resolved types): facet applicability per primitive value space,
//! facet bounds consistency, and enumeration value validity.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::types::{CompiledSchema, SimpleType, TypeDef};
use crate::schema::xsd::primitive::PrimitiveKind;
use crate::schema::xsd::value_compare::compare_values;

/// Checks every compiled type against schema-for-schemas constraints.
pub(crate) fn check_schema_validity(schema: &CompiledSchema) -> Result<()> {
    for (ns_name, type_def) in &schema.types_ns {
        let name = schema.display_name(ns_name);
        match type_def {
            TypeDef::Simple(st) => check_simple_type(schema, &name, st)?,
            TypeDef::Complex(ct) => {
                check_complex_restriction(schema, &name, ct)?;
                if let Some(elements) = content_elements(&ct.content) {
                    for elem in elements {
                        check_value_constraints(schema, elem)?;
                    }
                }
            }
        }
    }
    for elem in schema.elements_ns.values() {
        check_value_constraints(schema, elem)?;
    }
    Ok(())
}

/// An element's default/fixed value must be a valid instance of its type.
fn check_value_constraints(
    schema: &CompiledSchema,
    elem: &crate::schema::types::ElementDef,
) -> Result<()> {
    let Some(value) = elem.default.as_deref().or(elem.fixed.as_deref()) else {
        return Ok(());
    };
    // C4: ns-first (compile-time resolved), string fallback.
    let simple = match elem
        .type_ref
        .as_deref()
        .and_then(|t| schema.type_by_ref(elem.type_ns.as_ref(), t))
    {
        Some(TypeDef::Simple(st)) => st,
        _ => return Ok(()),
    };
    if let Some(kind) = PrimitiveKind::resolve(schema, simple)
        && kind.validate(value).is_err()
    {
        return Err(invalid(format!(
            "element '{}': default/fixed value '{}' is not valid for its type",
            elem.name, value
        )));
    }
    Ok(())
}

/// Pragmatic particle-restriction legality (a subset of the rcase-* rules):
/// a complex type derived by restriction must accept a subset of its base.
fn check_complex_restriction(
    schema: &CompiledSchema,
    name: &str,
    ct: &crate::schema::types::ComplexType,
) -> Result<()> {
    use crate::schema::types::{ContentModel, ProcessContents, WildcardNamespace};

    if ct.derivation != Some(crate::schema::types::DerivationMethod::Restriction) {
        return Ok(());
    }
    let Some(base_name) = ct.base_type.as_deref() else {
        return Ok(());
    };
    // C4: ns-first (compile-time resolved base_ns), string fallback.
    let Some(TypeDef::Complex(base)) = schema.type_by_ref(ct.base_ns.as_ref(), base_name) else {
        return Ok(());
    };
    // Self-reference via placeholder or unrelated lookup
    if std::ptr::eq(base, ct) {
        return Ok(());
    }

    // Wildcard rules: a restriction cannot introduce a wildcard, weaken its
    // processContents, or widen its namespace constraint.
    if let Some(ref dw) = ct.wildcard {
        match &base.wildcard {
            None => {
                // Only flag when the base genuinely has element content
                // (an empty base could not legally be restricted anyway).
                if !matches!(base.content, ContentModel::Empty) {
                    return Err(invalid(format!(
                        "type '{}': restriction introduces a wildcard not present in base '{}'",
                        name, base_name
                    )));
                }
            }
            Some(bw) => {
                let strength = |pc: ProcessContents| match pc {
                    ProcessContents::Strict => 3u8,
                    ProcessContents::Lax => 2,
                    ProcessContents::Skip => 1,
                };
                if strength(dw.process_contents) < strength(bw.process_contents) {
                    return Err(invalid(format!(
                        "type '{}': restriction weakens wildcard processContents of base '{}'",
                        name, base_name
                    )));
                }
                let subset = match (&dw.namespace, &bw.namespace) {
                    (_, WildcardNamespace::Any) => true,
                    (WildcardNamespace::Any, _) => false,
                    (WildcardNamespace::Other, WildcardNamespace::Other) => {
                        dw.target_namespace == bw.target_namespace
                    }
                    (WildcardNamespace::List(d), WildcardNamespace::List(b)) => {
                        d.iter().all(|u| b.contains(u))
                    }
                    (WildcardNamespace::List(d), WildcardNamespace::Other) => d.iter().all(|u| {
                        !u.is_empty() && Some(u.as_str()) != bw.target_namespace.as_deref()
                    }),
                    (WildcardNamespace::Other, WildcardNamespace::List(_)) => false,
                };
                if !subset {
                    return Err(invalid(format!(
                        "type '{}': restriction widens wildcard namespace of base '{}'",
                        name, base_name
                    )));
                }
            }
        }
    }

    // Element rules on the flattened content models.
    let derived_elems = content_elements(&ct.content);
    let base_elems = content_elements(&base.content);
    let (Some(derived_elems), Some(base_elems)) = (derived_elems, base_elems) else {
        return Ok(());
    };

    for d in derived_elems {
        match base_elems.iter().find(|b| b.name == d.name) {
            None => {
                // A substitution-group member may stand in for its head.
                let substitutes_into_base = {
                    let mut current = d.name.as_str();
                    let mut found = false;
                    for _ in 0..16 {
                        // Heads may be registered under qualified or local
                        // names; try both at every hop.
                        let head = schema.substitution_group_heads.get(current).or_else(|| {
                            let local = current.rsplit(':').next().unwrap_or(current);
                            schema.substitution_group_heads.get(local)
                        });
                        match head {
                            Some(head) => {
                                let head_local = head.rsplit(':').next().unwrap_or(head);
                                if base_elems.iter().any(|b| {
                                    b.name.rsplit(':').next().unwrap_or(&b.name) == head_local
                                }) {
                                    found = true;
                                    break;
                                }
                                current = head;
                            }
                            None => break,
                        }
                    }
                    found
                };
                // An element reference we cannot resolve at all most
                // likely lives in an unresolved import; don't reject the
                // schema based on incomplete knowledge.
                let unknown_ref = schema.get_element(&d.name).is_none();
                if base.wildcard.is_none() && !substitutes_into_base && !unknown_ref {
                    return Err(invalid(format!(
                        "type '{}': restriction adds element '{}' not present in base '{}'",
                        name, d.name, base_name
                    )));
                }
            }
            // A base wildcard can absorb additional occurrences, so the
            // pairwise occurrence comparison only holds without one.
            Some(b) if base.wildcard.is_none() => {
                // Occurrence range must narrow, never widen. Flattening
                // turns choice members into min=0, so only compare minima
                // when both content models are sequences.
                let both_sequences = matches!(ct.content, ContentModel::Sequence(_))
                    && matches!(base.content, ContentModel::Sequence(_));
                if both_sequences && d.min_occurs < b.min_occurs {
                    return Err(invalid(format!(
                        "type '{}': restriction lowers minOccurs of element '{}' below base '{}'",
                        name, d.name, base_name
                    )));
                }
                let widened = match (d.max_occurs, b.max_occurs) {
                    (None, Some(_)) => true,
                    (Some(dm), Some(bm)) => dm > bm,
                    _ => false,
                };
                if widened {
                    return Err(invalid(format!(
                        "type '{}': restriction raises maxOccurs of element '{}' above base '{}'",
                        name, d.name, base_name
                    )));
                }
            }
            Some(_) => {}
        }
    }

    Ok(())
}

/// The immediate flattened element list of a content model, when it has one.
fn content_elements(
    content: &crate::schema::types::ContentModel,
) -> Option<&[crate::schema::types::ElementDef]> {
    use crate::schema::types::ContentModel;
    match content {
        ContentModel::Sequence(e) | ContentModel::Choice(e) | ContentModel::All(e) => Some(e),
        _ => None,
    }
}

fn invalid(message: String) -> crate::error::Error {
    SchemaError::InvalidSchema { message }.into()
}

fn check_simple_type(schema: &CompiledSchema, name: &str, st: &SimpleType) -> Result<()> {
    let kind = PrimitiveKind::resolve(schema, st);
    let is_list = st.item_type.is_some();

    // Facet applicability. `None` covers both the string family and
    // unresolved bases; flag nothing there to avoid false rejections.
    if let Some(kind) = kind
        && !is_list
    {
        let length_ok = matches!(
            kind,
            PrimitiveKind::HexBinary
                | PrimitiveKind::Base64Binary
                | PrimitiveKind::AnyUri
                | PrimitiveKind::QName
                | PrimitiveKind::Name
                | PrimitiveKind::Ncname
                | PrimitiveKind::Nmtoken
                | PrimitiveKind::Language
                | PrimitiveKind::Id
                | PrimitiveKind::Idref
                | PrimitiveKind::Entity
        );
        if !length_ok && (st.length.is_some() || st.min_length.is_some() || st.max_length.is_some())
        {
            return Err(invalid(format!(
                "type '{}': length facets are not applicable to its base type",
                name
            )));
        }

        let ordered = !matches!(
            kind,
            PrimitiveKind::Boolean
                | PrimitiveKind::HexBinary
                | PrimitiveKind::Base64Binary
                | PrimitiveKind::AnyUri
                | PrimitiveKind::QName
                | PrimitiveKind::Name
                | PrimitiveKind::Ncname
                | PrimitiveKind::Nmtoken
                | PrimitiveKind::Language
                | PrimitiveKind::Id
                | PrimitiveKind::Idref
                | PrimitiveKind::Entity
        );
        if !ordered
            && (st.min_inclusive.is_some()
                || st.max_inclusive.is_some()
                || st.min_exclusive.is_some()
                || st.max_exclusive.is_some())
        {
            return Err(invalid(format!(
                "type '{}': range facets are not applicable to its base type",
                name
            )));
        }

        let decimal_family = matches!(
            kind,
            PrimitiveKind::Decimal
                | PrimitiveKind::Integer
                | PrimitiveKind::Long
                | PrimitiveKind::Int
                | PrimitiveKind::Short
                | PrimitiveKind::Byte
                | PrimitiveKind::NonNegativeInteger
                | PrimitiveKind::PositiveInteger
                | PrimitiveKind::NonPositiveInteger
                | PrimitiveKind::NegativeInteger
                | PrimitiveKind::UnsignedLong
                | PrimitiveKind::UnsignedInt
                | PrimitiveKind::UnsignedShort
                | PrimitiveKind::UnsignedByte
        );
        if !decimal_family && (st.total_digits.is_some() || st.fraction_digits.is_some()) {
            return Err(invalid(format!(
                "type '{}': digit facets are not applicable to its base type",
                name
            )));
        }

        // Enumeration values must be valid instances of the base type.
        for value in &st.enumeration {
            if kind.validate(value).is_err() {
                return Err(invalid(format!(
                    "type '{}': enumeration value '{}' is not valid for its base type",
                    name, value
                )));
            }
        }

        // Range facet values themselves must be valid for the base type.
        for (facet, value) in [
            ("minInclusive", &st.min_inclusive),
            ("maxInclusive", &st.max_inclusive),
            ("minExclusive", &st.min_exclusive),
            ("maxExclusive", &st.max_exclusive),
        ] {
            if let Some(v) = value
                && kind.validate(v).is_err()
            {
                return Err(invalid(format!(
                    "type '{}': {} value '{}' is not valid for its base type",
                    name, facet, v
                )));
            }
        }
    }

    // Mutually exclusive bounds.
    if st.min_inclusive.is_some() && st.min_exclusive.is_some() {
        return Err(invalid(format!(
            "type '{}': minInclusive and minExclusive cannot both be present",
            name
        )));
    }
    if st.max_inclusive.is_some() && st.max_exclusive.is_some() {
        return Err(invalid(format!(
            "type '{}': maxInclusive and maxExclusive cannot both be present",
            name
        )));
    }

    // Bound ordering, compared in the type's value space.
    if let (Some(min), Some(max)) = (&st.min_inclusive, &st.max_inclusive)
        && compare_values(kind, min, max) == Some(std::cmp::Ordering::Greater)
    {
        return Err(invalid(format!(
            "type '{}': minInclusive '{}' is greater than maxInclusive '{}'",
            name, min, max
        )));
    }
    if let (Some(min), Some(max)) = (&st.min_exclusive, &st.max_exclusive)
        && compare_values(kind, min, max) == Some(std::cmp::Ordering::Greater)
    {
        return Err(invalid(format!(
            "type '{}': minExclusive '{}' is greater than maxExclusive '{}'",
            name, min, max
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::schema::xsd::parse_xsd;

    fn schema_of(body: &str) -> crate::error::Result<crate::schema::types::CompiledSchema> {
        let xsd = format!(
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{}</xs:schema>"#,
            body
        );
        parse_xsd(xsd.as_bytes())
    }

    #[test]
    fn rejects_enum_invalid_for_base() {
        let result = schema_of(
            r#"<xs:simpleType name="t"><xs:restriction base="xs:integer">
                 <xs:enumeration value="10"/><xs:enumeration value="CA"/>
               </xs:restriction></xs:simpleType>"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_length_on_numeric() {
        let result = schema_of(
            r#"<xs:simpleType name="t"><xs:restriction base="xs:decimal">
                 <xs:maxLength value="5"/>
               </xs:restriction></xs:simpleType>"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_range_on_boolean() {
        let result = schema_of(
            r#"<xs:simpleType name="t"><xs:restriction base="xs:boolean">
                 <xs:minInclusive value="0"/>
               </xs:restriction></xs:simpleType>"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_inverted_bounds() {
        let result = schema_of(
            r#"<xs:simpleType name="t"><xs:restriction base="xs:int">
                 <xs:minInclusive value="10"/><xs:maxInclusive value="5"/>
               </xs:restriction></xs:simpleType>"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn accepts_valid_restrictions() {
        let result = schema_of(
            r#"<xs:simpleType name="t"><xs:restriction base="xs:string">
                 <xs:maxLength value="5"/><xs:enumeration value="ab"/>
               </xs:restriction></xs:simpleType>
               <xs:simpleType name="n"><xs:restriction base="xs:int">
                 <xs:minInclusive value="1"/><xs:maxInclusive value="10"/>
               </xs:restriction></xs:simpleType>"#,
        );
        assert!(result.is_ok(), "{result:?}");
    }
}
