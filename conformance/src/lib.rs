//! # fastxml-conformance
//!
//! Conformance test suite for fastxml - W3C/OASIS standard compliance testing.
//!
//! This crate provides test harnesses for running standard XML test suites:
//! - W3C XML Conformance Test Suite
//! - W3C XML Schema Test Suite
//! - OASIS XPath 1.0 Test Suite

pub mod baseline;
pub mod catalog;
pub mod downloader;
pub mod outcome;
pub mod reporter;
pub mod runner;

use std::path::PathBuf;

/// Get the path to conformance test data directory.
///
/// Returns the path to `conformance/data/` in the repository.
pub fn get_conformance_data_dir() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());

    // Data directory is inside conformance/
    manifest_dir.join("data")
}

/// Get path to a specific test suite's data directory.
///
/// Returns `Some(path)` if the directory exists, `None` otherwise.
pub fn get_test_data_path(suite: &str) -> Option<PathBuf> {
    let path = get_conformance_data_dir().join(suite);
    if path.exists() { Some(path) } else { None }
}

/// Check if test data download is requested via environment variable.
pub fn should_download_tests() -> bool {
    std::env::var("FASTXML_DOWNLOAD_TESTS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Macro to require test data, skipping the test if data is not available.
///
/// Usage:
/// ```ignore
/// #[test]
/// fn test_w3c_xml() {
///     let path = require_test_data!("w3c-xml");
///     // ... run tests using path
/// }
/// ```
#[macro_export]
macro_rules! require_test_data {
    ($suite:expr) => {
        match $crate::get_test_data_path($suite) {
            Some(path) => path,
            None => {
                eprintln!(
                    "Skipping: {} data not available. Set FASTXML_DOWNLOAD_TESTS=1 to download.",
                    $suite
                );
                return;
            }
        }
    };
}

/// Directory holding the committed baseline TSV files.
pub fn baselines_dir() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    manifest_dir.join("baselines")
}

/// Whether `FASTXML_UPDATE_BASELINE=1` is set (regenerate baselines instead of
/// asserting against them).
pub fn should_update_baseline() -> bool {
    std::env::var("FASTXML_UPDATE_BASELINE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Whether `FASTXML_CONFORMANCE_AUDIT=1` is set (print the classification audit
/// histogram).
pub fn should_audit() -> bool {
    std::env::var("FASTXML_CONFORMANCE_AUDIT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}
