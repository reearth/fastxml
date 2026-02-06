//! # fastxml-conformance
//!
//! Conformance test suite for fastxml - W3C/OASIS standard compliance testing.
//!
//! This crate provides test harnesses for running standard XML test suites:
//! - W3C XML Conformance Test Suite
//! - W3C XML Schema Test Suite
//! - OASIS XPath 1.0 Test Suite

pub mod catalog;
pub mod downloader;
pub mod reporter;

use std::collections::HashSet;
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

/// Test result enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    /// Test passed as expected.
    Pass,
    /// Test failed unexpectedly.
    Fail,
    /// Test was skipped (e.g., missing data or known limitation).
    Skip,
    /// Test failed as expected (known failure).
    ExpectedFail,
}

// Known failures due to fastxml limitations.
// These tests are expected to fail because fastxml does not support certain features.
lazy_static::lazy_static! {
    /// Tests that fail due to DTD not being fully supported.
    pub static ref KNOWN_DTD_FAILURES: HashSet<&'static str> = {
        let mut set = HashSet::new();
        // DTD entity expansion tests
        set.insert("ibm-valid-P02-ibm02v01.xml");
        set.insert("ibm-valid-P66-ibm66v01.xml");
        // External entity tests
        set.insert("sun/valid/ext01.xml");
        set.insert("sun/valid/ext02.xml");
        set
    };

    /// Tests that fail due to external entity not being supported.
    pub static ref KNOWN_EXTERNAL_ENTITY_FAILURES: HashSet<&'static str> = {
        let mut set = HashSet::new();
        set.insert("oasis/p01pass1.xml");
        set
    };
}

/// Check if a test is a known failure.
pub fn is_known_failure(test_id: &str) -> bool {
    KNOWN_DTD_FAILURES.contains(test_id) || KNOWN_EXTERNAL_ENTITY_FAILURES.contains(test_id)
}
