//! Honest outcome model for conformance testing.
//!
//! Every test resolves to exactly one [`Outcome`]. Unlike the previous harness
//! (where any error counted as a pass for negative tests and skips silently
//! shrank denominators), the outcome model distinguishes:
//!
//! - [`Outcome::Pass`] / [`Outcome::Fail`] — a decided result: the implementation
//!   gave the right or wrong answer.
//! - [`Outcome::Unsupported`] — the test targets a feature fastxml deliberately
//!   does not implement (e.g. XML 1.1, 5th-edition-only rules, non-UTF-8
//!   encodings). Excluded from the pass-rate denominator, but always counted.
//! - [`Outcome::Blocked`] — the harness could not decide because of an
//!   infrastructure problem (missing file, unresolvable import, validator error
//!   on a document we expected to *validate*). Also excluded from the pass rate.
//! - [`Outcome::Panic`] — the implementation panicked. Always a decided failure.
//!
//! The pass rate is always `pass / (pass + fail + panic)` and coverage is
//! `decided / total`. There is exactly one rate formula in the whole crate.

use fastxml::Error;
use fastxml::parser::error::ParseError;
use fastxml::schema::error::SchemaError;
use serde::{Deserialize, Serialize};

/// The single outcome of running one conformance test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The implementation produced the correct result.
    Pass,
    /// The implementation produced an incorrect result.
    Fail,
    /// The test targets a feature fastxml deliberately does not support.
    Unsupported,
    /// The harness could not decide due to an infrastructure problem.
    Blocked,
    /// The implementation panicked.
    Panic,
}

impl Outcome {
    /// The kebab-case string form used in TSV baselines.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::Unsupported => "unsupported",
            Outcome::Blocked => "blocked",
            Outcome::Panic => "panic",
        }
    }

    /// Parse an outcome from its kebab-case string form.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Outcome::Pass),
            "fail" => Some(Outcome::Fail),
            "unsupported" => Some(Outcome::Unsupported),
            "blocked" => Some(Outcome::Blocked),
            "panic" => Some(Outcome::Panic),
            _ => None,
        }
    }

    /// Whether this outcome counts toward the decided denominator.
    pub fn is_decided(self) -> bool {
        matches!(self, Outcome::Pass | Outcome::Fail | Outcome::Panic)
    }

    /// A severity rank used by the baseline ratchet. Lower is better; `Pass` is
    /// the best possible outcome and `Panic` the worst.
    pub fn severity(self) -> u8 {
        match self {
            Outcome::Pass => 0,
            Outcome::Unsupported => 1,
            Outcome::Blocked => 2,
            Outcome::Fail => 3,
            Outcome::Panic => 4,
        }
    }
}

/// One recorded test result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRecord {
    /// The category this test belongs to (e.g. "valid", "not-wf", "instance-invalid").
    pub category: String,
    /// A stable, unique-within-category identifier for the test.
    pub id: String,
    /// The single outcome.
    pub outcome: Outcome,
    /// Human-readable context (error message, path, panic message). Never
    /// compared by the baseline ratchet — only shown to humans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TestRecord {
    /// Construct a record.
    pub fn new(
        category: impl Into<String>,
        id: impl Into<String>,
        outcome: Outcome,
        detail: Option<String>,
    ) -> Self {
        Self {
            category: category.into(),
            id: id.into(),
            outcome,
            detail,
        }
    }
}

/// Tally of outcomes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeCounts {
    /// Number of passes.
    pub pass: usize,
    /// Number of fails.
    pub fail: usize,
    /// Number of unsupported tests.
    pub unsupported: usize,
    /// Number of blocked tests.
    pub blocked: usize,
    /// Number of panics.
    pub panic: usize,
}

impl OutcomeCounts {
    /// Total number of tests counted.
    pub fn total(&self) -> usize {
        self.pass + self.fail + self.unsupported + self.blocked + self.panic
    }

    /// Number of decided tests (`pass + fail + panic`).
    pub fn decided(&self) -> usize {
        self.pass + self.fail + self.panic
    }

    /// Pass rate as a percentage of decided tests. `0.0` when nothing decided.
    pub fn pass_rate(&self) -> f64 {
        let decided = self.decided();
        if decided == 0 {
            0.0
        } else {
            100.0 * self.pass as f64 / decided as f64
        }
    }

    /// Coverage: fraction of total tests that were decided, as a percentage.
    pub fn coverage(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            100.0 * self.decided() as f64 / total as f64
        }
    }

    /// Record one outcome.
    pub fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Pass => self.pass += 1,
            Outcome::Fail => self.fail += 1,
            Outcome::Unsupported => self.unsupported += 1,
            Outcome::Blocked => self.blocked += 1,
            Outcome::Panic => self.panic += 1,
        }
    }
}

/// Classify how fastxml rejected a document that is *not* well-formed.
///
/// A not-well-formed test passes when fastxml rejects the document for a
/// parse-level reason. A [`ParseError::MemoryLimitExceeded`] is a resource
/// limit, not a well-formedness judgment, so it is a `Fail`. An I/O error means
/// we could not read the input, so it is `Blocked`.
pub fn classify_notwf_rejection(err: &Error) -> Outcome {
    match err {
        Error::Parse(pe) => match pe {
            ParseError::NotWellFormed { .. }
            | ParseError::Generic { .. }
            | ParseError::AtPosition { .. }
            | ParseError::AttributeError { .. }
            | ParseError::AttributeDecodeError { .. }
            | ParseError::TextDecodeError { .. } => Outcome::Pass,
            ParseError::MemoryLimitExceeded { .. } => Outcome::Fail,
        },
        Error::Io(_) => Outcome::Blocked,
        _ => Outcome::Fail,
    }
}

/// The nature of a schema error, used to classify schema-compilation outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaErrorKind {
    /// A schema-for-schemas constraint violation — a legitimate reason to
    /// reject an invalid schema.
    ConstraintViolation,
    /// An infrastructure problem (missing import, bad base URI, circular
    /// dependency) — we could not decide.
    Infrastructure,
    /// The schema document could not even be parsed as XML / XSD.
    ParseLevel,
    /// Anything else.
    Other,
}

/// Categorize a fastxml [`Error`] returned while compiling a schema.
pub fn schema_error_kind(err: &Error) -> SchemaErrorKind {
    match err {
        Error::Schema(se) => match se {
            SchemaError::InvalidSchema { .. }
            | SchemaError::DanglingReference { .. }
            | SchemaError::InvalidOccurs { .. }
            | SchemaError::MinOccursGreaterThanMaxOccurs { .. }
            | SchemaError::InvalidFacetValue { .. }
            | SchemaError::MinLengthGreaterThanMaxLength { .. }
            | SchemaError::FractionDigitsGreaterThanTotalDigits { .. } => {
                SchemaErrorKind::ConstraintViolation
            }
            SchemaError::SchemaNotFound { .. }
            | SchemaError::InvalidBaseUri { .. }
            | SchemaError::UrlResolutionFailed { .. }
            | SchemaError::CircularDependency { .. } => SchemaErrorKind::Infrastructure,
        },
        // The XSD document failed to parse as XML or structurally as XSD. Per
        // the audit-informed policy this is "rejected for the wrong reason":
        // an invalid-schema test wants a schema-constraint rejection, not a
        // syntax error.
        Error::Parse(_) | Error::XsdParse(_) => SchemaErrorKind::ParseLevel,
        Error::Io(_) => SchemaErrorKind::Infrastructure,
        _ => SchemaErrorKind::Other,
    }
}

/// Classify the rejection of a schema expected to be *invalid*.
///
/// Only a schema-constraint violation is a genuine `Pass`. Infrastructure
/// errors are `Blocked`; parse-level or other errors are a `Fail` (the schema
/// was rejected, but for the wrong reason).
pub fn classify_invalid_schema_rejection(err: &Error) -> Outcome {
    match schema_error_kind(err) {
        SchemaErrorKind::ConstraintViolation => Outcome::Pass,
        SchemaErrorKind::Infrastructure => Outcome::Blocked,
        SchemaErrorKind::ParseLevel | SchemaErrorKind::Other => Outcome::Fail,
    }
}

/// Classify the failure of a schema expected to be *valid*.
///
/// Infrastructure errors are `Blocked` (we could not fetch/resolve, so cannot
/// decide); anything else is a `Fail`.
pub fn classify_valid_schema_failure(err: &Error) -> Outcome {
    match schema_error_kind(err) {
        SchemaErrorKind::Infrastructure => Outcome::Blocked,
        _ => Outcome::Fail,
    }
}

/// A short, stable name for the error variant, used by the audit histogram.
pub fn error_variant_name(err: &Error) -> &'static str {
    match err {
        Error::Parse(pe) => match pe {
            ParseError::AtPosition { .. } => "Parse::AtPosition",
            ParseError::MemoryLimitExceeded { .. } => "Parse::MemoryLimitExceeded",
            ParseError::TextDecodeError { .. } => "Parse::TextDecodeError",
            ParseError::AttributeDecodeError { .. } => "Parse::AttributeDecodeError",
            ParseError::AttributeError { .. } => "Parse::AttributeError",
            ParseError::Generic { .. } => "Parse::Generic",
            ParseError::NotWellFormed { .. } => "Parse::NotWellFormed",
        },
        Error::Io(_) => "Io",
        Error::XPathSyntax(_) => "XPathSyntax",
        Error::XPathEval(_) => "XPathEval",
        Error::Schema(se) => match se {
            SchemaError::InvalidOccurs { .. } => "Schema::InvalidOccurs",
            SchemaError::MinOccursGreaterThanMaxOccurs { .. } => {
                "Schema::MinOccursGreaterThanMaxOccurs"
            }
            SchemaError::InvalidFacetValue { .. } => "Schema::InvalidFacetValue",
            SchemaError::MinLengthGreaterThanMaxLength { .. } => {
                "Schema::MinLengthGreaterThanMaxLength"
            }
            SchemaError::FractionDigitsGreaterThanTotalDigits { .. } => {
                "Schema::FractionDigitsGreaterThanTotalDigits"
            }
            SchemaError::SchemaNotFound { .. } => "Schema::SchemaNotFound",
            SchemaError::InvalidBaseUri { .. } => "Schema::InvalidBaseUri",
            SchemaError::UrlResolutionFailed { .. } => "Schema::UrlResolutionFailed",
            SchemaError::CircularDependency { .. } => "Schema::CircularDependency",
            SchemaError::InvalidSchema { .. } => "Schema::InvalidSchema",
            SchemaError::DanglingReference { .. } => "Schema::DanglingReference",
        },
        Error::Validation { .. } => "Validation",
        Error::Namespace(_) => "Namespace",
        Error::Node(_) => "Node",
        Error::InvalidOperation(_) => "InvalidOperation",
        Error::Fetch(_) => "Fetch",
        Error::Utf8(_) => "Utf8",
        Error::FromUtf8(_) => "FromUtf8",
        Error::XsdParse(_) => "XsdParse",
        Error::Transform(_) => "Transform",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_math() {
        let mut c = OutcomeCounts::default();
        c.record(Outcome::Pass);
        c.record(Outcome::Pass);
        c.record(Outcome::Fail);
        c.record(Outcome::Unsupported);
        c.record(Outcome::Blocked);
        c.record(Outcome::Panic);
        assert_eq!(c.total(), 6);
        assert_eq!(c.decided(), 4); // 2 pass + 1 fail + 1 panic
        assert!((c.pass_rate() - 50.0).abs() < 1e-9);
        assert!((c.coverage() - 100.0 * 4.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn empty_counts_are_zero_not_nan() {
        let c = OutcomeCounts::default();
        assert_eq!(c.pass_rate(), 0.0);
        assert_eq!(c.coverage(), 0.0);
    }

    #[test]
    fn outcome_string_round_trip() {
        for o in [
            Outcome::Pass,
            Outcome::Fail,
            Outcome::Unsupported,
            Outcome::Blocked,
            Outcome::Panic,
        ] {
            assert_eq!(Outcome::from_str(o.as_str()), Some(o));
        }
        assert_eq!(Outcome::from_str("bogus"), None);
    }

    #[test]
    fn severity_ordering() {
        assert!(Outcome::Pass.severity() < Outcome::Unsupported.severity());
        assert!(Outcome::Unsupported.severity() < Outcome::Blocked.severity());
        assert!(Outcome::Blocked.severity() < Outcome::Fail.severity());
        assert!(Outcome::Fail.severity() < Outcome::Panic.severity());
    }

    #[test]
    fn notwf_classification() {
        assert_eq!(
            classify_notwf_rejection(&Error::Parse(ParseError::NotWellFormed {
                message: "x".into()
            })),
            Outcome::Pass
        );
        assert_eq!(
            classify_notwf_rejection(&Error::Parse(ParseError::Generic {
                message: "x".into()
            })),
            Outcome::Pass
        );
        assert_eq!(
            classify_notwf_rejection(&Error::Parse(ParseError::MemoryLimitExceeded {
                used: 10,
                max: 5
            })),
            Outcome::Fail
        );
        assert_eq!(
            classify_notwf_rejection(&Error::Io(std::io::Error::other("boom"))),
            Outcome::Blocked
        );
    }

    #[test]
    fn invalid_schema_classification() {
        assert_eq!(
            classify_invalid_schema_rejection(&Error::Schema(SchemaError::InvalidSchema {
                message: "bad".into()
            })),
            Outcome::Pass
        );
        assert_eq!(
            classify_invalid_schema_rejection(&Error::Schema(SchemaError::SchemaNotFound {
                uri: "u".into()
            })),
            Outcome::Blocked
        );
        // A schema rejected because its XSD failed to parse as XML is a
        // "rejected for the wrong reason" Fail, not a constraint pass.
        assert_eq!(
            classify_invalid_schema_rejection(&Error::Parse(ParseError::Generic {
                message: "x".into()
            })),
            Outcome::Fail
        );
    }

    #[test]
    fn valid_schema_failure_classification() {
        assert_eq!(
            classify_valid_schema_failure(&Error::Schema(SchemaError::CircularDependency {
                uri: "u".into()
            })),
            Outcome::Blocked
        );
        assert_eq!(
            classify_valid_schema_failure(&Error::Schema(SchemaError::InvalidSchema {
                message: "x".into()
            })),
            Outcome::Fail
        );
    }
}
