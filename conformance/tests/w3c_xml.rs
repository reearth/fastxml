//! W3C XML Conformance Test Suite tests.
//!
//! These tests run the W3C XML Conformance Test Suite against fastxml's parser.
//! Both DOM parsing and streaming parsing are tested.
//!
//! Note: fastxml has the following known limitations:
//! - Only UTF-8 encoding is supported (UTF-16, ISO-8859-1, etc. tests are skipped)
//! - DTD/external entity expansion is not supported
//! - Some edge cases in malformed XML detection may differ from strict compliance

use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml_conformance::catalog::xmlconf::{TestType, XmlConfCatalog, XmlConfTest};
use fastxml_conformance::reporter::SuiteReport;
use fastxml_conformance::{is_known_failure, require_test_data};
use std::fs;
use std::io::BufReader;

/// Check if a test requires non-UTF-8 encoding (which fastxml doesn't support).
fn requires_non_utf8(test: &XmlConfTest) -> bool {
    let path_str = test.uri.to_string_lossy().to_lowercase();
    // Skip UTF-16 and other encoding tests
    path_str.contains("utf16")
        || path_str.contains("utf-16")
        || path_str.contains("little")
        || path_str.contains("weekly-")
        || path_str.contains("pr-xml-")
}

/// Simple event counter for streaming tests.
#[allow(dead_code)]
struct EventCounter {
    elements: usize,
    texts: usize,
}

impl EventCounter {
    fn new() -> Self {
        Self {
            elements: 0,
            texts: 0,
        }
    }
}

impl XmlEventHandler for EventCounter {
    fn handle(&mut self, event: &XmlEvent) -> fastxml::error::Result<()> {
        match event {
            XmlEvent::StartElement { .. } => self.elements += 1,
            XmlEvent::Text(_) => self.texts += 1,
            _ => {}
        }
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

/// Run all W3C XML conformance tests with DOM parser.
#[test]
fn w3c_xml_conformance_dom() {
    let data_path = require_test_data!("w3c-xml");

    // The W3C test suite extracts to xmlconf/ directory
    let xmlconf_path = data_path.join("xmlconf");
    let catalog_path = if xmlconf_path.exists() {
        xmlconf_path.join("xmlconf.xml")
    } else {
        data_path.join("xmlconf.xml")
    };

    if !catalog_path.exists() {
        eprintln!(
            "Catalog not found at {}. Skipping tests.",
            catalog_path.display()
        );
        return;
    }

    eprintln!("Loading catalog from: {}", catalog_path.display());

    let catalog = match XmlConfCatalog::parse(&catalog_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse catalog: {}", e);
            return;
        }
    };

    let stats = catalog.stats();
    eprintln!(
        "Found {} tests: {} valid, {} invalid, {} not-wf, {} error",
        stats.total, stats.valid, stats.invalid, stats.not_wf, stats.error
    );

    let mut report = SuiteReport::new();

    // Run valid tests
    for test in catalog.tests_by_type(TestType::Valid) {
        let test_path = catalog.get_test_path(test);
        let category = Some("valid");

        if is_known_failure(&test.id) {
            report.record_expected_fail(category);
            continue;
        }

        // Skip tests requiring external entities
        if test.entities.as_deref() == Some("both") || test.entities.as_deref() == Some("general") {
            report.record_skip();
            continue;
        }

        // Skip non-UTF-8 encoding tests
        if requires_non_utf8(test) {
            report.record_skip();
            continue;
        }

        let content = match fs::read(&test_path) {
            Ok(c) => c,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        match fastxml::parse(&content) {
            Ok(_) => report.record_pass(category),
            Err(e) => {
                eprintln!("FAIL [valid/dom] {}: {}", test.id, e);
                report.record_fail(&test.id, category);
            }
        }
    }

    // Run not-well-formed tests
    for test in catalog.tests_by_type(TestType::NotWellFormed) {
        let test_path = catalog.get_test_path(test);
        let category = Some("not-wf");

        if is_known_failure(&test.id) {
            report.record_expected_fail(category);
            continue;
        }

        // Skip non-UTF-8 encoding tests
        if requires_non_utf8(test) {
            report.record_skip();
            continue;
        }

        let content = match fs::read(&test_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        match fastxml::parse(&content) {
            Ok(_) => {
                eprintln!("FAIL [not-wf/dom] {}: parser accepted invalid XML", test.id);
                report.record_fail(&test.id, category);
            }
            Err(_) => {
                report.record_pass(category);
            }
        }
    }

    // Run invalid tests (valid XML but invalid per DTD)
    for test in catalog.tests_by_type(TestType::Invalid) {
        let test_path = catalog.get_test_path(test);
        let category = Some("invalid");

        if is_known_failure(&test.id) {
            report.record_expected_fail(category);
            continue;
        }

        // Skip non-UTF-8 encoding tests
        if requires_non_utf8(test) {
            report.record_skip();
            continue;
        }

        let content = match fs::read(&test_path) {
            Ok(c) => c,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        match fastxml::parse(&content) {
            Ok(_) => report.record_pass(category),
            Err(e) => {
                eprintln!("FAIL [invalid/dom] {}: {}", test.id, e);
                report.record_fail(&test.id, category);
            }
        }
    }

    // Print summary
    print_report("W3C XML Conformance (DOM)", &report);

    // Note: Many failures are expected due to fastxml's limitations:
    // - No DTD entity expansion
    // - Lenient parsing of some malformed XML edge cases
    // The goal is to track progress, not enforce strict compliance.
}

/// Run all W3C XML conformance tests with Streaming parser.
#[test]
fn w3c_xml_conformance_streaming() {
    let data_path = require_test_data!("w3c-xml");

    let xmlconf_path = data_path.join("xmlconf");
    let catalog_path = if xmlconf_path.exists() {
        xmlconf_path.join("xmlconf.xml")
    } else {
        data_path.join("xmlconf.xml")
    };

    if !catalog_path.exists() {
        eprintln!("Catalog not found. Skipping streaming tests.");
        return;
    }

    let catalog = match XmlConfCatalog::parse(&catalog_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse catalog: {}", e);
            return;
        }
    };

    let mut report = SuiteReport::new();

    // Run valid tests with streaming parser
    for test in catalog.tests_by_type(TestType::Valid) {
        let test_path = catalog.get_test_path(test);
        let category = Some("valid");

        if is_known_failure(&test.id) {
            report.record_expected_fail(category);
            continue;
        }

        if test.entities.as_deref() == Some("both") || test.entities.as_deref() == Some("general") {
            report.record_skip();
            continue;
        }

        // Skip non-UTF-8 encoding tests
        if requires_non_utf8(test) {
            report.record_skip();
            continue;
        }

        let file = match fs::File::open(&test_path) {
            Ok(f) => f,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        let reader = BufReader::new(file);
        let mut parser = StreamingParser::new(reader);
        parser.add_handler(Box::new(EventCounter::new()));

        match parser.parse() {
            Ok(_) => report.record_pass(category),
            Err(e) => {
                eprintln!("FAIL [valid/streaming] {}: {}", test.id, e);
                report.record_fail(&test.id, category);
            }
        }
    }

    // Run not-well-formed tests with streaming parser
    for test in catalog.tests_by_type(TestType::NotWellFormed) {
        let test_path = catalog.get_test_path(test);
        let category = Some("not-wf");

        if is_known_failure(&test.id) {
            report.record_expected_fail(category);
            continue;
        }

        // Skip non-UTF-8 encoding tests
        if requires_non_utf8(test) {
            report.record_skip();
            continue;
        }

        let file = match fs::File::open(&test_path) {
            Ok(f) => f,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        let reader = BufReader::new(file);
        let mut parser = StreamingParser::new(reader);
        parser.add_handler(Box::new(EventCounter::new()));

        match parser.parse() {
            Ok(_) => {
                eprintln!(
                    "FAIL [not-wf/streaming] {}: parser accepted invalid XML",
                    test.id
                );
                report.record_fail(&test.id, category);
            }
            Err(_) => {
                report.record_pass(category);
            }
        }
    }

    // Print summary
    print_report("W3C XML Conformance (Streaming)", &report);

    // Note: Many failures are expected due to fastxml's limitations.
    // The goal is to track progress, not enforce strict compliance.
}

/// Test a few specific well-known XML test cases.
#[test]
fn w3c_xml_specific_cases() {
    let data_path = require_test_data!("w3c-xml");

    let xmlconf_path = data_path.join("xmlconf");
    let base_path = if xmlconf_path.exists() {
        xmlconf_path
    } else {
        data_path.clone()
    };

    // Test 1: Basic valid XML (james clark test) - DOM
    let valid_file = base_path
        .join("james clark")
        .join("valid")
        .join("sa")
        .join("001.xml");
    if valid_file.exists() {
        let content = fs::read(&valid_file).expect("read valid file");
        assert!(
            fastxml::parse(&content).is_ok(),
            "Should parse valid XML (DOM)"
        );

        // Also test streaming
        let file = fs::File::open(&valid_file).expect("open valid file");
        let reader = BufReader::new(file);
        let mut parser = StreamingParser::new(reader);
        parser.add_handler(Box::new(EventCounter::new()));
        assert!(parser.parse().is_ok(), "Should parse valid XML (Streaming)");
    }

    // Test 2: Not well-formed XML - DOM
    let not_wf_file = base_path
        .join("james clark")
        .join("not-wf")
        .join("sa")
        .join("001.xml");
    if not_wf_file.exists() {
        let content = fs::read(&not_wf_file).expect("read not-wf file");
        assert!(
            fastxml::parse(&content).is_err(),
            "Should reject not-well-formed XML (DOM)"
        );

        // Also test streaming
        let file = fs::File::open(&not_wf_file).expect("open not-wf file");
        let reader = BufReader::new(file);
        let mut parser = StreamingParser::new(reader);
        parser.add_handler(Box::new(EventCounter::new()));
        assert!(
            parser.parse().is_err(),
            "Should reject not-well-formed XML (Streaming)"
        );
    }
}

fn print_report(title: &str, report: &SuiteReport) {
    eprintln!();
    eprintln!("=== {} Results ===", title);
    eprintln!("Total: {}", report.total);
    eprintln!("Passed: {}", report.passed);
    eprintln!("Failed: {}", report.failed);
    eprintln!("Skipped: {}", report.skipped);
    eprintln!("Expected failures: {}", report.expected_failures);

    let pass_rate = if report.total > 0 {
        let effective_total = report.total - report.skipped - report.expected_failures;
        if effective_total > 0 {
            (report.passed as f64 / effective_total as f64) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    eprintln!("Pass rate: {:.1}%", pass_rate);

    for (cat, cat_report) in &report.categories {
        let cat_rate = if cat_report.total > 0 {
            (cat_report.passed as f64 / cat_report.total as f64) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {}: {}/{} ({:.1}%)",
            cat, cat_report.passed, cat_report.total, cat_rate
        );
    }
}
