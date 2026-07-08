//! Value-space comparison for XSD simple types.
//!
//! Range facets (`minInclusive`, `maxExclusive`, …) must be evaluated in the
//! *value space* of the type, not on the lexical form and not via lossy `f64`
//! conversion. This module implements ordering for the value spaces that
//! support range facets:
//!
//! - decimal / integer families: arbitrary-precision decimal comparison
//! - float / double: IEEE 754 comparison (including `INF` literals)
//! - date / dateTime / time / gYear / gYearMonth / gMonth / gMonthDay / gDay:
//!   component-wise comparison on the timeline
//! - duration: (months, seconds) comparison; indeterminate pairs return `None`
//!
//! All functions return `Option<Ordering>` — `None` means the pair is not
//! comparable (unparseable value or XSD-indeterminate order), in which case
//! the caller should skip the facet check rather than report an error.

use std::cmp::Ordering;

use super::primitive::PrimitiveKind;

/// Compares two lexical values in the value space of `kind`.
///
/// With `kind == None` the values are compared as `f64` when both parse,
/// matching the historical fallback behavior for types whose primitive base
/// is unknown.
pub fn compare_values(kind: Option<PrimitiveKind>, a: &str, b: &str) -> Option<Ordering> {
    match kind {
        Some(
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
            | PrimitiveKind::UnsignedByte,
        ) => compare_decimals(a, b),
        Some(PrimitiveKind::Float | PrimitiveKind::Double) => compare_floats(a, b),
        Some(
            PrimitiveKind::Date
            | PrimitiveKind::DateTime
            | PrimitiveKind::Time
            | PrimitiveKind::GYear
            | PrimitiveKind::GYearMonth
            | PrimitiveKind::GMonth
            | PrimitiveKind::GMonthDay
            | PrimitiveKind::GDay,
        ) => {
            let kind = kind.unwrap();
            Some(temporal_key(kind, a)?.total_cmp(&temporal_key(kind, b)?))
        }
        // dateTimeStamp shares dateTime's value space
        Some(PrimitiveKind::DateTimeStamp) => Some(
            temporal_key(PrimitiveKind::DateTime, a)?
                .total_cmp(&temporal_key(PrimitiveKind::DateTime, b)?),
        ),
        Some(
            PrimitiveKind::Duration
            | PrimitiveKind::DayTimeDuration
            | PrimitiveKind::YearMonthDuration,
        ) => compare_durations(a, b),
        Some(
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
            | PrimitiveKind::Entity,
        ) => None,
        None => {
            // Fallback: numeric comparison when both sides parse as f64.
            let a: f64 = a.parse().ok()?;
            let b: f64 = b.parse().ok()?;
            a.partial_cmp(&b)
        }
    }
}

/// Builds the identity-constraint comparison key for a field value.
///
/// The key is the value's canonical form within its primitive type's value
/// space (so xs:decimal `1`, `1.0` and `01` collapse to one key), tagged with
/// a discriminant identifying the *primitive* type. The discriminant makes
/// values of different primitive types compare unequal — xs:float `1` and
/// xs:decimal `1` are distinct keys — while values sharing a primitive type
/// (xs:int `1` and xs:integer `1`, both derived from decimal) collapse
/// together. This matches the value-space equality XSD identity constraints
/// require: two values are equal only when they belong to the same primitive
/// type. Types compared purely lexically (string-derived, QName, anyURI,
/// binary) carry no discriminant, preserving their existing behavior.
pub(crate) fn identity_key(kind: Option<PrimitiveKind>, v: &str) -> String {
    match primitive_class_tag(kind) {
        Some(tag) => {
            let canon = canonical_value(kind, v);
            let mut out = String::with_capacity(tag.len() + 1 + canon.len());
            out.push_str(tag);
            out.push('\u{1}');
            out.push_str(&canon);
            out
        }
        None => canonical_value(kind, v),
    }
}

/// A stable discriminant for the *primitive* type governing `kind`'s value
/// space, or `None` for types compared lexically (in which case identity keys
/// carry no tag). Types sharing a primitive (all the decimal-derived integer
/// types; both duration subtypes) share a tag so their equal values collapse.
fn primitive_class_tag(kind: Option<PrimitiveKind>) -> Option<&'static str> {
    use PrimitiveKind::*;
    Some(match kind? {
        Decimal | Integer | Long | Int | Short | Byte | NonNegativeInteger | PositiveInteger
        | NonPositiveInteger | NegativeInteger | UnsignedLong | UnsignedInt | UnsignedShort
        | UnsignedByte => "num",
        Float => "flt",
        Double => "dbl",
        Boolean => "bool",
        Date => "date",
        DateTime | DateTimeStamp => "dt",
        Time => "time",
        GYear => "gy",
        GYearMonth => "gym",
        GMonth => "gmo",
        GMonthDay => "gmd",
        GDay => "gd",
        Duration | DayTimeDuration | YearMonthDuration => "dur",
        // Compared lexically today; a discriminant would only add risk with no
        // failing test to justify it.
        HexBinary | Base64Binary | AnyUri | QName | Name | Ncname | Nmtoken | Language | Id
        | Idref | Entity => return None,
    })
}

/// Produces a canonical string for a lexical value in the given value
/// space, so equal values compare equal as strings (used by identity
/// constraint tuples). Unknown kinds return the input unchanged.
pub(crate) fn canonical_value(kind: Option<PrimitiveKind>, v: &str) -> String {
    let v = v.trim();
    match kind {
        Some(
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
            | PrimitiveKind::UnsignedByte,
        ) => match parse_decimal(v) {
            Some(d) => {
                let int = if d.int_digits.is_empty() {
                    "0"
                } else {
                    d.int_digits
                };
                let mut out = String::new();
                if d.negative {
                    out.push('-');
                }
                out.push_str(int);
                if !d.frac_digits.is_empty() {
                    out.push('.');
                    out.push_str(d.frac_digits);
                }
                out
            }
            None => v.to_string(),
        },
        Some(PrimitiveKind::Float | PrimitiveKind::Double) => match parse_xsd_float(v) {
            Some(f) => format!("{f}"),
            None => v.to_string(),
        },
        Some(
            PrimitiveKind::Date
            | PrimitiveKind::DateTime
            | PrimitiveKind::Time
            | PrimitiveKind::GYear
            | PrimitiveKind::GYearMonth
            | PrimitiveKind::GMonth
            | PrimitiveKind::GMonthDay
            | PrimitiveKind::GDay,
        ) => match temporal_key(kind.unwrap(), v) {
            Some(k) => format!("{k}"),
            None => v.to_string(),
        },
        Some(PrimitiveKind::Boolean) => match v {
            "1" => "true".to_string(),
            "0" => "false".to_string(),
            other => other.to_string(),
        },
        _ => v.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Decimal
// ---------------------------------------------------------------------------

/// A parsed decimal: sign + integer digits (no leading zeros) + fraction
/// digits (no trailing zeros).
struct Decimal<'a> {
    negative: bool,
    int_digits: &'a str,
    frac_digits: &'a str,
}

fn parse_decimal(v: &str) -> Option<Decimal<'_>> {
    let (negative, rest) = match v.as_bytes().first()? {
        b'-' => (true, &v[1..]),
        b'+' => (false, &v[1..]),
        _ => (false, v),
    };
    let (int_part, frac_part) = match rest.find('.') {
        Some(pos) => (&rest[..pos], &rest[pos + 1..]),
        None => (rest, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let int_digits = int_part.trim_start_matches('0');
    let frac_digits = frac_part.trim_end_matches('0');
    // -0 / +0 / 0.0 are all zero, not negative
    let is_zero = int_digits.is_empty() && frac_digits.is_empty();
    Some(Decimal {
        negative: negative && !is_zero,
        int_digits,
        frac_digits,
    })
}

/// Compares two decimal lexical values with arbitrary precision.
pub fn compare_decimals(a: &str, b: &str) -> Option<Ordering> {
    let da = parse_decimal(a)?;
    let db = parse_decimal(b)?;

    // Sign comparison
    match (da.negative, db.negative) {
        (false, true) => return Some(Ordering::Greater),
        (true, false) => return Some(Ordering::Less),
        _ => {}
    }

    // Magnitude comparison: longer integer part wins; same length compares
    // lexicographically; then the fraction compares lexicographically.
    let mag = da
        .int_digits
        .len()
        .cmp(&db.int_digits.len())
        .then_with(|| da.int_digits.cmp(db.int_digits))
        .then_with(|| da.frac_digits.cmp(db.frac_digits));

    Some(if da.negative { mag.reverse() } else { mag })
}

// ---------------------------------------------------------------------------
// Float / double
// ---------------------------------------------------------------------------

fn parse_xsd_float(v: &str) -> Option<f64> {
    match v {
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        _ => v.parse().ok(),
    }
}

/// Compares float/double values; NaN is incomparable.
pub fn compare_floats(a: &str, b: &str) -> Option<Ordering> {
    let a = parse_xsd_float(a)?;
    let b = parse_xsd_float(b)?;
    a.partial_cmp(&b)
}

// ---------------------------------------------------------------------------
// Date / time
// ---------------------------------------------------------------------------

/// Maps a temporal lexical value to a point on the timeline, in seconds.
///
/// Fields a kind does not carry are filled with fixed defaults (year 2000,
/// month/day 1, midnight) so values of the *same* kind compare consistently.
/// A timezone offset, when present, is applied. Mixed absent/present
/// timezones are compared as if absent meant UTC, which is the pragmatic
/// reading used by most validators for facet checks.
fn temporal_key(kind: PrimitiveKind, v: &str) -> Option<f64> {
    let p = parse_temporal(kind, v)?;
    let days = days_from_civil(p.year, p.month, p.day);
    let mut secs =
        days as f64 * 86_400.0 + p.hour as f64 * 3600.0 + p.minute as f64 * 60.0 + p.second;
    if let Some(offset_minutes) = p.tz_offset_minutes {
        secs -= offset_minutes as f64 * 60.0;
    }
    Some(secs)
}

struct TemporalParts {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: f64,
    tz_offset_minutes: Option<i32>,
}

impl Default for TemporalParts {
    fn default() -> Self {
        Self {
            year: 2000, // leap year so --02-29 stays representable
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0.0,
            tz_offset_minutes: None,
        }
    }
}

/// Splits a trailing timezone (`Z` or `±HH:MM`) off a temporal lexical value.
fn split_timezone(v: &str) -> (&str, Option<i32>) {
    if let Some(body) = v.strip_suffix('Z') {
        return (body, Some(0));
    }
    // ±HH:MM — but be careful not to confuse a leading sign or '-' date
    // separators; the offset is always exactly the last 6 chars.
    if v.len() > 6 {
        let (body, tail) = v.split_at(v.len() - 6);
        let bytes = tail.as_bytes();
        if (bytes[0] == b'+' || bytes[0] == b'-')
            && bytes[1].is_ascii_digit()
            && bytes[2].is_ascii_digit()
            && bytes[3] == b':'
            && bytes[4].is_ascii_digit()
            && bytes[5].is_ascii_digit()
        {
            let hh: i32 = tail[1..3].parse().unwrap_or(0);
            let mm: i32 = tail[4..6].parse().unwrap_or(0);
            let sign = if bytes[0] == b'-' { -1 } else { 1 };
            return (body, Some(sign * (hh * 60 + mm)));
        }
    }
    (v, None)
}

fn parse_temporal(kind: PrimitiveKind, v: &str) -> Option<TemporalParts> {
    let v = v.trim();
    let (body, tz) = split_timezone(v);
    let mut parts = TemporalParts {
        tz_offset_minutes: tz,
        ..Default::default()
    };

    match kind {
        PrimitiveKind::Date => parse_date_into(body, &mut parts)?,
        PrimitiveKind::DateTime => {
            let t_pos = body.find('T')?;
            parse_date_into(&body[..t_pos], &mut parts)?;
            parse_time_into(&body[t_pos + 1..], &mut parts)?;
        }
        PrimitiveKind::Time => parse_time_into(body, &mut parts)?,
        PrimitiveKind::GYear => {
            parts.year = body.parse().ok()?;
        }
        PrimitiveKind::GYearMonth => {
            // -?YYYY-MM: split on the last '-'
            let pos = body.rfind('-')?;
            if pos == 0 {
                return None;
            }
            parts.year = body[..pos].parse().ok()?;
            parts.month = body[pos + 1..].parse().ok()?;
        }
        PrimitiveKind::GMonth => {
            // --MM (also tolerate the pre-errata --MM-- form)
            let m = body.strip_prefix("--")?;
            let m = m.strip_suffix("--").unwrap_or(m);
            parts.month = m.parse().ok()?;
        }
        PrimitiveKind::GMonthDay => {
            let rest = body.strip_prefix("--")?;
            let (m, d) = rest.split_once('-')?;
            parts.month = m.parse().ok()?;
            parts.day = d.parse().ok()?;
        }
        PrimitiveKind::GDay => {
            parts.day = body.strip_prefix("---")?.parse().ok()?;
        }
        _ => return None,
    }
    Some(parts)
}

fn parse_date_into(body: &str, parts: &mut TemporalParts) -> Option<()> {
    // -?YYYY-MM-DD: month/day are the fixed-width tail
    if body.len() < 10 {
        return None;
    }
    let (y, rest) = body.split_at(body.len() - 6);
    parts.year = y.parse().ok()?;
    let rest = rest.strip_prefix('-')?;
    let (m, d) = rest.split_once('-')?;
    parts.month = m.parse().ok()?;
    parts.day = d.parse().ok()?;
    Some(())
}

fn parse_time_into(body: &str, parts: &mut TemporalParts) -> Option<()> {
    let mut it = body.splitn(3, ':');
    parts.hour = it.next()?.parse().ok()?;
    parts.minute = it.next()?.parse().ok()?;
    parts.second = it.next()?.parse().ok()?;
    Some(())
}

/// Days since 1970-01-01 from a proleptic Gregorian civil date
/// (Howard Hinnant's `days_from_civil` algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// Duration
// ---------------------------------------------------------------------------

/// Compares two `xs:duration` values.
///
/// Durations form a partial order: comparison is by (months, seconds) and
/// pairs whose components order in opposite directions are indeterminate.
pub fn compare_durations(a: &str, b: &str) -> Option<Ordering> {
    let (ma, sa) = parse_duration(a)?;
    let (mb, sb) = parse_duration(b)?;
    let month_ord = ma.cmp(&mb);
    let sec_ord = sa.partial_cmp(&sb)?;
    match (month_ord, sec_ord) {
        (Ordering::Equal, s) => Some(s),
        (m, Ordering::Equal) => Some(m),
        (m, s) if m == s => Some(m),
        _ => None, // indeterminate (e.g. P1M vs P30D)
    }
}

/// Parses an `xs:duration` into (total months, total seconds), both signed.
fn parse_duration(v: &str) -> Option<(i64, f64)> {
    let v = v.trim();
    let (negative, rest) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v),
    };
    let rest = rest.strip_prefix('P')?;

    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };

    let mut months: i64 = 0;
    let mut seconds: f64 = 0.0;
    let mut saw_field = false;

    let mut num_start = 0;
    let bytes = date_part.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'Y' => {
                months += date_part[num_start..i].parse::<i64>().ok()? * 12;
                num_start = i + 1;
                saw_field = true;
            }
            b'M' => {
                months += date_part[num_start..i].parse::<i64>().ok()?;
                num_start = i + 1;
                saw_field = true;
            }
            b'D' => {
                seconds += date_part[num_start..i].parse::<f64>().ok()? * 86_400.0;
                num_start = i + 1;
                saw_field = true;
            }
            _ => {}
        }
    }
    if num_start != date_part.len() {
        return None; // trailing digits without a designator
    }

    if let Some(time_part) = time_part {
        let mut num_start = 0;
        let bytes = time_part.as_bytes();
        let mut saw_time_field = false;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'H' => {
                    seconds += time_part[num_start..i].parse::<f64>().ok()? * 3600.0;
                    num_start = i + 1;
                    saw_time_field = true;
                }
                b'M' => {
                    seconds += time_part[num_start..i].parse::<f64>().ok()? * 60.0;
                    num_start = i + 1;
                    saw_time_field = true;
                }
                b'S' => {
                    seconds += time_part[num_start..i].parse::<f64>().ok()?;
                    num_start = i + 1;
                    saw_time_field = true;
                }
                _ => {}
            }
        }
        if !saw_time_field || num_start != time_part.len() {
            return None;
        }
        saw_field = true;
    }

    if !saw_field {
        return None;
    }
    if negative {
        months = -months;
        seconds = -seconds;
    }
    Some((months, seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_arbitrary_precision() {
        // f64 cannot distinguish these
        assert_eq!(
            compare_decimals("46766021207033324.9", "46766021207033325"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_decimals("46766021207033325.0", "46766021207033325"),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_decimals("-1", "1"), Some(Ordering::Less));
        assert_eq!(compare_decimals("-2", "-1"), Some(Ordering::Less));
        assert_eq!(compare_decimals("-0", "0"), Some(Ordering::Equal));
        assert_eq!(compare_decimals("+1.50", "1.5"), Some(Ordering::Equal));
        assert_eq!(compare_decimals("10", "9"), Some(Ordering::Greater));
        assert_eq!(compare_decimals("0.2", "0.10"), Some(Ordering::Greater));
        assert_eq!(compare_decimals("abc", "1"), None);
    }

    #[test]
    fn float_comparison() {
        assert_eq!(compare_floats("1e3", "999"), Some(Ordering::Greater));
        assert_eq!(compare_floats("-INF", "0"), Some(Ordering::Less));
        assert_eq!(compare_floats("INF", "1e308"), Some(Ordering::Greater));
        assert_eq!(compare_floats("NaN", "1"), None);
    }

    #[test]
    fn time_comparison() {
        let cmp = |a, b| compare_values(Some(PrimitiveKind::Time), a, b);
        assert_eq!(cmp("00:00:00", "02:50:21"), Some(Ordering::Less));
        assert_eq!(cmp("14:00:00", "02:50:21"), Some(Ordering::Greater));
        assert_eq!(cmp("02:50:21", "02:50:21"), Some(Ordering::Equal));
        assert_eq!(cmp("02:50:21.5", "02:50:21"), Some(Ordering::Greater));
    }

    #[test]
    fn date_comparison() {
        let cmp = |a, b| compare_values(Some(PrimitiveKind::Date), a, b);
        assert_eq!(cmp("1999-12-31", "2000-01-01"), Some(Ordering::Less));
        assert_eq!(cmp("2000-01-01Z", "2000-01-01"), Some(Ordering::Equal));
        assert_eq!(cmp("2000-01-01+09:00", "2000-01-01Z"), Some(Ordering::Less));
        assert_eq!(cmp("-0001-01-01", "0001-01-01"), Some(Ordering::Less));
    }

    #[test]
    fn datetime_comparison() {
        let cmp = |a, b| compare_values(Some(PrimitiveKind::DateTime), a, b);
        assert_eq!(
            cmp("2000-01-01T00:00:00", "2000-01-01T00:00:01"),
            Some(Ordering::Less)
        );
        assert_eq!(
            cmp("2000-01-01T09:00:00+09:00", "2000-01-01T00:00:00Z"),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn gregorian_partial_comparison() {
        assert_eq!(
            compare_values(Some(PrimitiveKind::GYear), "1999", "2000"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_values(Some(PrimitiveKind::GYearMonth), "1999-12", "2000-01"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_values(Some(PrimitiveKind::GMonth), "--03", "--11"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_values(Some(PrimitiveKind::GMonthDay), "--02-29", "--03-01"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_values(Some(PrimitiveKind::GDay), "---01", "---31"),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn duration_comparison() {
        assert_eq!(compare_durations("P1Y", "P12M"), Some(Ordering::Equal));
        assert_eq!(compare_durations("P1Y", "P13M"), Some(Ordering::Less));
        assert_eq!(compare_durations("PT1H", "PT59M"), Some(Ordering::Greater));
        assert_eq!(compare_durations("P1D", "PT24H"), Some(Ordering::Equal));
        assert_eq!(compare_durations("-P1D", "P1D"), Some(Ordering::Less));
        // indeterminate: 1 month vs 30 days
        assert_eq!(compare_durations("P1M", "P30D"), None);
        assert_eq!(
            compare_durations("P1Y2M3DT4H5M6S", "P14M3DT4H5M6S"),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn identity_key_distinguishes_primitive_types() {
        // Cross-primitive-type values are distinct keys even when their
        // canonical forms coincide (float 1 vs decimal 1, idF013/idF014).
        assert_ne!(
            identity_key(Some(PrimitiveKind::Float), "1"),
            identity_key(Some(PrimitiveKind::Decimal), "1")
        );
        assert_ne!(
            identity_key(Some(PrimitiveKind::Float), "1"),
            identity_key(Some(PrimitiveKind::UnsignedByte), "1")
        );
        // A typed numeric key never collides with an untyped lexical "1"
        // (idL090: xs:string "1" vs xs:decimal "1").
        assert_ne!(
            identity_key(Some(PrimitiveKind::Decimal), "1"),
            identity_key(None, "1")
        );
    }

    #[test]
    fn identity_key_collapses_within_primitive_class() {
        // The decimal-derived integer family shares decimal's value space.
        assert_eq!(
            identity_key(Some(PrimitiveKind::Int), "01"),
            identity_key(Some(PrimitiveKind::Integer), "1")
        );
        assert_eq!(
            identity_key(Some(PrimitiveKind::Decimal), "1.0"),
            identity_key(Some(PrimitiveKind::Decimal), "1")
        );
        // Lexically compared kinds are untagged and unchanged.
        assert_eq!(identity_key(None, " abc "), "abc");
        assert_eq!(identity_key(Some(PrimitiveKind::Ncname), "abc"), "abc");
    }
}
