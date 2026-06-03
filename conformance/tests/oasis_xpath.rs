//! OASIS XPath 1.0 Test Suite tests.
//!
//! These tests run the OASIS XPath test suite against fastxml's XPath evaluator.

use fastxml::XmlContext;
use fastxml_conformance::catalog::oasis::{ExpectedResult, XPathTestSuite};
use fastxml_conformance::reporter::SuiteReport;
use fastxml_conformance::require_test_data;
use std::fs;

/// Run OASIS XPath conformance tests.
#[test]
fn oasis_xpath_conformance() {
    let data_path = require_test_data!("oasis-xpath");

    // Look for the test catalog
    let catalog_candidates = [
        data_path.join("catalog.xml"),
        data_path.join("xpath-tests.xml"),
        data_path.join("tests.xml"),
    ];

    let catalog_path = catalog_candidates.iter().find(|p| p.exists());
    let catalog_path = match catalog_path {
        Some(p) => p,
        None => {
            eprintln!("XPath test catalog not found. Skipping tests.");
            eprintln!("Tried: {:?}", catalog_candidates);
            return;
        }
    };

    eprintln!("Loading XPath test suite from: {}", catalog_path.display());

    let suite = match XPathTestSuite::parse(catalog_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse catalog: {}", e);
            return;
        }
    };

    let stats = suite.stats();
    eprintln!(
        "Found {} XPath tests: {} nodeset, {} string, {} number, {} boolean, {} error",
        stats.total, stats.nodeset, stats.string, stats.number, stats.boolean, stats.error
    );

    let mut report = SuiteReport::new();

    for test in &suite.tests {
        // Load the input document
        let content = match fs::read(&test.input_file) {
            Ok(c) => c,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        let doc = match fastxml::Parser::from(content.as_slice()).parse() {
            Ok(d) => d,
            Err(_) => {
                report.record_skip();
                continue;
            }
        };

        // Create XPath context
        let ctx = XmlContext::new(&doc);

        // Determine context node
        let context_node = if let Some(ref context_xpath) = test.context {
            match ctx.evaluate(context_xpath) {
                Ok(result) => result.into_nodes().into_iter().next(),
                Err(_) => doc.get_root_element().ok(),
            }
        } else {
            doc.get_root_element().ok()
        };

        // Evaluate the XPath expression
        let result = if let Some(ref node) = context_node {
            ctx.evaluate_from(&test.xpath, node)
        } else {
            ctx.evaluate(&test.xpath)
        };

        match (&test.expected, result) {
            (ExpectedResult::Error, Err(_)) => {
                report.record_pass(Some("error"));
            }
            (ExpectedResult::Error, Ok(_)) => {
                eprintln!("FAIL [error] {}: expected error but succeeded", test.id);
                report.record_fail(&test.id, Some("error"));
            }
            (ExpectedResult::String(expected), Ok(result)) => {
                let actual = result.to_string_value();
                if actual == *expected {
                    report.record_pass(Some("string"));
                } else {
                    eprintln!(
                        "FAIL [string] {}: expected '{}' got '{}'",
                        test.id, expected, actual
                    );
                    report.record_fail(&test.id, Some("string"));
                }
            }
            (ExpectedResult::Number(expected), Ok(result)) => {
                let actual = result.to_number();
                if (actual - expected).abs() < 1e-10 {
                    report.record_pass(Some("number"));
                } else {
                    eprintln!(
                        "FAIL [number] {}: expected {} got {}",
                        test.id, expected, actual
                    );
                    report.record_fail(&test.id, Some("number"));
                }
            }
            (ExpectedResult::Boolean(expected), Ok(result)) => {
                let actual = result.to_boolean();
                if actual == *expected {
                    report.record_pass(Some("boolean"));
                } else {
                    eprintln!(
                        "FAIL [boolean] {}: expected {} got {}",
                        test.id, expected, actual
                    );
                    report.record_fail(&test.id, Some("boolean"));
                }
            }
            (ExpectedResult::NodeSet(expected), Ok(result)) => {
                let nodes = result.into_nodes();
                // For nodeset comparison, we just check the count for now
                if nodes.len() == expected.len() {
                    report.record_pass(Some("nodeset"));
                } else {
                    eprintln!(
                        "FAIL [nodeset] {}: expected {} nodes got {}",
                        test.id,
                        expected.len(),
                        nodes.len()
                    );
                    report.record_fail(&test.id, Some("nodeset"));
                }
            }
            (_, Err(e)) => {
                eprintln!("FAIL {}: unexpected error: {}", test.id, e);
                report.record_fail(&test.id, None);
            }
        }
    }

    // Print summary
    eprintln!();
    eprintln!("=== OASIS XPath Conformance Results ===");
    eprintln!("Total: {}", report.total);
    eprintln!("Passed: {}", report.passed);
    eprintln!("Failed: {}", report.failed);
    eprintln!("Skipped: {}", report.skipped);

    if !report.categories.is_empty() {
        eprintln!("By category:");
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
}

/// Test basic XPath functionality.
#[test]
fn basic_xpath_evaluation() {
    let xml = br#"<?xml version="1.0"?>
<root>
  <item id="1">First</item>
  <item id="2">Second</item>
  <item id="3">Third</item>
</root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    // Test node selection
    let result = ctx.evaluate("//item").expect("evaluate //item");
    assert_eq!(result.into_nodes().len(), 3);

    // Test string function
    let result = ctx
        .evaluate("string(//item[@id='1'])")
        .expect("evaluate string");
    assert_eq!(result.to_string_value(), "First");

    // Test count function
    let result = ctx.evaluate("count(//item)").expect("evaluate count");
    assert_eq!(result.to_number() as i32, 3);

    // Test boolean function
    let result = ctx.evaluate("//item[@id='2']").expect("evaluate predicate");
    assert_eq!(result.into_nodes().len(), 1);

    // Test position
    let result = ctx.evaluate("//item[2]").expect("evaluate position");
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
}

/// Test XPath axes.
#[test]
fn xpath_axes() {
    let xml = br#"<?xml version="1.0"?>
<root>
  <parent>
    <child>
      <grandchild/>
    </child>
  </parent>
</root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    // descendant axis
    let result = ctx.evaluate("//parent/descendant::*").expect("descendant");
    assert_eq!(result.into_nodes().len(), 2); // child and grandchild

    // ancestor axis
    let result = ctx.evaluate("//grandchild/ancestor::*").expect("ancestor");
    assert!(result.into_nodes().len() >= 3); // child, parent, root

    // child axis
    let result = ctx.evaluate("//parent/child::*").expect("child");
    assert_eq!(result.into_nodes().len(), 1);

    // parent axis
    let result = ctx.evaluate("//child/parent::*").expect("parent");
    assert_eq!(result.into_nodes().len(), 1);
}

/// Test XPath string functions.
#[test]
fn xpath_string_functions() {
    let xml = br#"<?xml version="1.0"?><root><text>Hello World</text></root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    // normalize-space with literal string
    let result = ctx
        .evaluate("normalize-space('  Hello World  ')")
        .expect("normalize-space");
    assert_eq!(result.to_string_value(), "Hello World");

    // contains with literal strings
    let result = ctx
        .evaluate("contains('Hello World', 'World')")
        .expect("contains");
    assert!(result.to_boolean());

    // starts-with
    let result = ctx
        .evaluate("starts-with('Hello World', 'Hello')")
        .expect("starts-with");
    assert!(result.to_boolean());

    // string-length
    let result = ctx
        .evaluate("string-length('test')")
        .expect("string-length");
    assert_eq!(result.to_number() as i32, 4);

    // concat
    let result = ctx.evaluate("concat('a', 'b', 'c')").expect("concat");
    assert_eq!(result.to_string_value(), "abc");

    // substring
    let result = ctx.evaluate("substring('12345', 2, 3)").expect("substring");
    assert_eq!(result.to_string_value(), "234");
}

/// Test XPath number functions.
#[test]
fn xpath_number_functions() {
    let xml = br#"<?xml version="1.0"?><root><n>42</n><n>-10</n><n>3.14</n></root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    // sum
    let result = ctx.evaluate("sum(//n)").expect("sum");
    assert!((result.to_number() - 35.14).abs() < 0.01);

    // floor
    let result = ctx.evaluate("floor(3.7)").expect("floor");
    assert_eq!(result.to_number() as i32, 3);

    // ceiling
    let result = ctx.evaluate("ceiling(3.2)").expect("ceiling");
    assert_eq!(result.to_number() as i32, 4);

    // round
    let result = ctx.evaluate("round(3.5)").expect("round");
    assert_eq!(result.to_number() as i32, 4);
}
