//! Lean, allocation-free lexical scanners for the numeric value-check fast
//! path (PR-B).
//!
//! Streaming validation of large CityGML documents is dominated by numeric
//! value checking: a single `gml:posList` / `gml:coordinates` text node is one
//! string of tens of thousands of whitespace-separated doubles. The canonical
//! path ([`FacetValidator`](crate::schema::xsd::facets::FacetValidator) +
//! [`PrimitiveKind::validate`](crate::schema::xsd::primitive::PrimitiveKind))
//! collapse-normalizes the whole string, splits it, and runs a compiled regex
//! per item — the regex dispatch per token being the dominant cost.
//!
//! For the common shape — an *unconstrained* list (or scalar) of a numeric
//! primitive with `whiteSpace=collapse` and no other facets — the entire check
//! reduces to "every whitespace-delimited token is lexically a valid number".
//! These scanners answer exactly that question in one pass over the raw bytes,
//! with no allocation and no regex.
//!
//! ## Correctness contract
//!
//! These scanners are *accelerators*, never the source of truth for errors.
//! The engine only trusts a `true` result (all tokens valid → skip the slow
//! path). On `false` — or on any input shape the fast path does not model —
//! the engine falls back to the canonical slow path, which produces the
//! byte-identical error message. The single invariant that must hold is
//! therefore:
//!
//! > `scan_* == true`  ⟹  the canonical path would report **no** error.
//!
//! To keep clean documents on the fast path (and to make the invariant easy to
//! test), each per-token scanner is written to accept *exactly* the same set as
//! the corresponding [`PrimitiveKind`] lexical space. The property tests in
//! this module assert that equivalence directly against `PrimitiveKind`.

use crate::schema::xsd::facets::{FacetConstraints, WhitespaceHandling};
use crate::schema::xsd::primitive::PrimitiveKind;

/// Numeric lexical family handled by the fast path. Each maps to one XSD
/// lexical space; several [`PrimitiveKind`]s share a family (e.g. `float` and
/// `double`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumClass {
    /// `xs:double` / `xs:float`: decimal mantissa, optional exponent, plus the
    /// special literals `INF` / `+INF` / `-INF` / `NaN`.
    Double,
    /// `xs:decimal`: signed decimal, no exponent, no specials.
    Decimal,
    /// `xs:integer`: `[+-]?[0-9]+`, arbitrary precision, no range/sign bound.
    Integer,
}

impl NumClass {
    /// Maps a [`PrimitiveKind`] to a fast-path numeric family, or `None` if the
    /// kind is not handled here (bounded / sign-constrained integer subtypes,
    /// and every non-numeric kind, stay on the canonical path).
    ///
    /// Bounded integer subtypes (`xs:int`, `xs:unsignedLong`, …) and
    /// sign-constrained ones (`xs:nonNegativeInteger`, …) are intentionally
    /// excluded: their value-space checks (range parse, sign classification)
    /// are not the measured bottleneck, and replicating them here would add
    /// risk for no real-world gain. They remain fully correct via the slow
    /// path.
    fn from_kind(kind: PrimitiveKind) -> Option<Self> {
        match kind {
            PrimitiveKind::Double | PrimitiveKind::Float => Some(Self::Double),
            PrimitiveKind::Decimal => Some(Self::Decimal),
            PrimitiveKind::Integer => Some(Self::Integer),
            _ => None,
        }
    }

    /// Validates a single, already-whitespace-free token against this family's
    /// lexical space. Must be no more lenient than the corresponding
    /// [`PrimitiveKind`] regex (see the module-level invariant).
    #[inline]
    fn accepts_token(self, tok: &[u8]) -> bool {
        match self {
            Self::Double => is_double(tok),
            Self::Decimal => is_decimal(tok),
            Self::Integer => is_integer(tok),
        }
    }
}

/// The fast-path plan a set of facet constraints qualifies for, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericPlan {
    /// A single numeric scalar value.
    Scalar(NumClass),
    /// A whitespace-separated list of numeric items.
    List(NumClass),
}

/// Decides whether `constraints` describe an *unconstrained* numeric scalar or
/// list eligible for the fast path.
///
/// Eligibility requires that the canonical path would do nothing beyond the
/// per-value / per-item lexical check: no length, range, digit, pattern,
/// enumeration or explicit-timezone facet, and the fixed `whiteSpace=collapse`
/// handling (so tokenizing directly on whitespace is semantically exact).
pub(crate) fn classify(constraints: &FacetConstraints) -> Option<NumericPlan> {
    if !is_unconstrained(constraints) {
        return None;
    }
    if constraints.is_list {
        // A pure list carries no scalar value space of its own.
        if constraints.value_kind.is_some() {
            return None;
        }
        let class = NumClass::from_kind(constraints.item_kind?)?;
        Some(NumericPlan::List(class))
    } else {
        let class = NumClass::from_kind(constraints.value_kind?)?;
        Some(NumericPlan::Scalar(class))
    }
}

/// True when no facet beyond the fixed `whiteSpace=collapse` is present.
fn is_unconstrained(c: &FacetConstraints) -> bool {
    c.length.is_none()
        && c.min_length.is_none()
        && c.max_length.is_none()
        && c.min_inclusive.is_none()
        && c.max_inclusive.is_none()
        && c.min_exclusive.is_none()
        && c.max_exclusive.is_none()
        && c.total_digits.is_none()
        && c.fraction_digits.is_none()
        && c.patterns.is_empty()
        && c.enumeration.is_empty()
        && c.explicit_timezone.is_none()
        && c.whitespace == WhitespaceHandling::Collapse
}

/// ASCII XML whitespace (S production): space, tab, CR, LF.
#[inline]
fn is_xml_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

/// Fast check for a numeric **list** value: every whitespace-delimited token
/// must be lexically valid. An empty list (no tokens) is valid.
///
/// Tokenizes on ASCII XML whitespace only. Any Unicode-whitespace or otherwise
/// unexpected byte lands inside a token, fails the lexical check, and defers to
/// the slow path — so this never diverges from `split_whitespace` in a way that
/// could hide an error.
#[inline]
pub(crate) fn scan_list(bytes: &[u8], class: NumClass) -> bool {
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        // Skip whitespace between tokens.
        while i < n && is_xml_ws(bytes[i]) {
            i += 1;
        }
        if i >= n {
            break;
        }
        let start = i;
        while i < n && !is_xml_ws(bytes[i]) {
            i += 1;
        }
        if !class.accepts_token(&bytes[start..i]) {
            return false;
        }
    }
    true
}

/// Fast check for a numeric **scalar** value: after trimming XML whitespace the
/// remainder must be exactly one lexically valid token. An empty (all
/// whitespace) value is rejected here so it defers to the slow path, which
/// applies the nillable/default/fixed rules and the canonical empty-value
/// error.
#[inline]
pub(crate) fn scan_scalar(bytes: &[u8], class: NumClass) -> bool {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && is_xml_ws(bytes[start]) {
        start += 1;
    }
    while end > start && is_xml_ws(bytes[end - 1]) {
        end -= 1;
    }
    if start == end {
        return false;
    }
    // `accepts_token` rejects any interior whitespace (not a numeric char), so
    // a multi-token value like "1 2" correctly defers.
    class.accepts_token(&bytes[start..end])
}

// ---------------------------------------------------------------------------
// Per-token lexical scanners (byte-exact mirrors of the PrimitiveKind regexes)
// ---------------------------------------------------------------------------

/// `xs:integer` lexical space: `[+-]?[0-9]+`.
#[inline]
fn is_integer(b: &[u8]) -> bool {
    let digits = match b.first() {
        Some(b'+') | Some(b'-') => &b[1..],
        _ => b,
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// Unsigned decimal mantissa: `[0-9]+(\.[0-9]*)?|\.[0-9]+`.
///
/// Shared by [`is_decimal`] (with a leading sign already stripped) and
/// [`is_double`] (the part before any exponent).
#[inline]
fn is_unsigned_mantissa(b: &[u8]) -> bool {
    if b.is_empty() {
        return false;
    }
    let mut dot = None;
    for (j, &c) in b.iter().enumerate() {
        if c == b'.' {
            if dot.is_some() {
                return false; // more than one '.'
            }
            dot = Some(j);
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    match dot {
        // No dot: one or more digits (b is non-empty and all digits).
        None => true,
        Some(d) => {
            let before = d;
            let after = b.len() - d - 1;
            // `[0-9]+(\.[0-9]*)?` (before >= 1) OR `\.[0-9]+` (before == 0, after >= 1).
            before >= 1 || after >= 1
        }
    }
}

/// `xs:decimal` lexical space: `[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)`.
#[inline]
fn is_decimal(b: &[u8]) -> bool {
    let rest = match b.first() {
        Some(b'+') | Some(b'-') => &b[1..],
        _ => b,
    };
    is_unsigned_mantissa(rest)
}

/// `xs:double` / `xs:float` lexical space:
/// `[+-]?mantissa([eE][+-]?[0-9]+)?` or one of `INF` / `+INF` / `-INF` / `NaN`.
#[inline]
fn is_double(b: &[u8]) -> bool {
    // Special literals (case-sensitive), checked before sign stripping.
    match b {
        b"INF" | b"+INF" | b"-INF" | b"NaN" => return true,
        _ => {}
    }
    let rest = match b.first() {
        Some(b'+') | Some(b'-') => &b[1..],
        _ => b,
    };
    // Split off an optional exponent at the first 'e'/'E'.
    match rest.iter().position(|&c| c == b'e' || c == b'E') {
        Some(p) => {
            if !is_unsigned_mantissa(&rest[..p]) {
                return false;
            }
            let exp = &rest[p + 1..];
            let exp_digits = match exp.first() {
                Some(b'+') | Some(b'-') => &exp[1..],
                _ => exp,
            };
            !exp_digits.is_empty() && exp_digits.iter().all(u8::is_ascii_digit)
        }
        None => is_unsigned_mantissa(rest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The [`PrimitiveKind`] each [`NumClass`] must agree with, token-for-token.
    fn representative_kind(class: NumClass) -> PrimitiveKind {
        match class {
            NumClass::Double => PrimitiveKind::Double,
            NumClass::Decimal => PrimitiveKind::Decimal,
            NumClass::Integer => PrimitiveKind::Integer,
        }
    }

    /// A single whitespace-free token: the fast per-token scanner must agree
    /// exactly with the canonical `PrimitiveKind::validate`. (Only meaningful
    /// for whitespace-free tokens; `PrimitiveKind` collapses whitespace, but
    /// the scanner is only ever fed already-split tokens.)
    fn assert_token_agrees(class: NumClass, tok: &str) {
        assert!(
            !tok.bytes().any(is_xml_ws),
            "assert_token_agrees is only valid for whitespace-free tokens"
        );
        let fast = class.accepts_token(tok.as_bytes());
        let slow = representative_kind(class).validate(tok).is_ok();
        assert_eq!(
            fast, slow,
            "token {tok:?} for {class:?}: fast={fast} slow={slow}"
        );
    }

    // ---- explicit lexical cases ----

    #[test]
    fn integer_tokens() {
        for t in ["0", "42", "-42", "+42", "000", "-0"] {
            assert!(is_integer(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Integer, t);
        }
        for t in ["", "+", "-", "1.0", "1e2", "abc", "0x1", "+-1"] {
            assert!(!is_integer(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Integer, t);
        }
        // Interior whitespace: the scanner rejects (a real token is never split
        // this way), even though PrimitiveKind would collapse it.
        for t in ["1 2", " 1", "1\t2"] {
            assert!(!is_integer(t.as_bytes()), "{t}");
        }
    }

    #[test]
    fn decimal_tokens() {
        for t in ["0", "1.5", "-1.5", ".5", "1.", "+1.5", "-0.0", "00.00"] {
            assert!(is_decimal(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Decimal, t);
        }
        for t in ["", ".", "+", "-", "1e2", "1.2.3", "abc", "1..2", "+."] {
            assert!(!is_decimal(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Decimal, t);
        }
        assert!(!is_decimal(b"1 2"));
    }

    #[test]
    fn double_tokens() {
        for t in [
            "0", "1.5", "-1.5e-3", "1.2E10", "INF", "-INF", "+INF", "NaN", ".5", "1.", "1e2",
            "1E+2", "-0.0e0",
        ] {
            assert!(is_double(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Double, t);
        }
        for t in [
            "", "abc", "inf", "nan", "+NaN", "-NaN", "1.2.3", "1e", "1e+", "e5", "1ee5", "INFI",
            "1.5f",
        ] {
            assert!(!is_double(t.as_bytes()), "{t}");
            assert_token_agrees(NumClass::Double, t);
        }
        assert!(!is_double(b"1 2"));
    }

    // ---- list / scalar wrappers ----

    #[test]
    fn scan_list_basic() {
        assert!(scan_list(b"1.0 2.0 3.0", NumClass::Double));
        assert!(scan_list(b"  1.0\t2.0\n3.0  ", NumClass::Double));
        assert!(scan_list(b"", NumClass::Double)); // empty list is valid
        assert!(scan_list(b"   ", NumClass::Double)); // all-whitespace == empty
        assert!(scan_list(b"42", NumClass::Integer)); // single token
        assert!(!scan_list(b"1.0 abc 3.0", NumClass::Double));
        assert!(!scan_list(b"1 2 3.5", NumClass::Integer)); // 3.5 not an integer
    }

    #[test]
    fn scan_scalar_basic() {
        assert!(scan_scalar(b"42", NumClass::Integer));
        assert!(scan_scalar(b"  42  ", NumClass::Integer));
        assert!(scan_scalar(b"-1.5e3", NumClass::Double));
        assert!(!scan_scalar(b"", NumClass::Integer)); // empty defers
        assert!(!scan_scalar(b"   ", NumClass::Integer)); // whitespace-only defers
        assert!(!scan_scalar(b"1 2", NumClass::Integer)); // multi-token defers
        assert!(!scan_scalar(b"abc", NumClass::Double));
    }

    // ---- property-style agreement: fast per-token == PrimitiveKind ----

    /// A tiny xorshift PRNG so the property test is deterministic and
    /// dependency-free.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
            xs[(self.next() as usize) % xs.len()]
        }
    }

    /// Builds a random token from numeric-ish fragments so we stress the
    /// boundary between accept and reject.
    fn random_token(rng: &mut Rng) -> String {
        let signs = ["", "+", "-"];
        let cores = [
            "",
            "0",
            "1",
            "42",
            "007",
            "123456789",
            ".",
            ".5",
            "5.",
            "1.5",
            "1.2.3",
            "..",
            "e",
            "E",
            "e5",
            "1e",
            "e+",
            "1e2",
            "1E-3",
            "INF",
            "NaN",
            "inf",
            "nan",
            "abc",
            "0x1f",
            " ",
            "1 2",
            "+",
            "-",
            "1.5f",
            "9999999999999999999999",
        ];
        let n = 1 + (rng.next() as usize % 3);
        let mut s = String::from(rng.pick(&signs));
        for _ in 0..n {
            s.push_str(rng.pick(&cores));
        }
        s
    }

    #[test]
    fn property_fast_token_matches_primitive_kind() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for _ in 0..20_000 {
            let tok = random_token(&mut rng);
            for class in [NumClass::Double, NumClass::Decimal, NumClass::Integer] {
                let fast = class.accepts_token(tok.as_bytes());
                // The canonical path only sees whitespace-free tokens here
                // (the list is split first); skip generated tokens that carry
                // interior whitespace, since PrimitiveKind would collapse them.
                if tok.bytes().any(is_xml_ws) {
                    // Fast path must reject anything with interior whitespace.
                    assert!(!fast, "token {tok:?} with ws accepted by {class:?}");
                    continue;
                }
                let slow = representative_kind(class).validate(&tok).is_ok();
                assert_eq!(
                    fast, slow,
                    "token {tok:?} for {class:?}: fast={fast} slow={slow}"
                );
            }
        }
    }

    /// Whole-value agreement: a random whitespace-joined list must be accepted
    /// by `scan_list` iff every item passes the canonical `PrimitiveKind`, and
    /// a random scalar by `scan_scalar` iff its trimmed form does. This
    /// exercises the exact accept/reject decision the engine relies on.
    #[test]
    fn property_scan_list_and_scalar_match_slow_path() {
        let mut rng = Rng(0xDEADBEEFCAFEF00D);
        let seps = [" ", "  ", "\t", "\n", " \t "];
        for _ in 0..5_000 {
            let count = rng.next() as usize % 6; // 0..=5 items (0 == empty list)
            let mut items = Vec::new();
            for _ in 0..count {
                items.push(random_token(&mut rng));
            }
            for class in [NumClass::Double, NumClass::Decimal, NumClass::Integer] {
                let kind = representative_kind(class);

                // List: join with random whitespace separators.
                let mut joined = String::new();
                for (idx, it) in items.iter().enumerate() {
                    if idx > 0 {
                        joined.push_str(rng.pick(&seps));
                    }
                    joined.push_str(it);
                }
                let fast_list = scan_list(joined.as_bytes(), class);
                let slow_list = joined
                    .split_whitespace()
                    .all(|it| kind.validate(it).is_ok());
                assert_eq!(
                    fast_list, slow_list,
                    "list {joined:?} for {class:?}: fast={fast_list} slow={slow_list}"
                );

                // Scalar: take the first item (or empty) and pad with ws.
                let core = items.first().cloned().unwrap_or_default();
                let padded = format!("{}{}{}", rng.pick(&seps), core, rng.pick(&seps));
                let fast_scalar = scan_scalar(padded.as_bytes(), class);
                let trimmed =
                    padded.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r');
                // scan_scalar accepts only a single whitespace-free token.
                let slow_scalar = !trimmed.is_empty()
                    && !trimmed.bytes().any(is_xml_ws)
                    && kind.validate(trimmed).is_ok();
                assert_eq!(
                    fast_scalar, slow_scalar,
                    "scalar {padded:?} for {class:?}: fast={fast_scalar} slow={slow_scalar}"
                );
            }
        }
    }
}
