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
use crate::schema::xsd::types::{WhiteSpaceValue, XsdFacet};
use crate::schema::xsd::value_compare::compare_values;

/// What is known about a restriction's base type for facet checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FacetBase {
    /// A built-in datatype with a known primitive kind.
    Primitive(PrimitiveKind),
    /// The `xs:string` family (string / normalizedString / token), with its
    /// whiteSpace: length facets apply, range and digit facets do not.
    StringFamily(WhiteSpaceValue),
    /// A built-in list type (`IDREFS`, `NMTOKENS`, `ENTITIES`), which has
    /// `minLength` 1.
    BuiltinList,
    /// A user-defined or unresolved base: only base-independent checks run.
    Unknown,
}

/// Classifies a built-in XSD type local name for facet checking.
pub(crate) fn classify_builtin_base(local: &str) -> FacetBase {
    if let Some(kind) = PrimitiveKind::from_type_name(local) {
        return FacetBase::Primitive(kind);
    }
    match local {
        "string" => FacetBase::StringFamily(WhiteSpaceValue::Preserve),
        "normalizedString" => FacetBase::StringFamily(WhiteSpaceValue::Replace),
        "token" => FacetBase::StringFamily(WhiteSpaceValue::Collapse),
        "IDREFS" | "NMTOKENS" | "ENTITIES" => FacetBase::BuiltinList,
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

/// Whether the kind is `xs:integer` or derived from it (fractionDigits is
/// fixed at 0).
fn integer_family(kind: PrimitiveKind) -> bool {
    digits_applicable(kind) && kind != PrimitiveKind::Decimal
}

/// Whether a whiteSpace facet may restrict `base`: preserve loosens
/// replace, and both loosen collapse.
fn white_space_allowed(value: WhiteSpaceValue, base: WhiteSpaceValue) -> bool {
    use WhiteSpaceValue::*;
    match base {
        Preserve => true,
        Replace => value != Preserve,
        Collapse => value == Collapse,
    }
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
    let mut length_value = None;
    let mut min_length = false;
    let mut min_length_value = None;
    let mut max_length = false;
    let mut white_space = false;
    let mut white_space_value = None;
    let mut total_digits = false;
    let mut fraction_digits = false;
    let mut fraction_digits_value = None;
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
            XsdFacet::Length(n) => {
                dup(&mut length, "length")?;
                length_value = Some(*n);
            }
            XsdFacet::MinLength(n) => {
                dup(&mut min_length, "minLength")?;
                min_length_value = Some(*n);
            }
            XsdFacet::MaxLength(_) => dup(&mut max_length, "maxLength")?,
            XsdFacet::WhiteSpace(v) => {
                dup(&mut white_space, "whiteSpace")?;
                white_space_value = Some(*v);
            }
            XsdFacet::TotalDigits(n) => {
                dup(&mut total_digits, "totalDigits")?;
                if *n == 0 {
                    return Err(err(
                        "totalDigits must be a positive integer, found 0".to_string()
                    ));
                }
            }
            XsdFacet::FractionDigits(n) => {
                dup(&mut fraction_digits, "fractionDigits")?;
                fraction_digits_value = Some(*n);
            }
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

    // length excludes minLength/maxLength in the same step. The one
    // exception (W3C bug 6446) is a minLength that restates the built-in
    // list types' own minLength of 1.
    let restated_list_min_length = base == FacetBase::BuiltinList
        && min_length_value == Some(1)
        && length_value.is_some_and(|n| n >= 1);
    if length && (max_length || (min_length && !restated_list_min_length)) {
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
            if integer_family(kind) && fraction_digits_value.is_some_and(|n| n != 0) {
                return Err(err(format!(
                    "fractionDigits is fixed at 0 on a {kind:?} base"
                )));
            }
            // Every built-in that is not in the string family collapses.
            if white_space_value.is_some_and(|v| v != WhiteSpaceValue::Collapse) {
                return Err(err(format!(
                    "whiteSpace is fixed at 'collapse' on a {kind:?} base"
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
        FacetBase::StringFamily(base_ws) => {
            if let Some(v) = white_space_value
                && !white_space_allowed(v, base_ws)
            {
                return Err(err(format!(
                    "whiteSpace {v:?} cannot restrict a base with whiteSpace {base_ws:?}"
                )));
            }
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
        FacetBase::BuiltinList | FacetBase::Unknown => {}
    }

    Ok(())
}

/// The facets a type has, as written: each restriction step's facet
/// replaces the inherited one (a bound replaces both bounds at its end).
#[derive(Debug, Clone, Default)]
pub(crate) struct InheritedFacets {
    min_inclusive: Option<String>,
    min_exclusive: Option<String>,
    max_inclusive: Option<String>,
    max_exclusive: Option<String>,
    white_space: Option<WhiteSpaceValue>,
    total_digits: Option<u32>,
    fraction_digits: Option<u32>,
    length: Option<u32>,
    min_length: Option<u32>,
    max_length: Option<u32>,
}

impl InheritedFacets {
    /// The facets of a built-in type.
    pub(crate) fn builtin(base: FacetBase) -> Self {
        match base {
            FacetBase::Primitive(kind) => Self::builtin_primitive(kind),
            FacetBase::StringFamily(ws) => Self {
                white_space: Some(ws),
                ..Self::default()
            },
            FacetBase::BuiltinList => Self::list(Some(1)),
            FacetBase::Unknown => Self::default(),
        }
    }

    /// The facets of a list type: whiteSpace is fixed at collapse.
    pub(crate) fn list(min_length: Option<u32>) -> Self {
        Self {
            white_space: Some(WhiteSpaceValue::Collapse),
            min_length,
            ..Self::default()
        }
    }

    fn builtin_primitive(kind: PrimitiveKind) -> Self {
        use PrimitiveKind::*;
        let (min, max) = match kind {
            NonPositiveInteger => (None, Some("0")),
            NegativeInteger => (None, Some("-1")),
            NonNegativeInteger => (Some("0"), None),
            PositiveInteger => (Some("1"), None),
            Long => (Some("-9223372036854775808"), Some("9223372036854775807")),
            Int => (Some("-2147483648"), Some("2147483647")),
            Short => (Some("-32768"), Some("32767")),
            Byte => (Some("-128"), Some("127")),
            UnsignedLong => (Some("0"), Some("18446744073709551615")),
            UnsignedInt => (Some("0"), Some("4294967295")),
            UnsignedShort => (Some("0"), Some("65535")),
            UnsignedByte => (Some("0"), Some("255")),
            _ => (None, None),
        };
        Self {
            min_inclusive: min.map(str::to_string),
            max_inclusive: max.map(str::to_string),
            white_space: Some(WhiteSpaceValue::Collapse),
            fraction_digits: integer_family(kind).then_some(0),
            ..Self::default()
        }
    }

    /// These facets narrowed by one restriction step's `facets`.
    pub(crate) fn restricted_by(mut self, facets: &[XsdFacet]) -> Self {
        for facet in facets {
            match facet {
                XsdFacet::MinInclusive(v) => {
                    self.min_inclusive = Some(v.trim().to_string());
                    self.min_exclusive = None;
                }
                XsdFacet::MinExclusive(v) => {
                    self.min_exclusive = Some(v.trim().to_string());
                    self.min_inclusive = None;
                }
                XsdFacet::MaxInclusive(v) => {
                    self.max_inclusive = Some(v.trim().to_string());
                    self.max_exclusive = None;
                }
                XsdFacet::MaxExclusive(v) => {
                    self.max_exclusive = Some(v.trim().to_string());
                    self.max_inclusive = None;
                }
                XsdFacet::WhiteSpace(v) => self.white_space = Some(*v),
                XsdFacet::TotalDigits(n) => self.total_digits = Some(*n),
                XsdFacet::FractionDigits(n) => self.fraction_digits = Some(*n),
                XsdFacet::Length(n) => self.length = Some(*n),
                XsdFacet::MinLength(n) => self.min_length = Some(*n),
                XsdFacet::MaxLength(n) => self.max_length = Some(*n),
                XsdFacet::Enumeration(_) | XsdFacet::Pattern(_) | XsdFacet::ExplicitTimezone(_) => {
                }
            }
        }
        self
    }
}

/// Checks one restriction step's facets against the facets of its base
/// (XSD 1.0 Part 2, the "valid restriction" constraints of §4.3). Range
/// bounds are compared in `kind`'s value space and skipped without one, or
/// when they cannot be compared.
pub(crate) fn check_against_base(
    facets: &[XsdFacet],
    base: &InheritedFacets,
    kind: Option<PrimitiveKind>,
) -> Result<()> {
    for facet in facets {
        match facet {
            XsdFacet::WhiteSpace(v) => {
                if let Some(b) = base.white_space
                    && !white_space_allowed(*v, b)
                {
                    return Err(err(format!(
                        "whiteSpace {v:?} cannot restrict a base with whiteSpace {b:?}"
                    )));
                }
            }
            XsdFacet::TotalDigits(n) => not_above("totalDigits", *n, base.total_digits)?,
            XsdFacet::FractionDigits(n) => not_above("fractionDigits", *n, base.fraction_digits)?,
            XsdFacet::MaxLength(n) => not_above("maxLength", *n, base.max_length)?,
            XsdFacet::MinLength(n) => {
                if let Some(b) = base.min_length
                    && *n < b
                {
                    return Err(err(format!(
                        "minLength {n} is less than the base's minLength {b}"
                    )));
                }
            }
            XsdFacet::Length(n) => {
                if base.length.is_some_and(|b| b != *n) {
                    return Err(err(format!(
                        "length {n} differs from the base's length {}",
                        base.length.unwrap_or_default()
                    )));
                }
            }
            _ => {}
        }
    }
    if let Some(kind) = kind {
        check_range_against_base(facets, base, kind)?;
    }
    Ok(())
}

/// Errors when a restriction step's `name` facet exceeds the base's.
fn not_above(name: &str, value: u32, base: Option<u32>) -> Result<()> {
    match base {
        Some(b) if value > b => Err(err(format!(
            "{name} {value} is greater than the base's {name} {b}"
        ))),
        _ => Ok(()),
    }
}

/// A base bound a derived range facet is compared with: its name, its
/// value, and the orderings of (derived, base) that make the restriction
/// invalid.
type BoundCheck<'a> = (&'a str, &'a Option<String>, &'a [std::cmp::Ordering]);

/// Checks one restriction step's range facets against the bounds of its
/// base (§4.3.7–§4.3.10). Bounds that cannot be compared are skipped.
fn check_range_against_base(
    facets: &[XsdFacet],
    base: &InheritedFacets,
    kind: PrimitiveKind,
) -> Result<()> {
    use std::cmp::Ordering::{Equal, Greater, Less};
    for facet in facets {
        let (name, value, checks): (&str, &str, [BoundCheck; 4]) = match facet {
            XsdFacet::MinInclusive(v) => (
                "minInclusive",
                v,
                [
                    ("minInclusive", &base.min_inclusive, &[Less]),
                    ("maxInclusive", &base.max_inclusive, &[Greater]),
                    ("minExclusive", &base.min_exclusive, &[Less, Equal]),
                    ("maxExclusive", &base.max_exclusive, &[Greater, Equal]),
                ],
            ),
            XsdFacet::MaxInclusive(v) => (
                "maxInclusive",
                v,
                [
                    ("maxInclusive", &base.max_inclusive, &[Greater]),
                    ("minInclusive", &base.min_inclusive, &[Less]),
                    ("minExclusive", &base.min_exclusive, &[Less, Equal]),
                    ("maxExclusive", &base.max_exclusive, &[Greater, Equal]),
                ],
            ),
            XsdFacet::MinExclusive(v) => (
                "minExclusive",
                v,
                [
                    ("minExclusive", &base.min_exclusive, &[Less]),
                    ("maxInclusive", &base.max_inclusive, &[Greater]),
                    ("minInclusive", &base.min_inclusive, &[Less]),
                    ("maxExclusive", &base.max_exclusive, &[Greater, Equal]),
                ],
            ),
            XsdFacet::MaxExclusive(v) => (
                "maxExclusive",
                v,
                [
                    ("maxExclusive", &base.max_exclusive, &[Greater]),
                    ("maxInclusive", &base.max_inclusive, &[Greater]),
                    ("minInclusive", &base.min_inclusive, &[Less, Equal]),
                    ("minExclusive", &base.min_exclusive, &[Less, Equal]),
                ],
            ),
            _ => continue,
        };
        for (base_name, base_value, invalid) in checks {
            let Some(base_value) = base_value else {
                continue;
            };
            if let Some(order) = compare_values(Some(kind), value.trim(), base_value)
                && invalid.contains(&order)
            {
                return Err(err(format!(
                    "'{name}' ({value}) is not a valid restriction of the base's \
                     '{base_name}' ({base_value})"
                )));
            }
        }
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
        assert!(check_facets(&facets, classify_builtin_base("string")).is_err());
    }

    #[test]
    fn length_with_a_restated_list_min_length_accepted() {
        let facets = [XsdFacet::Length(5), XsdFacet::MinLength(1)];
        assert!(check_facets(&facets, FacetBase::BuiltinList).is_ok());
        assert!(check_facets(&facets, FacetBase::Primitive(PrimitiveKind::Id)).is_err());
        let facets = [XsdFacet::Length(5), XsdFacet::MinLength(2)];
        assert!(check_facets(&facets, FacetBase::BuiltinList).is_err());
        let facets = [XsdFacet::Length(5), XsdFacet::MaxLength(10)];
        assert!(check_facets(&facets, FacetBase::BuiltinList).is_err());
    }

    #[test]
    fn fraction_digits_on_an_integer_base_must_stay_zero() {
        let integer = FacetBase::Primitive(PrimitiveKind::Integer);
        assert!(check_facets(&[XsdFacet::FractionDigits(1)], integer).is_err());
        assert!(check_facets(&[XsdFacet::FractionDigits(0)], integer).is_ok());
        let decimal = FacetBase::Primitive(PrimitiveKind::Decimal);
        assert!(check_facets(&[XsdFacet::FractionDigits(1)], decimal).is_ok());
    }

    #[test]
    fn white_space_cannot_be_loosened() {
        use crate::schema::xsd::types::WhiteSpaceValue::*;
        let ws = |v| [XsdFacet::WhiteSpace(v)];
        let normalized = classify_builtin_base("normalizedString");
        assert!(check_facets(&ws(Preserve), normalized).is_err());
        assert!(check_facets(&ws(Replace), normalized).is_ok());
        let token = classify_builtin_base("token");
        assert!(check_facets(&ws(Replace), token).is_err());
        assert!(check_facets(&ws(Collapse), token).is_ok());
        assert!(check_facets(&ws(Preserve), int_base()).is_err());
        assert!(check_facets(&ws(Collapse), int_base()).is_ok());
        let string = classify_builtin_base("string");
        assert!(check_facets(&ws(Preserve), string).is_ok());
    }

    #[test]
    fn range_facets_must_stay_within_the_base_range() {
        let base = |facets: &[XsdFacet]| InheritedFacets::default().restricted_by(facets);
        let check = |derived: XsdFacet, base: &InheritedFacets| {
            check_range_against_base(&[derived], base, PrimitiveKind::Int)
        };
        let max_incl_10 = base(&[XsdFacet::MaxInclusive("10".into())]);
        let max_excl_10 = base(&[XsdFacet::MaxExclusive("10".into())]);
        let min_incl_10 = base(&[XsdFacet::MinInclusive("10".into())]);
        let min_excl_10 = base(&[XsdFacet::MinExclusive("10".into())]);
        use XsdFacet::*;
        for (derived, base, ok) in [
            (MaxInclusive("10".into()), &max_incl_10, true),
            (MaxInclusive("11".into()), &max_incl_10, false),
            (MaxInclusive("10".into()), &max_excl_10, false),
            (MaxInclusive("9".into()), &min_incl_10, false),
            (MaxExclusive("10".into()), &max_excl_10, true),
            (MaxExclusive("11".into()), &max_incl_10, false),
            (MaxExclusive("9".into()), &min_excl_10, false),
            (MinInclusive("10".into()), &min_incl_10, true),
            (MinInclusive("9".into()), &min_incl_10, false),
            (MinInclusive("10".into()), &max_excl_10, false),
            (MinInclusive("11".into()), &max_incl_10, false),
            (MinExclusive("10".into()), &min_excl_10, true),
            (MinExclusive("8".into()), &min_excl_10, false),
            (MinExclusive("10".into()), &max_excl_10, false),
        ] {
            assert_eq!(
                check(derived.clone(), base).is_ok(),
                ok,
                "{derived:?} vs {base:?}"
            );
        }
        let positive =
            InheritedFacets::builtin(FacetBase::Primitive(PrimitiveKind::PositiveInteger));
        assert!(
            check_range_against_base(
                &[MaxExclusive("1".into())],
                &positive,
                PrimitiveKind::PositiveInteger
            )
            .is_err()
        );
    }

    #[test]
    fn facets_must_not_loosen_the_base() {
        use crate::schema::xsd::types::WhiteSpaceValue::*;
        use XsdFacet::*;
        let base = InheritedFacets::default().restricted_by(&[
            WhiteSpace(Replace),
            TotalDigits(5),
            MinLength(2),
            MaxLength(8),
        ]);
        for (derived, ok) in [
            (WhiteSpace(Collapse), true),
            (WhiteSpace(Preserve), false),
            (TotalDigits(5), true),
            (TotalDigits(6), false),
            (MinLength(3), true),
            (MinLength(1), false),
            (MaxLength(8), true),
            (MaxLength(9), false),
        ] {
            assert_eq!(
                check_against_base(std::slice::from_ref(&derived), &base, None).is_ok(),
                ok,
                "{derived:?}"
            );
        }
        let fixed_length = InheritedFacets::default().restricted_by(&[Length(4)]);
        assert!(check_against_base(&[Length(4)], &fixed_length, None).is_ok());
        assert!(check_against_base(&[Length(5)], &fixed_length, None).is_err());
    }

    #[test]
    fn zero_total_digits_rejected() {
        let facets = [XsdFacet::TotalDigits(0)];
        assert!(check_facets(&facets, FacetBase::Unknown).is_err());
    }

    #[test]
    fn range_on_string_rejected() {
        let facets = [XsdFacet::MinInclusive("1".into())];
        assert!(check_facets(&facets, classify_builtin_base("string")).is_err());
    }

    #[test]
    fn length_on_int_rejected() {
        let facets = [XsdFacet::Length(5)];
        assert!(check_facets(&facets, int_base()).is_err());
    }

    #[test]
    fn digits_on_string_rejected() {
        let facets = [XsdFacet::FractionDigits(1)];
        assert!(check_facets(&facets, classify_builtin_base("string")).is_err());
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
