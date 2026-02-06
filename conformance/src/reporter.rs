//! Conformance test reporter for generating summary reports.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Conformance report for all test suites.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceReport {
    /// Report generation timestamp.
    pub timestamp: String,
    /// Version of fastxml being tested.
    pub fastxml_version: String,
    /// Results for each test suite.
    pub suites: HashMap<String, SuiteReport>,
}

/// Report for a single test suite.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SuiteReport {
    /// Total number of tests.
    pub total: usize,
    /// Number of passed tests.
    pub passed: usize,
    /// Number of failed tests.
    pub failed: usize,
    /// Number of skipped tests.
    pub skipped: usize,
    /// Number of expected failures (known limitations).
    pub expected_failures: usize,
    /// List of unexpected failures (test IDs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unexpected_failures: Vec<String>,
    /// Categories breakdown (if applicable).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub categories: HashMap<String, CategoryReport>,
}

/// Report for a test category.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CategoryReport {
    /// Total tests in this category.
    pub total: usize,
    /// Passed tests.
    pub passed: usize,
    /// Failed tests.
    pub failed: usize,
}

impl ConformanceReport {
    /// Create a new empty report.
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| {
                let secs = d.as_secs();
                // Simple ISO 8601-ish timestamp
                format!(
                    "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                    1970 + secs / 31536000,
                    (secs % 31536000) / 2592000 + 1,
                    (secs % 2592000) / 86400 + 1,
                    (secs % 86400) / 3600,
                    (secs % 3600) / 60,
                    secs % 60
                )
            })
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            timestamp,
            fastxml_version: env!("CARGO_PKG_VERSION").to_string(),
            suites: HashMap::new(),
        }
    }

    /// Add or update a suite report.
    pub fn add_suite(&mut self, name: &str, report: SuiteReport) {
        self.suites.insert(name.to_string(), report);
    }

    /// Calculate overall pass rate.
    pub fn overall_pass_rate(&self) -> f64 {
        let total: usize = self.suites.values().map(|s| s.total).sum();
        let passed: usize = self.suites.values().map(|s| s.passed).sum();

        if total == 0 {
            0.0
        } else {
            (passed as f64 / total as f64) * 100.0
        }
    }

    /// Print a summary to stdout.
    pub fn print_summary(&self) {
        println!("=== Conformance Report ===");
        println!("Timestamp: {}", self.timestamp);
        println!("fastxml version: {}", self.fastxml_version);
        println!();

        for (name, suite) in &self.suites {
            let pass_rate = if suite.total > 0 {
                (suite.passed as f64 / suite.total as f64) * 100.0
            } else {
                0.0
            };

            println!("--- {} ---", name);
            println!(
                "  Total: {}, Passed: {}, Failed: {}, Skipped: {}",
                suite.total, suite.passed, suite.failed, suite.skipped
            );
            if suite.expected_failures > 0 {
                println!("  Expected failures: {}", suite.expected_failures);
            }
            println!("  Pass rate: {:.1}%", pass_rate);

            if !suite.unexpected_failures.is_empty() {
                println!(
                    "  Unexpected failures ({}):",
                    suite.unexpected_failures.len()
                );
                for id in suite.unexpected_failures.iter().take(10) {
                    println!("    - {}", id);
                }
                if suite.unexpected_failures.len() > 10 {
                    println!("    ... and {} more", suite.unexpected_failures.len() - 10);
                }
            }

            if !suite.categories.is_empty() {
                println!("  By category:");
                for (cat, report) in &suite.categories {
                    let cat_rate = if report.total > 0 {
                        (report.passed as f64 / report.total as f64) * 100.0
                    } else {
                        0.0
                    };
                    println!(
                        "    {}: {}/{} ({:.1}%)",
                        cat, report.passed, report.total, cat_rate
                    );
                }
            }
            println!();
        }

        println!("Overall pass rate: {:.1}%", self.overall_pass_rate());
    }

    /// Export to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl Default for ConformanceReport {
    fn default() -> Self {
        Self::new()
    }
}

impl SuiteReport {
    /// Create a new empty suite report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a passed test.
    pub fn record_pass(&mut self, category: Option<&str>) {
        self.total += 1;
        self.passed += 1;
        if let Some(cat) = category {
            let entry = self.categories.entry(cat.to_string()).or_default();
            entry.total += 1;
            entry.passed += 1;
        }
    }

    /// Record a failed test.
    pub fn record_fail(&mut self, test_id: &str, category: Option<&str>) {
        self.total += 1;
        self.failed += 1;
        self.unexpected_failures.push(test_id.to_string());
        if let Some(cat) = category {
            let entry = self.categories.entry(cat.to_string()).or_default();
            entry.total += 1;
            entry.failed += 1;
        }
    }

    /// Record an expected failure.
    pub fn record_expected_fail(&mut self, category: Option<&str>) {
        self.total += 1;
        self.expected_failures += 1;
        if let Some(cat) = category {
            let entry = self.categories.entry(cat.to_string()).or_default();
            entry.total += 1;
        }
    }

    /// Record a skipped test.
    pub fn record_skip(&mut self) {
        self.total += 1;
        self.skipped += 1;
    }
}
