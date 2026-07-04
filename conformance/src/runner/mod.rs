//! Test-suite runners.
//!
//! The runners own the test-execution loops that used to live inline in
//! `tests/w3c_*.rs`. Extracting them here kills the DOM/streaming duplication
//! (one [`Engine`] parameter selects the parser/validator entry point) and lets
//! the binary `report` target reuse exactly the same logic the tests assert on.

pub mod xml;
pub mod xsd;

use crate::outcome::{Outcome, OutcomeCounts, TestRecord};
use std::collections::BTreeMap;
use std::path::Path;

/// The outcome of evaluating a single test body, plus optional audit info.
pub(crate) struct Eval {
    pub(crate) outcome: Outcome,
    pub(crate) detail: Option<String>,
    /// The error-variant name, when the run produced an error worth auditing.
    pub(crate) audit_variant: Option<&'static str>,
}

impl Eval {
    pub(crate) fn new(outcome: Outcome, detail: Option<String>) -> Self {
        Self {
            outcome,
            detail,
            audit_variant: None,
        }
    }
    pub(crate) fn with_audit(mut self, variant: &'static str) -> Self {
        self.audit_variant = Some(variant);
        self
    }
}

/// Render a path relative to `base`, for stable, machine-independent details.
pub(crate) fn rel(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Best-effort message from a caught panic payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("panic: {s}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("panic: {s}")
    } else {
        "panic".to_string()
    }
}

/// Which parsing/validation entry point to exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Parse into a DOM, then operate on the tree.
    Dom,
    /// Stream events without building a tree.
    Streaming,
}

impl Engine {
    /// A short, stable name used in report keys and baseline filenames.
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Dom => "dom",
            Engine::Streaming => "streaming",
        }
    }
}

/// The result of running a whole suite with one engine.
#[derive(Debug, Clone, Default)]
pub struct SuiteRun {
    /// Every test's record, in the order produced.
    pub records: Vec<TestRecord>,
    /// Aggregate counts across all categories.
    pub counts: OutcomeCounts,
    /// Per-category counts, keyed by category name (sorted).
    pub categories: BTreeMap<String, OutcomeCounts>,
}

impl SuiteRun {
    /// Record one test result, updating aggregate and per-category counts.
    pub fn push(&mut self, record: TestRecord) {
        self.counts.record(record.outcome);
        self.categories
            .entry(record.category.clone())
            .or_default()
            .record(record.outcome);
        self.records.push(record);
    }
}

/// Whether a test tagged with the given `VERSION` list applies to fastxml,
/// which targets XML 1.0. Absent version means unspecified (applies).
pub fn xml_version_applies(version: Option<&str>) -> bool {
    match version {
        None => true,
        Some(v) => v.split_whitespace().any(|t| t == "1.0"),
    }
}

/// Whether a test tagged with the given `EDITION` list applies to fastxml,
/// which targets the XML 1.0 4th edition. Applies when the edition is absent or
/// mentions any of editions 1-4. A "5"-only edition test does not apply.
pub fn xml_edition_applies(edition: Option<&str>) -> bool {
    match edition {
        None => true,
        Some(e) => e
            .split_whitespace()
            .any(|t| matches!(t, "1" | "2" | "3" | "4")),
    }
}

/// The result of sniffing a document's declared encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodingVerdict {
    /// UTF-8 / US-ASCII / unspecified: fastxml can run this document.
    Runnable,
    /// A non-UTF-8 encoding fastxml does not support; the string names it.
    Unsupported(String),
}

/// Determine whether fastxml can run a document based on a byte-order mark and
/// the `encoding=` pseudo-attribute of the XML declaration.
///
/// fastxml only decodes UTF-8. A UTF-16/UTF-32 BOM, or an explicit
/// `encoding="..."` naming anything other than UTF-8 / US-ASCII / ASCII, is
/// reported as [`EncodingVerdict::Unsupported`].
pub fn sniff_encoding(bytes: &[u8]) -> EncodingVerdict {
    // Byte-order marks take precedence over any declaration.
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        return EncodingVerdict::Unsupported("utf-32 BOM".to_string());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return EncodingVerdict::Unsupported("utf-16 BOM".to_string());
    }
    // A UTF-8 BOM is fine.
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    if let Some(enc) = declared_encoding(body) {
        let lower = enc.to_ascii_lowercase();
        if !matches!(lower.as_str(), "utf-8" | "us-ascii" | "ascii") {
            return EncodingVerdict::Unsupported(format!("encoding: {}", enc));
        }
    }
    EncodingVerdict::Runnable
}

/// Extract the `encoding="..."` pseudo-attribute value from an XML declaration,
/// if present. Only looks at the ASCII prefix, which is safe: the declaration
/// itself must be ASCII per the XML spec.
fn declared_encoding(bytes: &[u8]) -> Option<String> {
    // Only inspect the first line / declaration region.
    let prefix_len = bytes.len().min(256);
    let prefix = &bytes[..prefix_len];
    let text = std::str::from_utf8(prefix).ok()?;
    let decl_end = text.find("?>")?;
    let decl = &text[..decl_end];
    if !decl.trim_start().starts_with("<?xml") {
        return None;
    }
    let idx = decl.find("encoding")?;
    let after = &decl[idx + "encoding".len()..];
    let after = after.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Accumulates an audit histogram of `(category, expected, error-variant)` →
/// count so classification decisions can be sanity-checked.
#[derive(Debug, Default)]
pub struct AuditHistogram {
    counts: BTreeMap<(String, String, String), usize>,
}

impl AuditHistogram {
    /// Record one observed error.
    pub fn record(&mut self, category: &str, expected: &str, variant: &str) {
        *self
            .counts
            .entry((
                category.to_string(),
                expected.to_string(),
                variant.to_string(),
            ))
            .or_default() += 1;
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Print the histogram to stderr, sorted by descending count.
    pub fn print(&self, title: &str) {
        eprintln!();
        eprintln!("=== Audit histogram: {} ===", title);
        eprintln!("(category, expected, error-variant) -> count");
        let mut rows: Vec<_> = self.counts.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for ((cat, expected, variant), count) in rows {
            eprintln!("  {:>5}  {} | {} | {}", count, cat, expected, variant);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate() {
        assert!(xml_version_applies(None));
        assert!(xml_version_applies(Some("1.0")));
        assert!(xml_version_applies(Some("1.0 1.1")));
        assert!(!xml_version_applies(Some("1.1")));
    }

    #[test]
    fn edition_gate() {
        assert!(xml_edition_applies(None));
        assert!(xml_edition_applies(Some("1 2 3 4")));
        assert!(xml_edition_applies(Some("4")));
        assert!(xml_edition_applies(Some("2 5")));
        assert!(!xml_edition_applies(Some("5")));
    }

    #[test]
    fn encoding_sniff_bom() {
        assert_eq!(
            sniff_encoding(&[0xFE, 0xFF, 0x00, 0x3C]),
            EncodingVerdict::Unsupported("utf-16 BOM".to_string())
        );
        assert_eq!(
            sniff_encoding(&[0xFF, 0xFE, 0x3C, 0x00]),
            EncodingVerdict::Unsupported("utf-16 BOM".to_string())
        );
        assert_eq!(
            sniff_encoding(&[0x00, 0x00, 0xFE, 0xFF]),
            EncodingVerdict::Unsupported("utf-32 BOM".to_string())
        );
    }

    #[test]
    fn encoding_sniff_declaration() {
        assert_eq!(
            sniff_encoding(br#"<?xml version="1.0" encoding="UTF-8"?><a/>"#),
            EncodingVerdict::Runnable
        );
        assert_eq!(
            sniff_encoding(br#"<?xml version="1.0"?><a/>"#),
            EncodingVerdict::Runnable
        );
        assert_eq!(
            sniff_encoding(br#"<?xml version="1.0" encoding="ISO-8859-1"?><a/>"#),
            EncodingVerdict::Unsupported("encoding: ISO-8859-1".to_string())
        );
        assert_eq!(
            sniff_encoding(br#"<?xml version='1.0' encoding='Shift_JIS'?><a/>"#),
            EncodingVerdict::Unsupported("encoding: Shift_JIS".to_string())
        );
        // UTF-8 BOM followed by a UTF-8 declaration is runnable.
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(br#"<?xml version="1.0"?><a/>"#);
        assert_eq!(sniff_encoding(&with_bom), EncodingVerdict::Runnable);
    }
}
