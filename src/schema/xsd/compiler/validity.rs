//! Post-compilation schema validity checks (schema-for-schemas constraints
//! that need resolved types): facet applicability per primitive value space,
//! facet bounds consistency, and enumeration value validity.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::types::{CompiledSchema, SimpleType, TypeDef};
use crate::schema::xsd::primitive::PrimitiveKind;
use crate::schema::xsd::value_compare::compare_values;

/// Checks every compiled simple type against schema-for-schemas constraints.
pub(crate) fn check_schema_validity(schema: &CompiledSchema) -> Result<()> {
    for (name, type_def) in &schema.types {
        if let TypeDef::Simple(st) = type_def {
            check_simple_type(schema, name, st)?;
        }
    }
    Ok(())
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
        if !length_ok
            && (st.length.is_some() || st.min_length.is_some() || st.max_length.is_some())
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
