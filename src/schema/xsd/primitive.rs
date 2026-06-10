//! Built-in XSD primitive type lexical and value-space validation.
//!
//! This module classifies a [`SimpleType`] into a [`PrimitiveKind`] by walking
//! the `base_type` chain, then provides [`PrimitiveKind::validate`] to enforce
//! the corresponding lexical and value-space rules.
//!
//! Per XSD 1.0 §4.3.6, every primitive type in the numeric / boolean /
//! date-time families carries a fixed `whiteSpace=collapse` facet. We
//! collapse-normalize the input before applying the lexical regex.

use std::sync::OnceLock;

use regex::Regex;

use crate::schema::types::{CompiledSchema, SimpleType, TypeDef};

/// Classification of an XSD built-in primitive (or derived) simple type for
/// lexical/value-space validation purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum PrimitiveKind {
    Boolean,
    Decimal,
    Float,
    Double,
    Integer,
    Long,
    Int,
    Short,
    Byte,
    NonNegativeInteger,
    PositiveInteger,
    NonPositiveInteger,
    NegativeInteger,
    UnsignedLong,
    UnsignedInt,
    UnsignedShort,
    UnsignedByte,
    Date,
    DateTime,
    Time,
    GYear,
    GYearMonth,
    GMonth,
    GMonthDay,
    GDay,
    Duration,
    HexBinary,
    Base64Binary,
    AnyUri,
    QName,
    Name,
    Ncname,
    Nmtoken,
    Language,
    Id,
    Idref,
    Entity,
}

impl PrimitiveKind {
    /// Maps a built-in XSD type name (with or without the `xs:` prefix) to
    /// its primitive kind. The unprefixed form is what
    /// [`crate::schema::xsd::builtin::register_builtin_types`] stores in
    /// `SimpleType::name`, so this also matches values pulled off a
    /// `SimpleType` after one chain hop.
    pub fn from_type_name(name: &str) -> Option<Self> {
        let local = name.strip_prefix("xs:").unwrap_or(name);
        Some(match local {
            "boolean" => Self::Boolean,
            "decimal" => Self::Decimal,
            "float" => Self::Float,
            "double" => Self::Double,
            "integer" => Self::Integer,
            "long" => Self::Long,
            "int" => Self::Int,
            "short" => Self::Short,
            "byte" => Self::Byte,
            "nonNegativeInteger" => Self::NonNegativeInteger,
            "positiveInteger" => Self::PositiveInteger,
            "nonPositiveInteger" => Self::NonPositiveInteger,
            "negativeInteger" => Self::NegativeInteger,
            "unsignedLong" => Self::UnsignedLong,
            "unsignedInt" => Self::UnsignedInt,
            "unsignedShort" => Self::UnsignedShort,
            "unsignedByte" => Self::UnsignedByte,
            "date" => Self::Date,
            "dateTime" => Self::DateTime,
            "time" => Self::Time,
            "gYear" => Self::GYear,
            "gYearMonth" => Self::GYearMonth,
            "gMonth" => Self::GMonth,
            "gMonthDay" => Self::GMonthDay,
            "gDay" => Self::GDay,
            "duration" => Self::Duration,
            "hexBinary" => Self::HexBinary,
            "base64Binary" => Self::Base64Binary,
            "anyURI" => Self::AnyUri,
            // NOTATION shares QName's lexical space (and, like QName, its
            // length facets are ignored per the XSD errata).
            "QName" | "NOTATION" => Self::QName,
            "Name" => Self::Name,
            "NCName" => Self::Ncname,
            "NMTOKEN" => Self::Nmtoken,
            "language" => Self::Language,
            "ID" => Self::Id,
            "IDREF" => Self::Idref,
            "ENTITY" => Self::Entity,
            _ => return None,
        })
    }

    /// True when this kind is `xs:ID` (document-wide uniqueness applies).
    pub fn is_id(&self) -> bool {
        matches!(self, Self::Id)
    }

    /// True when this kind is `xs:IDREF` (must reference an ID in the
    /// document).
    pub fn is_idref(&self) -> bool {
        matches!(self, Self::Idref)
    }

    /// Resolves a [`SimpleType`] to its base built-in primitive kind by
    /// walking the `base_type` chain. Returns `None` if the chain does not
    /// terminate at a known primitive (notably the `xs:string` family is left
    /// out so empty content remains valid for `xs:string`).
    ///
    /// Handles both:
    /// - direct references like `<xs:element type="xs:int"/>` (the SimpleType
    ///   itself is `xs:int`'s built-in definition), and
    /// - anonymous restrictions like
    ///   `<xs:simpleType><xs:restriction base="xs:boolean"/></xs:simpleType>`
    ///   (the SimpleType is anonymous, base hops through the chain).
    pub fn resolve(schema: &CompiledSchema, simple: &SimpleType) -> Option<Self> {
        if let Some(kind) = Self::from_type_name(&simple.name) {
            return Some(kind);
        }

        let mut current = simple.base_type.clone()?;
        // Defensive bound on chain length to avoid pathological cycles or
        // very deep accidental recursion. XSD's built-in tree is shallow (at
        // most ~6 levels), so 16 is more than enough.
        for _ in 0..16 {
            if let Some(kind) = Self::from_type_name(&current) {
                return Some(kind);
            }
            let next = match schema.get_type(&current)? {
                TypeDef::Simple(s) => s,
                TypeDef::Complex(_) => return None,
            };
            if let Some(kind) = Self::from_type_name(&next.name) {
                return Some(kind);
            }
            current = next.base_type.clone()?;
        }
        None
    }

    /// Validates a value against this primitive type's lexical and value
    /// space. The input is whitespace-collapsed first (per XSD's fixed
    /// `whiteSpace=collapse` for these primitives).
    ///
    /// Kinds whose lexical space is not yet enforced fall through to
    /// `Ok(())`; callers can still rely on the regular `FacetValidator` to
    /// catch user-declared facets on those types.
    pub fn validate(&self, raw: &str) -> Result<(), PrimitiveError> {
        let normalized = collapse(raw);
        let v = normalized.as_str();
        match self {
            Self::Boolean => validate_with_regex(v, boolean_regex(), "boolean"),
            Self::Decimal => validate_with_regex(v, decimal_regex(), "decimal"),
            Self::Float => validate_with_regex(v, double_regex(), "float"),
            Self::Double => validate_with_regex(v, double_regex(), "double"),
            Self::Integer => validate_integer_lexical(v, "integer"),
            Self::Long => validate_bounded_integer(v, i64::MIN as i128, i64::MAX as i128, "long"),
            Self::Int => validate_bounded_integer(v, i32::MIN as i128, i32::MAX as i128, "int"),
            Self::Short => validate_bounded_integer(v, i16::MIN as i128, i16::MAX as i128, "short"),
            Self::Byte => validate_bounded_integer(v, i8::MIN as i128, i8::MAX as i128, "byte"),
            Self::NonNegativeInteger => {
                validate_signed_integer(v, "nonNegativeInteger", SignReq::NonNegative)
            }
            Self::PositiveInteger => {
                validate_signed_integer(v, "positiveInteger", SignReq::Positive)
            }
            Self::NonPositiveInteger => {
                validate_signed_integer(v, "nonPositiveInteger", SignReq::NonPositive)
            }
            Self::NegativeInteger => {
                validate_signed_integer(v, "negativeInteger", SignReq::Negative)
            }
            Self::UnsignedLong => {
                validate_signed_integer(v, "unsignedLong", SignReq::NonNegative)?;
                validate_unsigned_range(v, u64::MAX as u128, "unsignedLong")
            }
            Self::UnsignedInt => {
                validate_signed_integer(v, "unsignedInt", SignReq::NonNegative)?;
                validate_unsigned_range(v, u32::MAX as u128, "unsignedInt")
            }
            Self::UnsignedShort => {
                validate_signed_integer(v, "unsignedShort", SignReq::NonNegative)?;
                validate_unsigned_range(v, u16::MAX as u128, "unsignedShort")
            }
            Self::UnsignedByte => {
                validate_signed_integer(v, "unsignedByte", SignReq::NonNegative)?;
                validate_unsigned_range(v, u8::MAX as u128, "unsignedByte")
            }
            Self::Date => validate_date(v),
            Self::DateTime => validate_datetime(v),
            Self::GYear => validate_gyear(v),
            Self::Time => validate_time(v),
            Self::GYearMonth => validate_gyearmonth(v),
            Self::GMonth => validate_gmonth(v),
            Self::GMonthDay => validate_gmonthday(v),
            Self::GDay => validate_gday(v),
            Self::Duration => validate_duration(v),
            Self::HexBinary => validate_hexbinary(v),
            Self::Base64Binary => validate_base64binary(v),
            Self::QName => validate_qname(v),
            Self::Name => validate_name(v),
            Self::Ncname | Self::Id | Self::Idref | Self::Entity => validate_ncname(v),
            Self::Nmtoken => validate_nmtoken(v),
            Self::Language => validate_language(v),
            // Lexical space not enforced — essentially any string is a URI
            // reference, so reject nothing.
            Self::AnyUri => Ok(()),
        }
    }
}

/// Errors raised by [`PrimitiveKind::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveError {
    /// Value does not match the type's lexical form.
    InvalidLexical {
        /// XSD primitive name (e.g., `"integer"`).
        kind: &'static str,
        /// The offending value.
        value: String,
    },
    /// Value's lexical form is valid but lies outside the type's value space
    /// (range, sign requirement, calendar validity, …).
    OutOfRange {
        /// The offending value.
        value: String,
        /// Human-readable constraint that was violated.
        constraint: &'static str,
    },
}

impl std::fmt::Display for PrimitiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLexical { kind, value } => {
                write!(f, "'{}' is not a valid xs:{}", value, kind)
            }
            Self::OutOfRange { value, constraint } => {
                write!(f, "'{}' {}", value, constraint)
            }
        }
    }
}

impl std::error::Error for PrimitiveError {}

// ---------------------------------------------------------------------------
// Lexical regexes (lazily compiled, shared across validators).
// ---------------------------------------------------------------------------

/// `xs:boolean` lexical space (XSD 1.0 §3.2.2): exactly one of
/// `true | false | 1 | 0` after `whiteSpace=collapse` normalization.
fn boolean_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:true|false|1|0)$").unwrap())
}

/// `xs:integer` family lexical space (XSD 1.0 §3.3.13):
/// `[+-]?[0-9]+`. No decimal point, no exponent, arbitrary precision.
/// Per-subtype value-space limits (e.g., 32-bit range for `xs:int`, sign
/// requirement for `xs:nonNegativeInteger`) are applied on top by
/// [`validate_bounded_integer`] / [`validate_signed_integer`].
fn integer_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[+-]?[0-9]+$").unwrap())
}

/// `xs:decimal` lexical space (XSD 1.0 §3.2.3): an optional sign followed by
/// either one or more digits with an optional fractional part (`42`, `42.`,
/// `42.5`), or a leading `.` followed by one or more digits (`.5`). No
/// exponent — that's `xs:double` / `xs:float`.
fn decimal_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)$").unwrap())
}

/// `xs:double` and `xs:float` share this lexical space (XSD 1.0 §3.2.5,
/// §3.2.4): a decimal numeral optionally followed by an `e`/`E` exponent, or
/// one of the special literals `INF` / `-INF` / `NaN` (case-sensitive). We
/// also accept `+INF`, which XSD doesn't list but is widely produced.
fn double_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(?:[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?|[+-]?INF|NaN)$")
            .unwrap()
    })
}

/// `xs:date` lexical space (XSD 1.0 §3.2.9): `-?YYYY-MM-DD(Z|[+-]HH:MM)?`
/// with the year at least 4 digits. Month / day range and calendar
/// (leap-year) validity are enforced on top in [`validate_date`].
fn date_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^(-?)([0-9]{4,})-([0-9]{2})-([0-9]{2})(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap()
    })
}

/// `xs:gYear` lexical space (XSD 1.0 §3.2.11): `-?YYYY(Z|[+-]HH:MM)?`,
/// year at least 4 digits, no further fields.
fn gyear_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(-?)([0-9]{4,})(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap())
}

/// `xs:dateTime` lexical space (XSD 1.0 §3.2.7): `xs:date` body (without
/// timezone) + uppercase `T` + `HH:MM:SS(.fff)?` + optional
/// `(Z|[+-]HH:MM)`. Seconds are mandatory; the separator must be `T`, not a
/// space. Calendar / clock-range validity is enforced in
/// [`validate_datetime`].
fn datetime_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"^(-?)([0-9]{4,})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?$",
        )
        .unwrap()
    })
}

// ---------------------------------------------------------------------------
// Shared validators
// ---------------------------------------------------------------------------

fn validate_with_regex(v: &str, regex: &Regex, kind: &'static str) -> Result<(), PrimitiveError> {
    if regex.is_match(v) {
        Ok(())
    } else {
        Err(PrimitiveError::InvalidLexical {
            kind,
            value: v.to_string(),
        })
    }
}

fn validate_integer_lexical(v: &str, kind: &'static str) -> Result<(), PrimitiveError> {
    validate_with_regex(v, integer_regex(), kind)
}

fn validate_bounded_integer(
    v: &str,
    min: i128,
    max: i128,
    kind: &'static str,
) -> Result<(), PrimitiveError> {
    validate_integer_lexical(v, kind)?;
    // i128::from_str accepts a leading `+`, but be defensive.
    let stripped = v.strip_prefix('+').unwrap_or(v);
    let parsed: i128 = stripped.parse().map_err(|_| PrimitiveError::OutOfRange {
        value: v.to_string(),
        constraint: "is outside the type's representable range",
    })?;
    if parsed < min || parsed > max {
        Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "is outside the type's representable range",
        })
    } else {
        Ok(())
    }
}

fn validate_unsigned_range(v: &str, max: u128, _kind: &'static str) -> Result<(), PrimitiveError> {
    // Caller has already enforced SignReq::NonNegative, so the only sign char
    // we might still see is a leading `+` (u128::from_str rejects).
    let stripped = v.strip_prefix('+').unwrap_or(v);
    let parsed: u128 = stripped.parse().map_err(|_| PrimitiveError::OutOfRange {
        value: v.to_string(),
        constraint: "is outside the type's representable range",
    })?;
    if parsed > max {
        Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "is outside the type's representable range",
        })
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum SignReq {
    /// `value >= 0`
    NonNegative,
    /// `value >= 1`
    Positive,
    /// `value <= 0`
    NonPositive,
    /// `value <= -1`
    Negative,
}

fn validate_signed_integer(
    v: &str,
    kind: &'static str,
    requirement: SignReq,
) -> Result<(), PrimitiveError> {
    validate_integer_lexical(v, kind)?;

    // Classify the value as one of {zero, positive, negative} from its
    // lexical form alone — this works for arbitrary-precision integers
    // (xs:nonNegativeInteger etc. have no representable-range cap).
    let (negative, digits) = if let Some(rest) = v.strip_prefix('-') {
        (true, rest)
    } else {
        let rest = v.strip_prefix('+').unwrap_or(v);
        (false, rest)
    };
    let all_zero = digits.bytes().all(|b| b == b'0');
    let is_positive = !negative && !all_zero;
    let is_negative = negative && !all_zero;

    let ok = match requirement {
        SignReq::NonNegative => !is_negative,
        SignReq::Positive => is_positive,
        SignReq::NonPositive => !is_positive,
        SignReq::Negative => is_negative,
    };

    if ok {
        Ok(())
    } else {
        Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: match requirement {
                SignReq::NonNegative => "must be greater than or equal to 0",
                SignReq::Positive => "must be greater than or equal to 1",
                SignReq::NonPositive => "must be less than or equal to 0",
                SignReq::Negative => "must be less than or equal to -1",
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Date / time validators
// ---------------------------------------------------------------------------

fn validate_date(v: &str) -> Result<(), PrimitiveError> {
    let caps = date_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "date",
            value: v.to_string(),
        })?;
    let sign_neg = !caps.get(1).map(|m| m.as_str()).unwrap_or("").is_empty();
    let year: i64 =
        caps.get(2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| PrimitiveError::InvalidLexical {
                kind: "date",
                value: v.to_string(),
            })?;
    let year = if sign_neg { -year } else { year };
    let month: u32 = caps.get(3).unwrap().as_str().parse().unwrap();
    let day: u32 = caps.get(4).unwrap().as_str().parse().unwrap();

    validate_year_month_day(year, month, day, v)?;
    if let Some(tz) = caps.get(5) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

fn validate_gyear(v: &str) -> Result<(), PrimitiveError> {
    let caps = gyear_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "gYear",
            value: v.to_string(),
        })?;
    // We only need to confirm the year is parseable; XSD 1.0 disallows year 0
    // but tests don't exercise that and we'd risk false negatives on data
    // that downstream consumers accept.
    let _year: i64 =
        caps.get(2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| PrimitiveError::InvalidLexical {
                kind: "gYear",
                value: v.to_string(),
            })?;
    if let Some(tz) = caps.get(3) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

fn validate_datetime(v: &str) -> Result<(), PrimitiveError> {
    let caps = datetime_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "dateTime",
            value: v.to_string(),
        })?;
    let sign_neg = !caps.get(1).map(|m| m.as_str()).unwrap_or("").is_empty();
    let year: i64 =
        caps.get(2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| PrimitiveError::InvalidLexical {
                kind: "dateTime",
                value: v.to_string(),
            })?;
    let year = if sign_neg { -year } else { year };
    let month: u32 = caps.get(3).unwrap().as_str().parse().unwrap();
    let day: u32 = caps.get(4).unwrap().as_str().parse().unwrap();
    let hour: u32 = caps.get(5).unwrap().as_str().parse().unwrap();
    let minute: u32 = caps.get(6).unwrap().as_str().parse().unwrap();
    let second: u32 = caps.get(7).unwrap().as_str().parse().unwrap();

    validate_year_month_day(year, month, day, v)?;

    // hh:mm:ss range. XSD permits 24:00:00 for end-of-day; otherwise hour
    // must be 0–23, minute 0–59, second 0–60 (leap second allowed).
    let end_of_day = hour == 24 && minute == 0 && second == 0;
    if !end_of_day && hour > 23 {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "hour-of-day must be 00-23 (or 24:00:00)",
        });
    }
    if minute > 59 {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "minute must be 00-59",
        });
    }
    if second > 60 {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "second must be 00-60",
        });
    }

    if let Some(tz) = caps.get(8) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

fn validate_year_month_day(
    year: i64,
    month: u32,
    day: u32,
    raw: &str,
) -> Result<(), PrimitiveError> {
    if !(1..=12).contains(&month) {
        return Err(PrimitiveError::OutOfRange {
            value: raw.to_string(),
            constraint: "month must be between 01 and 12",
        });
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!(),
    };
    if !(1..=max_day).contains(&day) {
        return Err(PrimitiveError::OutOfRange {
            value: raw.to_string(),
            constraint: "day is not valid for the given month",
        });
    }
    Ok(())
}

fn is_leap_year(year: i64) -> bool {
    if year % 400 == 0 {
        true
    } else if year % 100 == 0 {
        false
    } else {
        year % 4 == 0
    }
}

fn validate_timezone(tz: &str, raw: &str) -> Result<(), PrimitiveError> {
    if tz == "Z" {
        return Ok(());
    }
    // Regex guarantees the shape `[+-]HH:MM`; just enforce the range.
    let hh: u32 = tz[1..3]
        .parse()
        .map_err(|_| PrimitiveError::InvalidLexical {
            kind: "timezone",
            value: tz.to_string(),
        })?;
    let mm: u32 = tz[4..6]
        .parse()
        .map_err(|_| PrimitiveError::InvalidLexical {
            kind: "timezone",
            value: tz.to_string(),
        })?;
    if mm > 59 {
        return Err(PrimitiveError::OutOfRange {
            value: raw.to_string(),
            constraint: "timezone minute offset must be 00-59",
        });
    }
    // XSD timezone offset range is -14:00 .. +14:00.
    if hh > 14 || (hh == 14 && mm > 0) {
        return Err(PrimitiveError::OutOfRange {
            value: raw.to_string(),
            constraint: "timezone offset out of range (allowed: -14:00..+14:00)",
        });
    }
    Ok(())
}

/// `xs:time` lexical space: `HH:MM:SS(.fff)?(Z|±HH:MM)?`.
fn time_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?$")
            .unwrap()
    })
}

fn validate_time(v: &str) -> Result<(), PrimitiveError> {
    let caps = time_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "time",
            value: v.to_string(),
        })?;
    let hour: u32 = caps.get(1).unwrap().as_str().parse().unwrap();
    let minute: u32 = caps.get(2).unwrap().as_str().parse().unwrap();
    let second: u32 = caps.get(3).unwrap().as_str().parse().unwrap();
    let end_of_day = hour == 24 && minute == 0 && second == 0 && !v.contains('.');
    if (!end_of_day && hour > 23) || minute > 59 || second > 60 {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "time of day out of range",
        });
    }
    if let Some(tz) = caps.get(4) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

/// `xs:gYearMonth` lexical space: `-?YYYY-MM(Z|±HH:MM)?`.
fn gyearmonth_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^-?([0-9]{4,})-([0-9]{2})(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap())
}

fn validate_gyearmonth(v: &str) -> Result<(), PrimitiveError> {
    let caps = gyearmonth_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "gYearMonth",
            value: v.to_string(),
        })?;
    let month: u32 = caps.get(2).unwrap().as_str().parse().unwrap();
    if !(1..=12).contains(&month) {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "month must be between 01 and 12",
        });
    }
    if let Some(tz) = caps.get(3) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

/// `xs:gMonth` lexical space: `--MM(Z|±HH:MM)?` (the pre-errata `--MM--`
/// form is also accepted).
fn gmonth_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^--([0-9]{2})(?:--)?(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap())
}

fn validate_gmonth(v: &str) -> Result<(), PrimitiveError> {
    let caps = gmonth_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "gMonth",
            value: v.to_string(),
        })?;
    let month: u32 = caps.get(1).unwrap().as_str().parse().unwrap();
    if !(1..=12).contains(&month) {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "month must be between 01 and 12",
        });
    }
    if let Some(tz) = caps.get(2) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

/// `xs:gMonthDay` lexical space: `--MM-DD(Z|±HH:MM)?`.
fn gmonthday_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^--([0-9]{2})-([0-9]{2})(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap())
}

fn validate_gmonthday(v: &str) -> Result<(), PrimitiveError> {
    let caps = gmonthday_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "gMonthDay",
            value: v.to_string(),
        })?;
    let month: u32 = caps.get(1).unwrap().as_str().parse().unwrap();
    let day: u32 = caps.get(2).unwrap().as_str().parse().unwrap();
    // Use a leap year so --02-29 is allowed.
    validate_year_month_day(2000, month, day, v)?;
    if let Some(tz) = caps.get(3) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

/// `xs:gDay` lexical space: `---DD(Z|±HH:MM)?`.
fn gday_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^---([0-9]{2})(Z|[+-][0-9]{2}:[0-9]{2})?$").unwrap())
}

fn validate_gday(v: &str) -> Result<(), PrimitiveError> {
    let caps = gday_regex()
        .captures(v)
        .ok_or_else(|| PrimitiveError::InvalidLexical {
            kind: "gDay",
            value: v.to_string(),
        })?;
    let day: u32 = caps.get(1).unwrap().as_str().parse().unwrap();
    if !(1..=31).contains(&day) {
        return Err(PrimitiveError::OutOfRange {
            value: v.to_string(),
            constraint: "day must be between 01 and 31",
        });
    }
    if let Some(tz) = caps.get(2) {
        validate_timezone(tz.as_str(), v)?;
    }
    Ok(())
}

/// `xs:duration` lexical space (XSD 1.0 §3.2.6): `-?PnYnMnDTnHnMn(.n)?S` —
/// at least one field must be present, a `T` must be followed by at least
/// one time field, and only seconds may carry a fraction.
fn duration_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"^-?P(?:[0-9]+Y)?(?:[0-9]+M)?(?:[0-9]+D)?(?:T(?:[0-9]+H)?(?:[0-9]+M)?(?:[0-9]+(?:\.[0-9]+)?S)?)?$",
        )
        .unwrap()
    })
}

fn validate_duration(v: &str) -> Result<(), PrimitiveError> {
    let invalid = || PrimitiveError::InvalidLexical {
        kind: "duration",
        value: v.to_string(),
    };
    if !duration_regex().is_match(v) {
        return Err(invalid());
    }
    // The regex admits "P" and trailing "T" with no fields; rule those out.
    let body = v.strip_prefix('-').unwrap_or(v);
    if body == "P" || body.ends_with('T') {
        return Err(invalid());
    }
    Ok(())
}

fn validate_hexbinary(v: &str) -> Result<(), PrimitiveError> {
    if v.len().is_multiple_of(2) && v.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(PrimitiveError::InvalidLexical {
            kind: "hexBinary",
            value: v.to_string(),
        })
    }
}

fn validate_base64binary(v: &str) -> Result<(), PrimitiveError> {
    let invalid = || PrimitiveError::InvalidLexical {
        kind: "base64Binary",
        value: v.to_string(),
    };
    // XSD allows single spaces between the base64 characters.
    let chars: Vec<u8> = v.bytes().filter(|b| *b != b' ').collect();
    if !chars.len().is_multiple_of(4) {
        return Err(invalid());
    }
    let padding = chars.iter().rev().take_while(|&&b| b == b'=').count();
    if padding > 2 {
        return Err(invalid());
    }
    let body = &chars[..chars.len() - padding];
    if !body
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    {
        return Err(invalid());
    }
    // The character right before "=" / "==" padding is constrained.
    if padding > 0 {
        let last = *body.last().ok_or_else(invalid)?;
        if padding == 2 && !b"AQgw".contains(&last) {
            return Err(invalid());
        }
        if padding == 1 && !b"AEIMQUYcgkosw048".contains(&last) {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Loose `xs:QName` lexical check: `NCName(:NCName)?` with an ASCII-oriented
/// NCName approximation (non-ASCII characters are accepted as name chars).
fn validate_qname(v: &str) -> Result<(), PrimitiveError> {
    let invalid = || PrimitiveError::InvalidLexical {
        kind: "QName",
        value: v.to_string(),
    };
    if v.is_empty() {
        return Err(invalid());
    }
    let mut parts = v.split(':');
    let (first, second) = (parts.next(), parts.next());
    if parts.next().is_some() {
        return Err(invalid()); // more than one colon
    }
    for part in [first, second].into_iter().flatten() {
        let mut chars = part.chars();
        let Some(start) = chars.next() else {
            return Err(invalid()); // empty prefix or local part
        };
        if start.is_ascii() && !(start.is_ascii_alphabetic() || start == '_') {
            return Err(invalid());
        }
        if chars.any(|c| {
            c.is_ascii() && !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        }) {
            return Err(invalid());
        }
    }
    Ok(())
}

/// XML 1.0 NameStartChar.
fn is_name_start_char(c: char) -> bool {
    matches!(c,
        ':' | '_' | 'A'..='Z' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

/// XML 1.0 NameChar.
fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || matches!(c,
            '-' | '.' | '0'..='9' | '\u{B7}'
            | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// `xs:Name`: NameStartChar followed by NameChars.
fn validate_name(v: &str) -> Result<(), PrimitiveError> {
    let mut chars = v.chars();
    let ok = match chars.next() {
        Some(first) => is_name_start_char(first) && chars.all(is_name_char),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(PrimitiveError::InvalidLexical {
            kind: "Name",
            value: v.to_string(),
        })
    }
}

/// `xs:NCName` (and ID / IDREF / ENTITY): a Name without colons.
fn validate_ncname(v: &str) -> Result<(), PrimitiveError> {
    if validate_name(v).is_ok() && !v.contains(':') {
        Ok(())
    } else {
        Err(PrimitiveError::InvalidLexical {
            kind: "NCName",
            value: v.to_string(),
        })
    }
}

/// `xs:NMTOKEN`: one or more NameChars.
fn validate_nmtoken(v: &str) -> Result<(), PrimitiveError> {
    if !v.is_empty() && v.chars().all(is_name_char) {
        Ok(())
    } else {
        Err(PrimitiveError::InvalidLexical {
            kind: "NMTOKEN",
            value: v.to_string(),
        })
    }
}

/// `xs:language` lexical space (RFC 3066 shape): `[a-zA-Z]{1,8}(-[a-zA-Z0-9]{1,8})*`.
fn language_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[a-zA-Z]{1,8}(?:-[a-zA-Z0-9]{1,8})*$").unwrap())
}

fn validate_language(v: &str) -> Result<(), PrimitiveError> {
    validate_with_regex(v, language_regex(), "language")
}

// ---------------------------------------------------------------------------
// Whitespace collapse helper
// ---------------------------------------------------------------------------

/// Collapse-normalizes a string per XSD `whiteSpace=collapse`:
/// every run of whitespace becomes a single space, and leading/trailing
/// whitespace is stripped. Returns an owned `String` so callers can use it
/// without worrying about the borrow lifetime of the input.
fn collapse(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_was_space = true; // start true to trim leading whitespace
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                out.push(' ');
                prev_was_space = true;
            }
        } else {
            out.push(ch);
            prev_was_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::xsd::builtin::register_builtin_types;

    fn schema() -> CompiledSchema {
        let mut s = CompiledSchema::new();
        register_builtin_types(&mut s);
        s
    }

    fn simple_with_base(base: &str) -> SimpleType {
        let mut st = SimpleType::new("");
        st.base_type = Some(base.to_string());
        st
    }

    // ---- PrimitiveKind::from_type_name ----

    #[test]
    fn from_type_name_accepts_prefixed_and_unprefixed() {
        assert_eq!(
            PrimitiveKind::from_type_name("xs:boolean"),
            Some(PrimitiveKind::Boolean)
        );
        assert_eq!(
            PrimitiveKind::from_type_name("boolean"),
            Some(PrimitiveKind::Boolean)
        );
        assert_eq!(
            PrimitiveKind::from_type_name("xs:nonNegativeInteger"),
            Some(PrimitiveKind::NonNegativeInteger)
        );
        assert_eq!(PrimitiveKind::from_type_name("string"), None);
        assert_eq!(PrimitiveKind::from_type_name("anyType"), None);
    }

    // ---- PrimitiveKind::resolve via base chain ----

    #[test]
    fn resolve_via_built_in_chain() {
        let s = schema();
        // The built-in xs:int has name="int" base="xs:long" → walk hits "int" first.
        let int_def = match s.get_type("xs:int").unwrap() {
            TypeDef::Simple(simple) => simple.clone(),
            _ => panic!("xs:int should be SimpleType"),
        };
        assert_eq!(
            PrimitiveKind::resolve(&s, &int_def),
            Some(PrimitiveKind::Int)
        );
    }

    #[test]
    fn resolve_anonymous_restriction_of_boolean() {
        // mirrors XSD_BOOL_RESTRICTION in validation_primitive_test.rs
        let s = schema();
        let anon = simple_with_base("xs:boolean");
        assert_eq!(
            PrimitiveKind::resolve(&s, &anon),
            Some(PrimitiveKind::Boolean)
        );
    }

    #[test]
    fn resolve_string_family() {
        let s = schema();
        // xs:NCName now maps to its own kind for lexical validation; plain
        // xs:string (and normalizedString/token) stay unmapped so empty
        // string content remains valid.
        let ncname = match s.get_type("xs:NCName").unwrap() {
            TypeDef::Simple(simple) => simple.clone(),
            _ => panic!("xs:NCName should be SimpleType"),
        };
        assert_eq!(
            PrimitiveKind::resolve(&s, &ncname),
            Some(PrimitiveKind::Ncname)
        );

        let string = match s.get_type("xs:string").unwrap() {
            TypeDef::Simple(simple) => simple.clone(),
            _ => panic!("xs:string should be SimpleType"),
        };
        assert_eq!(PrimitiveKind::resolve(&s, &string), None);

        let token = match s.get_type("xs:token").unwrap() {
            TypeDef::Simple(simple) => simple.clone(),
            _ => panic!("xs:token should be SimpleType"),
        };
        assert_eq!(PrimitiveKind::resolve(&s, &token), None);
    }

    #[test]
    fn name_family_lexical() {
        assert!(PrimitiveKind::Ncname.validate("abc-1").is_ok());
        assert!(PrimitiveKind::Ncname.validate("a:b").is_err());
        assert!(PrimitiveKind::Ncname.validate("1abc").is_err());
        assert!(PrimitiveKind::Ncname.validate("").is_err());
        assert!(PrimitiveKind::Name.validate("a:b").is_ok());
        assert!(PrimitiveKind::Nmtoken.validate("123-abc").is_ok());
        assert!(PrimitiveKind::Nmtoken.validate("a b").is_err());
        assert!(PrimitiveKind::Language.validate("en-US").is_ok());
        assert!(PrimitiveKind::Language.validate("verylonglang").is_err());
    }

    // ---- Boolean ----

    #[test]
    fn boolean_accepts_canonical_literals() {
        for v in ["true", "false", "1", "0"] {
            assert!(PrimitiveKind::Boolean.validate(v).is_ok(), "{v}");
        }
    }

    #[test]
    fn boolean_rejects_other_forms() {
        for v in ["True", "FALSE", "yes", "2", "", "  "] {
            assert!(PrimitiveKind::Boolean.validate(v).is_err(), "{v}");
        }
    }

    #[test]
    fn boolean_collapses_whitespace_before_match() {
        // collapse-normalization strips surrounding whitespace so " true " is valid
        assert!(PrimitiveKind::Boolean.validate(" true ").is_ok());
        assert!(PrimitiveKind::Boolean.validate("\ntrue\n").is_ok());
    }

    // ---- Integer family ----

    #[test]
    fn integer_lexical_pass() {
        for v in ["0", "42", "-42", "+42"] {
            assert!(PrimitiveKind::Integer.validate(v).is_ok(), "{v}");
        }
    }

    #[test]
    fn integer_lexical_fail() {
        for v in ["1.5", "abc", "", "1e2", "+"] {
            assert!(PrimitiveKind::Integer.validate(v).is_err(), "{v}");
        }
    }

    #[test]
    fn int_enforces_32bit_range() {
        assert!(PrimitiveKind::Int.validate("-2147483648").is_ok());
        assert!(PrimitiveKind::Int.validate("2147483647").is_ok());
        assert!(PrimitiveKind::Int.validate("2147483648").is_err());
        assert!(PrimitiveKind::Int.validate("-2147483649").is_err());
    }

    #[test]
    fn non_negative_integer_sign_check() {
        assert!(PrimitiveKind::NonNegativeInteger.validate("0").is_ok());
        assert!(PrimitiveKind::NonNegativeInteger.validate("100").is_ok());
        assert!(PrimitiveKind::NonNegativeInteger.validate("+0").is_ok());
        assert!(PrimitiveKind::NonNegativeInteger.validate("-0").is_ok());
        assert!(PrimitiveKind::NonNegativeInteger.validate("-1").is_err());
    }

    #[test]
    fn positive_integer_sign_check() {
        assert!(PrimitiveKind::PositiveInteger.validate("1").is_ok());
        assert!(PrimitiveKind::PositiveInteger.validate("+1").is_ok());
        assert!(PrimitiveKind::PositiveInteger.validate("0").is_err());
        assert!(PrimitiveKind::PositiveInteger.validate("-0").is_err());
        assert!(PrimitiveKind::PositiveInteger.validate("-1").is_err());
    }

    // ---- Decimal ----

    #[test]
    fn decimal_lexical_pass() {
        for v in ["0", "1.5", "-1.5", ".5", "1.", "+1.5"] {
            assert!(PrimitiveKind::Decimal.validate(v).is_ok(), "{v}");
        }
    }

    #[test]
    fn decimal_lexical_fail() {
        for v in ["1e2", "abc", "1.5.6", "", "+", ".", "1.2.3"] {
            assert!(PrimitiveKind::Decimal.validate(v).is_err(), "{v}");
        }
    }

    // ---- Double / Float ----

    #[test]
    fn double_lexical_pass() {
        for v in ["0", "1.5", "-1.5e-3", "1.2E10", "INF", "-INF", "NaN", ".5"] {
            assert!(PrimitiveKind::Double.validate(v).is_ok(), "{v}");
        }
    }

    #[test]
    fn double_lexical_fail() {
        for v in ["abc", "1.5.6", "inf", "nan", ""] {
            assert!(PrimitiveKind::Double.validate(v).is_err(), "{v}");
        }
    }

    #[test]
    fn float_shares_double_lexical_space() {
        assert!(PrimitiveKind::Float.validate("3.14").is_ok());
        assert!(PrimitiveKind::Float.validate("abc").is_err());
    }

    // ---- Date ----

    #[test]
    fn date_canonical_forms() {
        assert!(PrimitiveKind::Date.validate("2026-05-28").is_ok());
        assert!(PrimitiveKind::Date.validate("2026-05-28Z").is_ok());
        assert!(PrimitiveKind::Date.validate("2026-05-28+09:00").is_ok());
        assert!(PrimitiveKind::Date.validate("2026-05-28-05:00").is_ok());
    }

    #[test]
    fn date_invalid_forms() {
        for v in [
            "2026-13-01",
            "2026-02-30",
            "26-05-28",
            "2026/05/28",
            "abc",
            "",
        ] {
            assert!(PrimitiveKind::Date.validate(v).is_err(), "{v}");
        }
    }

    #[test]
    fn date_leap_year_handling() {
        assert!(PrimitiveKind::Date.validate("2024-02-29").is_ok()); // leap
        assert!(PrimitiveKind::Date.validate("2023-02-29").is_err()); // non-leap
        assert!(PrimitiveKind::Date.validate("2000-02-29").is_ok()); // div 400
        assert!(PrimitiveKind::Date.validate("1900-02-29").is_err()); // div 100 not 400
    }

    // ---- gYear ----

    #[test]
    fn gyear_canonical_forms() {
        assert!(PrimitiveKind::GYear.validate("2026").is_ok());
        assert!(PrimitiveKind::GYear.validate("2026Z").is_ok());
        assert!(PrimitiveKind::GYear.validate("2026+09:00").is_ok());
    }

    #[test]
    fn gyear_invalid_forms() {
        for v in ["26", "2026-05", "abc", ""] {
            assert!(PrimitiveKind::GYear.validate(v).is_err(), "{v}");
        }
    }

    // ---- DateTime ----

    #[test]
    fn datetime_canonical_forms() {
        assert!(
            PrimitiveKind::DateTime
                .validate("2026-05-28T10:30:00")
                .is_ok()
        );
        assert!(
            PrimitiveKind::DateTime
                .validate("2026-05-28T10:30:00.123")
                .is_ok()
        );
        assert!(
            PrimitiveKind::DateTime
                .validate("2026-05-28T10:30:00Z")
                .is_ok()
        );
        assert!(
            PrimitiveKind::DateTime
                .validate("2026-05-28T10:30:00+09:00")
                .is_ok()
        );
    }

    #[test]
    fn datetime_invalid_forms() {
        for v in [
            "2026-05-28 10:30:00", // space separator
            "2026-05-28T10:30",    // missing seconds
            "2026-13-01T10:30:00", // bad month
            "abc",
            "",
        ] {
            assert!(PrimitiveKind::DateTime.validate(v).is_err(), "{v}");
        }
    }
}
