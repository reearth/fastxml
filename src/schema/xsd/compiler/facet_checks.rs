//! Compile-time facet validity checking (XSD 1.0 "Constraints on Simple Type
//! Definition Schema Components" and the per-facet validity rules).
//!
//! Two tiers of checks:
//! - base-independent: duplicate facets, `length` vs `minLength`/`maxLength`,
//!   mutually exclusive inclusive/exclusive bounds, `totalDigits >= 1`;
//! - base-dependent (when the restriction base is a built-in XSD datatype):
//!   facet applicability (length facets on strings/binaries only, digit
//!   facets on decimals only, range facets on ordered types only), range
//!   facet and enumeration values must be valid literals of the base type,
//!   and range bounds must be coherent.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::xsd::primitive::PrimitiveKind;
use crate::schema::xsd::types::XsdFacet;
use crate::schema::xsd::value_compare::compare_values;

/// What is known about a restriction's base type for facet checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FacetBase {
    /// A built-in datatype with a known primitive kind.
    Primitive(PrimitiveKind),
    /// The `xs:string` family (string / normalizedString / token): length
    /// facets apply, range and digit facets do not.
    StringFamily,
    /// A user-defined or unresolved base: only base-independent checks run.
    Unknown,
}

/// Classifies a built-in XSD type local name for facet checking.
pub(crate) fn classify_builtin_base(local: &str) -> FacetBase {
    if let Some(kind) = PrimitiveKind::from_type_name(local) {
        return FacetBase::Primitive(kind);
    }
    match local {
        "string" | "normalizedString" | "token" => FacetBase::StringFamily,
        _ => FacetBase::Unknown,
    }
}

/// Whether length/minLength/maxLength apply to this kind (string-derived,
/// binary, anyURI, QName/NOTATION — the latter tolerated per the errata).
fn length_applicable(kind: PrimitiveKind) -> bool {
    use PrimitiveKind::*;
    matches!(
        kind,
        HexBinary
            | Base64Binary
            | AnyUri
            | QName
            | Name
            | Ncname
            | Nmtoken
            | Language
            | Id
            | Idref
            | Entity
    )
}

/// Whether the kind's value space is ordered (range facets apply).
fn ordered(kind: PrimitiveKind) -> bool {
    use PrimitiveKind::*;
    !matches!(
        kind,
        Boolean
            | HexBinary
            | Base64Binary
            | AnyUri
            | QName
            | Name
            | Ncname
            | Nmtoken
            | Language
            | Id
            | Idref
            | Entity
    )
}

/// Whether totalDigits/fractionDigits apply (decimal-derived types).
fn digits_applicable(kind: PrimitiveKind) -> bool {
    use PrimitiveKind::*;
    matches!(
        kind,
        Decimal
            | Integer
            | Long
            | Int
            | Short
            | Byte
            | NonNegativeInteger
            | PositiveInteger
            | NonPositiveInteger
            | NegativeInteger
            | UnsignedLong
            | UnsignedInt
            | UnsignedShort
            | UnsignedByte
    )
}

fn err(message: String) -> crate::error::Error {
    SchemaError::InvalidSchema { message }.into()
}

/// Validates the facets of one restriction step against its base.
pub(crate) fn check_facets(facets: &[XsdFacet], base: FacetBase) -> Result<()> {
    let mut length = false;
    let mut min_length = false;
    let mut max_length = false;
    let mut white_space = false;
    let mut total_digits = false;
    let mut fraction_digits = false;
    let mut min_inclusive: Option<&str> = None;
    let mut min_exclusive: Option<&str> = None;
    let mut max_inclusive: Option<&str> = None;
    let mut max_exclusive: Option<&str> = None;

    let dup = |seen: &mut bool, name: &str| -> Result<()> {
        if *seen {
            return Err(err(format!(
                "facet '{name}' may only appear once in a restriction"
            )));
        }
        *seen = true;
        Ok(())
    };

    for facet in facets {
        match facet {
            XsdFacet::Length(_) => dup(&mut length, "length")?,
            XsdFacet::MinLength(_) => dup(&mut min_length, "minLength")?,
            XsdFacet::MaxLength(_) => dup(&mut max_length, "maxLength")?,
            XsdFacet::WhiteSpace(_) => dup(&mut white_space, "whiteSpace")?,
            XsdFacet::TotalDigits(n) => {
                dup(&mut total_digits, "totalDigits")?;
                if *n == 0 {
                    return Err(err(
                        "totalDigits must be a positive integer, found 0".to_string()
                    ));
                }
            }
            XsdFacet::FractionDigits(_) => dup(&mut fraction_digits, "fractionDigits")?,
            XsdFacet::MinInclusive(v) => {
                if min_inclusive.is_some() {
                    return Err(err("facet 'minInclusive' may only appear once".to_string()));
                }
                min_inclusive = Some(v);
            }
            XsdFacet::MinExclusive(v) => {
                if min_exclusive.is_some() {
                    return Err(err("facet 'minExclusive' may only appear once".to_string()));
                }
                min_exclusive = Some(v);
            }
            XsdFacet::MaxInclusive(v) => {
                if max_inclusive.is_some() {
                    return Err(err("facet 'maxInclusive' may only appear once".to_string()));
                }
                max_inclusive = Some(v);
            }
            XsdFacet::MaxExclusive(v) => {
                if max_exclusive.is_some() {
                    return Err(err("facet 'maxExclusive' may only appear once".to_string()));
                }
                max_exclusive = Some(v);
            }
            XsdFacet::Enumeration(_) | XsdFacet::Pattern(_) | XsdFacet::ExplicitTimezone(_) => {}
        }
    }

    // length excludes minLength/maxLength in the same step.
    if length && (min_length || max_length) {
        return Err(err(
            "'length' cannot be combined with 'minLength' or 'maxLength'".to_string(),
        ));
    }
    // Inclusive and exclusive bounds of the same end are mutually exclusive.
    if min_inclusive.is_some() && min_exclusive.is_some() {
        return Err(err(
            "'minInclusive' and 'minExclusive' cannot both be specified".to_string(),
        ));
    }
    if max_inclusive.is_some() && max_exclusive.is_some() {
        return Err(err(
            "'maxInclusive' and 'maxExclusive' cannot both be specified".to_string(),
        ));
    }

    let has_length_facet = length || min_length || max_length;
    let has_digit_facet = total_digits || fraction_digits;
    let range_facets = [
        ("minInclusive", min_inclusive),
        ("minExclusive", min_exclusive),
        ("maxInclusive", max_inclusive),
        ("maxExclusive", max_exclusive),
    ];

    match base {
        FacetBase::Primitive(kind) => {
            if has_length_facet && !length_applicable(kind) {
                return Err(err(format!(
                    "length facets are not applicable to a {kind:?} base"
                )));
            }
            if has_digit_facet && !digits_applicable(kind) {
                return Err(err(format!(
                    "digit facets are not applicable to a {kind:?} base"
                )));
            }
            for (name, value) in range_facets {
                if let Some(value) = value {
                    if !ordered(kind) {
                        return Err(err(format!(
                            "'{name}' is not applicable to an unordered {kind:?} base"
                        )));
                    }
                    if kind.validate(value.trim()).is_err() {
                        return Err(SchemaError::InvalidFacetValue {
                            facet: name.to_string(),
                            value: value.to_string(),
                            reason: format!("not a valid {kind:?} literal"),
                        }
                        .into());
                    }
                }
            }
            for facet in facets {
                if let XsdFacet::Enumeration(value) = facet
                    && kind.validate(value.trim()).is_err()
                {
                    return Err(SchemaError::InvalidFacetValue {
                        facet: "enumeration".to_string(),
                        value: value.to_string(),
                        reason: format!("not a valid {kind:?} literal"),
                    }
                    .into());
                }
            }
            // Range coherence (only when both bounds are comparable).
            let kind = Some(kind);
            for (lo_name, lo, hi_name, hi) in [
                ("minInclusive", min_inclusive, "maxInclusive", max_inclusive),
                ("minExclusive", min_exclusive, "maxExclusive", max_exclusive),
                ("minInclusive", min_inclusive, "maxExclusive", max_exclusive),
                ("minExclusive", min_exclusive, "maxInclusive", max_inclusive),
            ] {
                if let (Some(lo), Some(hi)) = (lo, hi)
                    && compare_values(kind, lo.trim(), hi.trim())
                        == Some(std::cmp::Ordering::Greater)
                {
                    return Err(err(format!(
                        "'{lo_name}' ({lo}) must not be greater than '{hi_name}' ({hi})"
                    )));
                }
            }
        }
        FacetBase::StringFamily => {
            for (name, value) in range_facets {
                if value.is_some() {
                    return Err(err(format!("'{name}' is not applicable to a string base")));
                }
            }
            if has_digit_facet {
                return Err(err(
                    "digit facets are not applicable to a string base".to_string()
                ));
            }
        }
        FacetBase::Unknown => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_base() -> FacetBase {
        FacetBase::Primitive(PrimitiveKind::Int)
    }

    #[test]
    fn bad_range_literal_rejected() {
        let facets = [XsdFacet::MinInclusive("not-a-number".into())];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn good_range_literal_accepted() {
        let facets = [
            XsdFacet::MinInclusive("1".into()),
            XsdFacet::MaxInclusive("10".into()),
        ];
        assert!(check_facets(&facets, int_base()).is_ok());
    }

    #[test]
    fn min_greater_than_max_rejected() {
        let facets = [
            XsdFacet::MinInclusive("7".into()),
            XsdFacet::MaxInclusive("1".into()),
        ];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn inclusive_and_exclusive_bound_rejected() {
        let facets = [
            XsdFacet::MaxInclusive("5".into()),
            XsdFacet::MaxExclusive("5".into()),
        ];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn length_with_max_length_rejected() {
        let facets = [XsdFacet::Length(5), XsdFacet::MaxLength(10)];
        assert!(check_facets(&facets, FacetBase::StringFamily).is_err());
    }

    #[test]
    fn zero_total_digits_rejected() {
        let facets = [XsdFacet::TotalDigits(0)];
        assert!(check_facets(&facets, FacetBase::Unknown).is_err());
    }

    #[test]
    fn range_on_string_rejected() {
        let facets = [XsdFacet::MinInclusive("1".into())];
        assert!(check_facets(&facets, FacetBase::StringFamily).is_err());
    }

    #[test]
    fn length_on_int_rejected() {
        let facets = [XsdFacet::Length(5)];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn digits_on_string_rejected() {
        let facets = [XsdFacet::FractionDigits(1)];
        assert!(check_facets(&facets, FacetBase::StringFamily).is_err());
    }

    #[test]
    fn bad_enumeration_literal_rejected() {
        let facets = [XsdFacet::Enumeration("twelve".into())];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn duplicate_facet_rejected() {
        let facets = [
            XsdFacet::MaxInclusive("5".into()),
            XsdFacet::MaxInclusive("6".into()),
        ];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn unknown_base_skips_value_checks() {
        let facets = [
            XsdFacet::MinInclusive("garbage".into()),
            XsdFacet::Length(5),
        ];
        assert!(check_facets(&facets, FacetBase::Unknown).is_ok());
    }
}
