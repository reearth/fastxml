//! Conformance reporting built on the honest [`OutcomeCounts`] model.
//!
//! There is exactly one rate formula in the crate: [`OutcomeCounts::pass_rate`]
//! (`pass / decided`). Coverage (`decided / total`) is always reported next to
//! it so a high pass rate on a thin denominator is visible.

use crate::outcome::{OutcomeCounts, TestRecord};
use crate::runner::SuiteRun;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A report for one suite/engine combination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    /// Suite name, e.g. "w3c-xml".
    pub suite: String,
    /// Engine name, e.g. "dom".
    pub engine: String,
    /// Aggregate counts.
    pub counts: OutcomeCounts,
    /// Pass rate over decided tests (percentage).
    pub pass_rate: f64,
    /// Coverage: decided over total (percentage).
    pub coverage: f64,
    /// Per-category counts.
    pub categories: BTreeMap<String, OutcomeCounts>,
    /// Every non-pass record, for triage.
    pub non_pass: Vec<TestRecord>,
}

impl SuiteReport {
    /// Build a report from a suite run.
    pub fn from_run(suite: &str, engine: &str, run: &SuiteRun) -> Self {
        let non_pass = run
            .records
            .iter()
            .filter(|r| r.outcome != crate::outcome::Outcome::Pass)
            .cloned()
            .collect();
        Self {
            suite: suite.to_string(),
            engine: engine.to_string(),
            counts: run.counts,
            pass_rate: run.counts.pass_rate(),
            coverage: run.counts.coverage(),
            categories: run.categories.clone(),
            non_pass,
        }
    }
}

/// The full conformance report across all suites/engines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceReport {
    /// fastxml version under test.
    pub fastxml_version: String,
    /// Reports, one per suite/engine.
    pub reports: Vec<SuiteReport>,
}

impl ConformanceReport {
    /// Create an empty report.
    pub fn new() -> Self {
        Self {
            fastxml_version: env!("CARGO_PKG_VERSION").to_string(),
            reports: Vec::new(),
        }
    }

    /// Add a suite/engine report.
    pub fn add(&mut self, report: SuiteReport) {
        self.reports.push(report);
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl Default for ConformanceReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Print a single suite/engine run to stderr, always showing all five outcome
/// counts plus coverage, using the one rate formula.
pub fn print_suite_run(title: &str, run: &SuiteRun) {
    let c = &run.counts;
    eprintln!();
    eprintln!("=== {title} ===");
    eprintln!(
        "total={} pass={} fail={} unsupported={} blocked={} panic={}",
        c.total(),
        c.pass,
        c.fail,
        c.unsupported,
        c.blocked,
        c.panic,
    );
    eprintln!(
        "pass rate (pass/decided) = {:.1}%  |  coverage (decided/total) = {:.1}%",
        c.pass_rate(),
        c.coverage(),
    );
    for (cat, cc) in &run.categories {
        eprintln!(
            "  {cat}: pass={} fail={} unsupported={} blocked={} panic={} | rate={:.1}% coverage={:.1}%",
            cc.pass,
            cc.fail,
            cc.unsupported,
            cc.blocked,
            cc.panic,
            cc.pass_rate(),
            cc.coverage(),
        );
    }
}
