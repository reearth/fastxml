//! Baseline ratchet.
//!
//! A baseline is a committed TSV snapshot of every *non-pass* test result for
//! one suite/engine, plus a header line with the aggregate counts. The test
//! flow diffs a fresh run against the baseline:
//!
//! - Any test that got *worse* than its baseline (including a Pass that started
//!   failing) is a **regression** and fails the test.
//! - Any test that got *better* (a baseline non-pass that now passes, or a
//!   less-severe outcome) is an **improvement** and *also* fails the test, with
//!   a message telling you to regenerate the baseline.
//! - A change in the total number of tests is **count drift** and fails.
//!
//! `FASTXML_UPDATE_BASELINE=1` regenerates the files instead of asserting.
//!
//! Only the `(category, id, outcome)` triple participates in equality; the
//! `detail` column is informational and never compared (it can contain
//! machine-specific text such as byte offsets).

use crate::outcome::{Outcome, OutcomeCounts, TestRecord};
use std::collections::BTreeMap;
use std::path::Path;

/// A single non-pass entry stored in a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonPass {
    /// The recorded (non-pass) outcome.
    pub outcome: Outcome,
    /// Informational detail (never compared).
    pub detail: Option<String>,
}

/// A parsed or freshly-built baseline.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    /// Total number of tests the baseline was built from.
    pub total: usize,
    /// Aggregate counts.
    pub counts: OutcomeCounts,
    /// Non-pass records, keyed by `(category, id)`, sorted.
    pub records: BTreeMap<(String, String), NonPass>,
}

impl Baseline {
    /// Build a baseline from a full run's records.
    pub fn from_records(records: &[TestRecord]) -> Self {
        let mut counts = OutcomeCounts::default();
        let mut map = BTreeMap::new();
        for r in records {
            counts.record(r.outcome);
            if r.outcome != Outcome::Pass {
                map.insert(
                    (r.category.clone(), r.id.clone()),
                    NonPass {
                        outcome: r.outcome,
                        detail: r.detail.clone().map(|d| sanitize(&d)),
                    },
                );
            }
        }
        Self {
            total: records.len(),
            counts,
            records: map,
        }
    }

    /// Serialize to the committed TSV form.
    pub fn to_tsv(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "#counts\ttotal={}\tpass={}\tfail={}\tunsupported={}\tblocked={}\tpanic={}\n",
            self.total,
            self.counts.pass,
            self.counts.fail,
            self.counts.unsupported,
            self.counts.blocked,
            self.counts.panic,
        ));
        // BTreeMap iterates in sorted (category, id) order — deterministic.
        for ((category, id), np) in &self.records {
            out.push_str(category);
            out.push('\t');
            out.push_str(id);
            out.push('\t');
            out.push_str(np.outcome.as_str());
            out.push('\t');
            if let Some(detail) = &np.detail {
                out.push_str(detail);
            }
            out.push('\n');
        }
        out
    }

    /// Parse from the committed TSV form.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut total = 0usize;
        let mut counts = OutcomeCounts::default();
        let mut records = BTreeMap::new();
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("#counts") {
                for tok in rest.split('\t') {
                    let tok = tok.trim();
                    let parse_num =
                        |v: &str| v.parse::<usize>().map_err(|e| format!("bad count: {e}"));
                    if let Some(v) = tok.strip_prefix("total=") {
                        total = parse_num(v)?;
                    } else if let Some(v) = tok.strip_prefix("pass=") {
                        counts.pass = parse_num(v)?;
                    } else if let Some(v) = tok.strip_prefix("fail=") {
                        counts.fail = parse_num(v)?;
                    } else if let Some(v) = tok.strip_prefix("unsupported=") {
                        counts.unsupported = parse_num(v)?;
                    } else if let Some(v) = tok.strip_prefix("blocked=") {
                        counts.blocked = parse_num(v)?;
                    } else if let Some(v) = tok.strip_prefix("panic=") {
                        counts.panic = parse_num(v)?;
                    }
                }
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            let mut cols = line.splitn(4, '\t');
            let category = cols.next().ok_or("missing category")?.to_string();
            let id = cols.next().ok_or("missing id")?.to_string();
            let outcome_str = cols.next().ok_or("missing outcome")?;
            let outcome = Outcome::from_str(outcome_str)
                .ok_or_else(|| format!("bad outcome: {outcome_str}"))?;
            let detail = cols.next().filter(|s| !s.is_empty()).map(str::to_string);
            records.insert((category, id), NonPass { outcome, detail });
        }
        Ok(Self {
            total,
            counts,
            records,
        })
    }

    /// Load a baseline TSV file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text).map_err(std::io::Error::other)
    }

    /// Write a baseline TSV file (creating the parent directory if needed).
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_tsv())
    }

    /// Diff a fresh run against this baseline.
    pub fn diff(&self, records: &[TestRecord]) -> Diff {
        let mut diff = Diff::default();
        let mut seen: BTreeMap<(String, String), ()> = BTreeMap::new();

        for r in records {
            let key = (r.category.clone(), r.id.clone());
            seen.insert(key.clone(), ());
            let baseline_outcome = self
                .records
                .get(&key)
                .map(|np| np.outcome)
                .unwrap_or(Outcome::Pass);
            let actual = r.outcome;
            if actual == baseline_outcome {
                continue;
            }
            let entry = DiffEntry {
                category: r.category.clone(),
                id: r.id.clone(),
                from: baseline_outcome,
                to: actual,
                detail: r.detail.clone(),
            };
            if actual.severity() > baseline_outcome.severity() {
                diff.regressions.push(entry);
            } else {
                diff.improvements.push(entry);
            }
        }

        // A baseline non-pass that no longer appears in the run at all: the
        // test was removed or renamed. Treat as a regression so the baseline
        // must be regenerated deliberately.
        for (key, np) in &self.records {
            if !seen.contains_key(key) {
                diff.removed.push(DiffEntry {
                    category: key.0.clone(),
                    id: key.1.clone(),
                    from: np.outcome,
                    to: Outcome::Pass, // it vanished; unknown
                    detail: None,
                });
            }
        }

        if self.total != records.len() {
            diff.count_drift = Some((self.total, records.len()));
        }
        diff
    }
}

/// One test whose outcome differs from its baseline.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// Category.
    pub category: String,
    /// Test id.
    pub id: String,
    /// Baseline outcome.
    pub from: Outcome,
    /// Actual outcome.
    pub to: Outcome,
    /// Actual detail, for display.
    pub detail: Option<String>,
}

impl std::fmt::Display for DiffEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}: {} -> {}",
            self.category,
            self.id,
            self.from.as_str(),
            self.to.as_str()
        )?;
        if let Some(d) = &self.detail {
            write!(f, "  ({d})")?;
        }
        Ok(())
    }
}

/// Result of diffing a run against a baseline.
#[derive(Debug, Clone, Default)]
pub struct Diff {
    /// Tests that got worse (fail the ratchet).
    pub regressions: Vec<DiffEntry>,
    /// Tests that got better (require a baseline update).
    pub improvements: Vec<DiffEntry>,
    /// Baseline entries no longer present in the run.
    pub removed: Vec<DiffEntry>,
    /// `(baseline_total, actual_total)` when they differ.
    pub count_drift: Option<(usize, usize)>,
}

impl Diff {
    /// Whether the run matches the baseline exactly.
    pub fn is_clean(&self) -> bool {
        self.regressions.is_empty()
            && self.improvements.is_empty()
            && self.removed.is_empty()
            && self.count_drift.is_none()
    }

    /// A human-readable multi-line summary suitable for an assertion message.
    pub fn message(&self, baseline_name: &str) -> String {
        let mut s = format!("baseline mismatch for {baseline_name}:\n");
        if let Some((b, a)) = self.count_drift {
            s.push_str(&format!(
                "  COUNT DRIFT: baseline total {b} != actual total {a}. If the \
                 catalog changed intentionally, regenerate baselines.\n"
            ));
        }
        if !self.regressions.is_empty() {
            s.push_str(&format!("  {} REGRESSION(S):\n", self.regressions.len()));
            for e in &self.regressions {
                s.push_str(&format!("    {e}\n"));
            }
        }
        if !self.removed.is_empty() {
            s.push_str(&format!(
                "  {} baseline entr(y/ies) missing from the run:\n",
                self.removed.len()
            ));
            for e in &self.removed {
                s.push_str(&format!(
                    "    {}/{} ({})\n",
                    e.category,
                    e.id,
                    e.from.as_str()
                ));
            }
        }
        if !self.improvements.is_empty() {
            s.push_str(&format!(
                "  {} test(s) improved -- run `FASTXML_UPDATE_BASELINE=1 cargo test \
                 -p fastxml-conformance` and commit the updated baselines:\n",
                self.improvements.len()
            ));
            for e in &self.improvements {
                s.push_str(&format!("    {e}\n"));
            }
        }
        s
    }
}

/// Replace tab/newline with spaces so a detail string stays on one TSV column.
fn sanitize(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(cat: &str, id: &str, o: Outcome, detail: Option<&str>) -> TestRecord {
        TestRecord::new(cat, id, o, detail.map(str::to_string))
    }

    #[test]
    fn tsv_round_trip() {
        let records = vec![
            rec("valid", "a", Outcome::Pass, None),
            rec("valid", "b", Outcome::Fail, Some("boom")),
            rec("not-wf", "c", Outcome::Blocked, Some("read x")),
            rec("not-wf", "d", Outcome::Unsupported, Some("xml-1.1")),
        ];
        let baseline = Baseline::from_records(&records);
        let tsv = baseline.to_tsv();
        let parsed = Baseline::parse(&tsv).unwrap();
        assert_eq!(parsed.total, 4);
        // Only non-pass records are stored.
        assert_eq!(parsed.records.len(), 3);
        assert_eq!(
            parsed.records[&("valid".into(), "b".into())].outcome,
            Outcome::Fail
        );
        // Byte-identical on re-serialization (determinism).
        assert_eq!(Baseline::parse(&tsv).unwrap().to_tsv(), tsv);
    }

    #[test]
    fn diff_detects_regression() {
        let baseline = Baseline::from_records(&[rec("v", "a", Outcome::Pass, None)]);
        let actual = vec![rec("v", "a", Outcome::Fail, Some("now failing"))];
        let diff = baseline.diff(&actual);
        assert!(!diff.is_clean());
        assert_eq!(diff.regressions.len(), 1);
        assert!(diff.improvements.is_empty());
    }

    #[test]
    fn diff_detects_improvement() {
        let baseline = Baseline::from_records(&[rec("v", "a", Outcome::Fail, None)]);
        let actual = vec![rec("v", "a", Outcome::Pass, None)];
        let diff = baseline.diff(&actual);
        assert!(!diff.is_clean());
        assert_eq!(diff.improvements.len(), 1);
        assert!(diff.regressions.is_empty());
    }

    #[test]
    fn diff_ignores_detail_changes() {
        let baseline = Baseline::from_records(&[rec("v", "a", Outcome::Fail, Some("old detail"))]);
        let actual = vec![rec(
            "v",
            "a",
            Outcome::Fail,
            Some("totally different detail"),
        )];
        let diff = baseline.diff(&actual);
        assert!(diff.is_clean(), "detail must not affect equality");
    }

    #[test]
    fn diff_detects_count_drift() {
        let baseline = Baseline::from_records(&[
            rec("v", "a", Outcome::Pass, None),
            rec("v", "b", Outcome::Pass, None),
        ]);
        let actual = vec![rec("v", "a", Outcome::Pass, None)];
        let diff = baseline.diff(&actual);
        assert_eq!(diff.count_drift, Some((2, 1)));
        assert!(!diff.is_clean());
    }

    #[test]
    fn lateral_severity_change_classified() {
        // blocked (2) -> fail (3) is a regression; fail -> blocked an improvement.
        let baseline = Baseline::from_records(&[rec("v", "a", Outcome::Blocked, None)]);
        let actual = vec![rec("v", "a", Outcome::Fail, None)];
        assert_eq!(baseline.diff(&actual).regressions.len(), 1);

        let baseline = Baseline::from_records(&[rec("v", "a", Outcome::Fail, None)]);
        let actual = vec![rec("v", "a", Outcome::Blocked, None)];
        assert_eq!(baseline.diff(&actual).improvements.len(), 1);
    }
}
