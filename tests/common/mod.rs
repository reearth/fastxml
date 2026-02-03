//! Common test utilities and libxml comparison helpers.

#[cfg(feature = "compare-libxml")]
#[allow(dead_code)]
pub mod libxml_compare {
    use std::collections::HashMap;

    /// Comparison result between fastxml and libxml
    #[derive(Debug)]
    pub struct CompareResult {
        pub matches: bool,
        pub differences: Vec<String>,
    }

    impl CompareResult {
        pub fn ok() -> Self {
            Self {
                matches: true,
                differences: vec![],
            }
        }

        pub fn diff(msg: impl Into<String>) -> Self {
            Self {
                matches: false,
                differences: vec![msg.into()],
            }
        }

        pub fn assert_match(&self) {
            if !self.matches {
                panic!(
                    "fastxml and libxml results differ:\n{}",
                    self.differences.join("\n")
                );
            }
        }
    }

    /// Parse XML with libxml and return comparable data
    pub fn libxml_parse(xml: &str) -> Result<LibxmlDoc, String> {
        let parser = libxml::parser::Parser::default();
        let doc = parser
            .parse_string(xml)
            .map_err(|e| format!("libxml parse error: {:?}", e))?;
        Ok(LibxmlDoc { doc })
    }

    pub struct LibxmlDoc {
        doc: libxml::tree::Document,
    }

    /// XPath result type for libxml comparison
    #[derive(Debug)]
    pub enum XPathResult {
        NodeSet(Vec<libxml::tree::Node>),
        String(String),
        Number(f64),
        Boolean(bool),
    }

    impl LibxmlDoc {
        pub fn root_name(&self) -> Option<String> {
            self.doc.get_root_element().map(|n| n.get_name())
        }

        /// Evaluate XPath and return typed result
        pub fn xpath_eval(&self, xpath: &str) -> Result<XPathResult, String> {
            let ctx = libxml::xpath::Context::new(&self.doc)
                .map_err(|_| "Failed to create xpath context")?;

            // Register namespaces from root
            if let Some(root) = self.doc.get_root_element() {
                for ns in root.get_namespace_declarations() {
                    let prefix = ns.get_prefix();
                    if !prefix.is_empty() {
                        let href = ns.get_href();
                        let _ = ctx.register_namespace(&prefix, &href);
                    }
                }
            }

            let result = ctx
                .evaluate(xpath)
                .map_err(|_| format!("XPath evaluation failed: {}", xpath))?;

            // libxml-rs provides get_nodes_as_vec() which returns nodes for node-set results
            // and empty vec for scalar results (number, string, boolean)
            let nodes = result.get_nodes_as_vec();

            // For scalar results, libxml returns empty nodes but we can get values through
            // number_of_nodes (returns 0 for scalar) and the actual scalar value methods
            // Unfortunately libxml-rs doesn't expose scalar value getters directly
            // So we just return empty NodeSet for scalar results and rely on the fallback
            // comparison logic
            Ok(XPathResult::NodeSet(nodes))
        }

        pub fn root_attributes(&self) -> HashMap<String, String> {
            self.doc
                .get_root_element()
                .map(|n| n.get_attributes().into_iter().collect())
                .unwrap_or_default()
        }

        pub fn root_namespace_prefix(&self) -> Option<String> {
            self.doc
                .get_root_element()
                .and_then(|n| n.get_namespace().map(|ns| ns.get_prefix()))
        }

        pub fn child_element_names(&self) -> Vec<String> {
            self.doc
                .get_root_element()
                .map(|n| {
                    n.get_child_elements()
                        .into_iter()
                        .map(|c| c.get_name())
                        .collect()
                })
                .unwrap_or_default()
        }

        pub fn xpath_string_results(&self, xpath: &str) -> Result<Vec<String>, String> {
            self.xpath_string_results_with_variables(xpath, &HashMap::new())
        }

        pub fn xpath_string_results_with_variables(
            &self,
            xpath: &str,
            variables: &HashMap<String, XPathVarValue>,
        ) -> Result<Vec<String>, String> {
            let root = self.doc.get_root_element().ok_or("No root element")?;
            let ctx = libxml::xpath::Context::new(&self.doc)
                .map_err(|_| "Failed to create xpath context")?;

            // Register namespaces from root
            for ns in root.get_namespace_declarations() {
                let prefix = ns.get_prefix();
                if !prefix.is_empty() {
                    let href = ns.get_href();
                    let _ = ctx.register_namespace(&prefix, &href);
                }
            }

            // Substitute variables in XPath expression
            // libxml-rs doesn't expose variable registration, so we substitute directly
            let mut substituted_xpath = xpath.to_string();
            for (name, value) in variables {
                let var_pattern = format!("${}", name);
                let replacement = match value {
                    XPathVarValue::String(s) => format!("'{}'", s.replace('\'', "''")),
                    XPathVarValue::Number(n) => n.to_string(),
                    XPathVarValue::Boolean(b) => {
                        if *b {
                            "true()".to_string()
                        } else {
                            "false()".to_string()
                        }
                    }
                };
                substituted_xpath = substituted_xpath.replace(&var_pattern, &replacement);
            }

            let result = ctx
                .evaluate(&substituted_xpath)
                .map_err(|_| format!("XPath evaluation failed: {}", substituted_xpath))?;

            Ok(result
                .get_nodes_as_vec()
                .into_iter()
                .map(|n| n.get_content())
                .collect())
        }
    }

    /// Variable value for comparison (simplified version of XPathValue)
    #[derive(Debug, Clone)]
    pub enum XPathVarValue {
        String(String),
        Number(f64),
        Boolean(bool),
    }

    /// Compare fastxml parse result with libxml
    pub fn compare_parse(xml: &str, fastxml_doc: &fastxml::XmlDocument) -> CompareResult {
        let libxml_doc = match libxml_parse(xml) {
            Ok(d) => d,
            Err(e) => return CompareResult::diff(format!("libxml failed to parse: {}", e)),
        };

        let mut differences = vec![];

        // Compare root element name
        let fastxml_root = fastxml::get_root_node(fastxml_doc).ok();
        let fastxml_root_name = fastxml_root.as_ref().map(|n| n.get_name());
        let libxml_root_name = libxml_doc.root_name();

        if fastxml_root_name != libxml_root_name {
            differences.push(format!(
                "Root name: fastxml={:?}, libxml={:?}",
                fastxml_root_name, libxml_root_name
            ));
        }

        // Compare root namespace prefix
        // Note: libxml returns Some("") for default namespace, fastxml returns None
        // Treat them as equivalent
        let fastxml_prefix = fastxml_root.as_ref().and_then(|n| n.get_prefix());
        let libxml_prefix = libxml_doc.root_namespace_prefix();
        let normalize_prefix = |p: Option<String>| match p {
            Some(s) if s.is_empty() => None,
            other => other,
        };
        if normalize_prefix(fastxml_prefix.clone()) != normalize_prefix(libxml_prefix.clone()) {
            differences.push(format!(
                "Root prefix: fastxml={:?}, libxml={:?}",
                fastxml_prefix, libxml_prefix
            ));
        }

        // Compare child element names
        let fastxml_children: Vec<String> = fastxml_root
            .as_ref()
            .map(|n| {
                n.get_child_elements()
                    .into_iter()
                    .map(|c| c.get_name())
                    .collect()
            })
            .unwrap_or_default();
        let libxml_children = libxml_doc.child_element_names();

        if fastxml_children != libxml_children {
            differences.push(format!(
                "Child elements: fastxml={:?}, libxml={:?}",
                fastxml_children, libxml_children
            ));
        }

        if differences.is_empty() {
            CompareResult::ok()
        } else {
            CompareResult {
                matches: false,
                differences,
            }
        }
    }

    /// Compare XPath results between fastxml and libxml
    ///
    /// Note: Some XPath features are not properly supported by libxml-rs and will be skipped:
    /// - namespace axis (namespace::*) - libxml-rs returns empty or garbage data for these queries
    pub fn compare_xpath(
        xml: &str,
        xpath: &str,
        fastxml_doc: &fastxml::XmlDocument,
    ) -> CompareResult {
        // Skip comparison for namespace axis queries
        // libxml-rs doesn't properly support namespace axis - it returns empty results
        // or garbage data for namespace::* queries, which is a known limitation of the
        // Rust bindings (not libxml2 itself)
        if xpath.contains("namespace::") {
            return CompareResult::ok();
        }

        let libxml_doc = match libxml_parse(xml) {
            Ok(d) => d,
            Err(e) => return CompareResult::diff(format!("libxml failed to parse: {}", e)),
        };

        // Get fastxml results
        let fastxml_result = match fastxml::xpath::evaluate(fastxml_doc, xpath) {
            Ok(r) => r,
            Err(e) => {
                // Check if libxml also fails
                if libxml_doc.xpath_eval(xpath).is_err() {
                    return CompareResult::ok(); // Both failed, that's consistent
                }
                return CompareResult::diff(format!(
                    "fastxml XPath failed but libxml succeeded: {}",
                    e
                ));
            }
        };

        // Get libxml results
        let libxml_result = match libxml_doc.xpath_eval(xpath) {
            Ok(r) => r,
            Err(e) => {
                return CompareResult::diff(format!(
                    "libxml XPath failed but fastxml succeeded: {}",
                    e
                ));
            }
        };

        // Compare results
        // Since libxml-rs doesn't expose scalar values, we can only compare node-sets reliably
        // For scalar results, libxml returns empty node-set, so we handle that case specially
        use fastxml::xpath::XPathResult as FastXmlResult;

        match (&fastxml_result, &libxml_result) {
            // Both are node-sets: compare nodes
            (FastXmlResult::Nodes(fastxml_nodes), XPathResult::NodeSet(libxml_nodes)) => {
                if fastxml_nodes.len() != libxml_nodes.len() {
                    return CompareResult::diff(format!(
                        "XPath '{}' node count differs: fastxml={}, libxml={}",
                        xpath,
                        fastxml_nodes.len(),
                        libxml_nodes.len()
                    ));
                }

                // Compare node contents
                let fastxml_texts: Vec<String> = fastxml_nodes
                    .iter()
                    .filter_map(|n| n.get_content())
                    .filter(|s| !s.is_empty())
                    .collect();
                let libxml_texts: Vec<String> = libxml_nodes
                    .iter()
                    .map(|n| n.get_content())
                    .filter(|s| !s.is_empty())
                    .collect();

                if fastxml_texts != libxml_texts {
                    CompareResult::diff(format!(
                        "XPath '{}' text values differ:\n  fastxml: {:?}\n  libxml:  {:?}",
                        xpath, fastxml_texts, libxml_texts
                    ))
                } else {
                    CompareResult::ok()
                }
            }
            // fastxml returned scalar, libxml returned empty node-set (expected for scalar XPath)
            (FastXmlResult::String(_), XPathResult::NodeSet(nodes)) if nodes.is_empty() => {
                // libxml-rs can't return scalar values, so we accept this
                CompareResult::ok()
            }
            (FastXmlResult::Number(_), XPathResult::NodeSet(nodes)) if nodes.is_empty() => {
                CompareResult::ok()
            }
            (FastXmlResult::Boolean(_), XPathResult::NodeSet(nodes)) if nodes.is_empty() => {
                CompareResult::ok()
            }
            _ => {
                // Unexpected case - report it
                CompareResult::diff(format!(
                    "XPath '{}' result type mismatch:\n  fastxml: {:?}\n  libxml: {:?}",
                    xpath, fastxml_result, libxml_result
                ))
            }
        }
    }

    /// Compare XPath results with variables between fastxml and libxml
    pub fn compare_xpath_with_variables(
        xml: &str,
        xpath: &str,
        fastxml_doc: &fastxml::XmlDocument,
        fastxml_vars: std::collections::HashMap<String, fastxml::xpath::XPathValue>,
        libxml_vars: HashMap<String, XPathVarValue>,
    ) -> CompareResult {
        let libxml_doc = match libxml_parse(xml) {
            Ok(d) => d,
            Err(e) => return CompareResult::diff(format!("libxml failed to parse: {}", e)),
        };

        // Get fastxml results with variables
        let evaluator = fastxml::xpath::XPathEvaluator::new(fastxml_doc);
        let (fastxml_count, fastxml_texts) =
            match evaluator.evaluate_with_variables(xpath, fastxml_vars) {
                Ok(r) => {
                    let nodes = r.clone().into_nodes();
                    let count = nodes.len();
                    let texts = fastxml::xpath::collect_text_values(&r);
                    (count, texts)
                }
                Err(e) => {
                    // Check if libxml also fails
                    if libxml_doc
                        .xpath_string_results_with_variables(xpath, &libxml_vars)
                        .is_err()
                    {
                        return CompareResult::ok(); // Both failed, that's consistent
                    }
                    return CompareResult::diff(format!(
                        "fastxml XPath with variables failed but libxml succeeded: {}",
                        e
                    ));
                }
            };

        // Get libxml results with variables
        let libxml_result =
            match libxml_doc.xpath_string_results_with_variables(xpath, &libxml_vars) {
                Ok(r) => r,
                Err(e) => {
                    return CompareResult::diff(format!(
                        "libxml XPath with variables failed but fastxml succeeded: {}",
                        e
                    ));
                }
            };

        // Filter out empty strings from libxml results for fair comparison
        let libxml_texts: Vec<String> = libxml_result
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        let libxml_count = libxml_result.len();

        // First compare node counts
        if fastxml_count != libxml_count {
            return CompareResult::diff(format!(
                "XPath '{}' with variables node count differs: fastxml={}, libxml={}",
                xpath, fastxml_count, libxml_count
            ));
        }

        // Compare non-empty text values
        if fastxml_texts != libxml_texts {
            CompareResult::diff(format!(
                "XPath '{}' with variables text values differ:\n  fastxml: {:?}\n  libxml:  {:?}",
                xpath, fastxml_texts, libxml_texts
            ))
        } else {
            CompareResult::ok()
        }
    }

    /// Assert that both parsers either succeed or fail on the given XML
    pub fn assert_parse_consistency(xml: &str) {
        let fastxml_result = fastxml::parse(xml);
        let libxml_result = libxml_parse(xml);

        match (&fastxml_result, &libxml_result) {
            (Ok(_), Ok(_)) => {}   // Both succeeded
            (Err(_), Err(_)) => {} // Both failed
            (Ok(_), Err(e)) => {
                panic!("fastxml succeeded but libxml failed: {}", e);
            }
            (Err(e), Ok(_)) => {
                panic!("fastxml failed but libxml succeeded: {}", e);
            }
        }
    }

    /// Validate XML with libxml against XSD schema
    pub fn validate_with_libxml(xml: &str, xsd: &str) -> (bool, Vec<String>) {
        use libxml::parser::Parser;
        use libxml::schemas::{SchemaParserContext, SchemaValidationContext};

        let parser = Parser::default();
        let doc = parser
            .parse_string(xml)
            .expect("libxml: Failed to parse XML");

        let mut schema_parser = SchemaParserContext::from_buffer(xsd.as_bytes());
        let mut ctx = SchemaValidationContext::from_parser(&mut schema_parser)
            .expect("libxml: Failed to create validation context");

        let result = ctx.validate_document(&doc);
        let is_valid = result.is_ok();

        let messages: Vec<String> = if let Err(errors) = result {
            errors.iter().filter_map(|e| e.message.clone()).collect()
        } else {
            vec![]
        };

        (is_valid, messages)
    }

    /// Compare XSD validation results between fastxml and libxml
    pub fn compare_xsd_validation(xml: &str, xsd: &str) -> CompareResult {
        use fastxml::schema::validator::XmlSchemaValidationContext;
        use fastxml::schema::xsd::parse_xsd;

        // fastxml validation
        let fastxml_valid = match fastxml::parse(xml.as_bytes()) {
            Ok(doc) => match parse_xsd(xsd.as_bytes()) {
                Ok(schema) => {
                    let ctx = XmlSchemaValidationContext::new(schema);
                    ctx.validate(&doc)
                        .map(|errors| errors.iter().all(|e| !e.is_error()))
                        .unwrap_or(false)
                }
                Err(_) => return CompareResult::diff("fastxml: Failed to parse XSD"),
            },
            Err(_) => return CompareResult::diff("fastxml: Failed to parse XML"),
        };

        // libxml validation
        let (libxml_valid, _) = validate_with_libxml(xml, xsd);

        if fastxml_valid == libxml_valid {
            CompareResult::ok()
        } else {
            CompareResult::diff(format!(
                "Validation result differs: fastxml={}, libxml={}",
                fastxml_valid, libxml_valid
            ))
        }
    }
}

/// Macro to compare fastxml result with libxml when the feature is enabled
#[macro_export]
#[cfg(feature = "compare-libxml")]
macro_rules! compare_with_libxml {
    (parse: $xml:expr, $doc:expr) => {
        $crate::common::libxml_compare::compare_parse($xml, $doc).assert_match();
    };
    (xpath: $xml:expr, $xpath:expr, $doc:expr) => {
        $crate::common::libxml_compare::compare_xpath($xml, $xpath, $doc).assert_match();
    };
    (xpath_vars: $xml:expr, $xpath:expr, $doc:expr, $fastxml_vars:expr, $libxml_vars:expr) => {
        $crate::common::libxml_compare::compare_xpath_with_variables(
            $xml,
            $xpath,
            $doc,
            $fastxml_vars,
            $libxml_vars,
        )
        .assert_match();
    };
    (consistency: $xml:expr) => {
        $crate::common::libxml_compare::assert_parse_consistency($xml);
    };
    (validate: $xml:expr, $xsd:expr) => {
        $crate::common::libxml_compare::compare_xsd_validation($xml, $xsd).assert_match();
    };
}

#[macro_export]
#[cfg(not(feature = "compare-libxml"))]
macro_rules! compare_with_libxml {
    (parse: $xml:expr, $doc:expr) => {};
    (xpath: $xml:expr, $xpath:expr, $doc:expr) => {};
    (xpath_vars: $xml:expr, $xpath:expr, $doc:expr, $fastxml_vars:expr, $libxml_vars:expr) => {};
    (consistency: $xml:expr) => {};
    (validate: $xml:expr, $xsd:expr) => {};
}

// =============================================================================
// Unified Validation Helpers
// =============================================================================

use std::sync::Arc;

use fastxml::StructuredError;
#[allow(deprecated)]
use fastxml::schema::validator::{
    DomSchemaValidator, OnePassSchemaValidator, TwoPassSchemaValidator,
};
use fastxml::schema::xsd::parse_xsd;

// Note: The following types and functions use `#[allow(dead_code)]` because
// they are called through the `test_validation!` macro. The compiler doesn't
// detect macro-based usage during dead code analysis.

/// Result of schema validation containing validity and errors.
#[allow(dead_code)]
#[derive(Debug)]
pub struct ValidationResult {
    /// Whether the document is valid
    pub valid: bool,
    /// Validation errors (may contain warnings too)
    pub errors: Vec<StructuredError>,
}

#[allow(dead_code)]
impl ValidationResult {
    /// Returns true if the document is valid (no errors).
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Returns the list of validation errors.
    pub fn errors(&self) -> &[StructuredError] {
        &self.errors
    }

    /// Asserts that all errors have line position information.
    /// Call this when validation is expected to fail to verify error positions are reported.
    pub fn assert_errors_have_line(&self) {
        for (i, error) in self.errors.iter().filter(|e| e.is_error()).enumerate() {
            assert!(
                error.line().is_some(),
                "Error {} should have line number: {:?}",
                i,
                error
            );
        }
    }

    /// Asserts that all errors have both line and column position information.
    pub fn assert_errors_have_line_column(&self) {
        for (i, error) in self.errors.iter().filter(|e| e.is_error()).enumerate() {
            assert!(
                error.line().is_some(),
                "Error {} should have line number: {:?}",
                i,
                error
            );
            assert!(
                error.column().is_some(),
                "Error {} should have column number: {:?}",
                i,
                error
            );
        }
    }

    /// Asserts that the first error has the expected line number.
    pub fn assert_first_error_line(&self, expected_line: usize) {
        let error = self
            .errors
            .iter()
            .find(|e| e.is_error())
            .expect("Expected at least one error");
        assert_eq!(
            error.line(),
            Some(expected_line),
            "First error line mismatch. Error: {:?}",
            error
        );
    }

    /// Asserts that the first error has the expected line and column.
    pub fn assert_first_error_position(&self, expected_line: usize, expected_column: usize) {
        let error = self
            .errors
            .iter()
            .find(|e| e.is_error())
            .expect("Expected at least one error");
        assert_eq!(
            error.line(),
            Some(expected_line),
            "First error line mismatch. Error: {:?}",
            error
        );
        assert_eq!(
            error.column(),
            Some(expected_column),
            "First error column mismatch. Error: {:?}",
            error
        );
    }
}

/// Validate XML against XSD using DOM validator.
#[allow(dead_code)]
pub fn validate_dom(xml: &str, xsd: &str) -> ValidationResult {
    let doc = fastxml::parse(xml.as_bytes()).expect("Failed to parse XML");
    let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).expect("Validation failed");
    let valid = errors.iter().all(|e| !e.is_error());
    ValidationResult { valid, errors }
}

/// Validate XML against XSD using TwoPass validator.
#[allow(dead_code, deprecated)]
pub fn validate_twopass(xml: &str, xsd: &str) -> ValidationResult {
    use std::io::Cursor;
    let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
    let reader = Cursor::new(xml.as_bytes().to_vec());
    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .expect("Validation failed");
    let valid = errors.iter().all(|e| !e.is_error());
    ValidationResult { valid, errors }
}

/// Validate XML against XSD using OnePass (streaming) validator.
#[allow(dead_code)]
pub fn validate_onepass(xml: &str, xsd: &str) -> ValidationResult {
    use std::io::BufReader;
    let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
    let reader = BufReader::new(xml.as_bytes());
    let errors = OnePassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .expect("Validation failed");
    let valid = errors.iter().all(|e| !e.is_error());
    ValidationResult { valid, errors }
}

/// Validate with all validators and check consistency.
/// Returns (dom_result, twopass_result, onepass_result).
#[allow(dead_code)]
pub fn validate_all(
    xml: &str,
    xsd: &str,
) -> (ValidationResult, ValidationResult, ValidationResult) {
    let dom = validate_dom(xml, xsd);
    let twopass = validate_twopass(xml, xsd);
    let onepass = validate_onepass(xml, xsd);
    (dom, twopass, onepass)
}

/// Assert that all validators produce consistent results.
#[allow(dead_code)]
pub fn assert_validators_consistent(xml: &str, xsd: &str) {
    let (dom, twopass, onepass) = validate_all(xml, xsd);

    assert_eq!(
        dom.is_valid(),
        twopass.is_valid(),
        "DOM vs TwoPass mismatch for validity.\nDOM errors: {:?}\nTwoPass errors: {:?}",
        dom.errors(),
        twopass.errors()
    );

    assert_eq!(
        twopass.is_valid(),
        onepass.is_valid(),
        "TwoPass vs OnePass mismatch for validity.\nTwoPass errors: {:?}\nOnePass errors: {:?}",
        twopass.errors(),
        onepass.errors()
    );
}

/// Macro to test validation with all validators and optionally compare with libxml.
///
/// Usage:
/// ```ignore
/// // Basic usage (checks errors have line info when invalid)
/// test_validation!(test_name, xml, xsd, should_be_valid);
///
/// // With expected error position (line, column)
/// test_validation!(test_name, xml, xsd, false, line: 5, column: 3);
/// ```
#[macro_export]
macro_rules! test_validation {
    // Variant with expected error position (line and column)
    ($name:ident, $xml:expr, $xsd:expr, false, line: $line:expr, column: $col:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn dom() {
                let result = $crate::common::validate_dom($xml, $xsd);
                assert!(
                    !result.is_valid(),
                    "DOM validation: expected invalid, got valid"
                );
                result.assert_first_error_position($line, $col);
            }

            #[test]
            fn twopass() {
                let result = $crate::common::validate_twopass($xml, $xsd);
                assert!(
                    !result.is_valid(),
                    "TwoPass validation: expected invalid, got valid"
                );
                result.assert_first_error_position($line, $col);
            }

            #[test]
            fn onepass() {
                let result = $crate::common::validate_onepass($xml, $xsd);
                assert!(
                    !result.is_valid(),
                    "OnePass validation: expected invalid, got valid"
                );
                result.assert_first_error_position($line, $col);
            }

            #[test]
            fn consistency() {
                $crate::common::assert_validators_consistent($xml, $xsd);
            }

            #[test]
            fn libxml_comparison() {
                $crate::compare_with_libxml!(validate: $xml, $xsd);
            }
        }
    };
    // Basic variant (existing behavior)
    ($name:ident, $xml:expr, $xsd:expr, $expected_valid:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn dom() {
                let result = $crate::common::validate_dom($xml, $xsd);
                assert_eq!(
                    result.is_valid(),
                    $expected_valid,
                    "DOM validation: expected valid={}, got valid={}\nErrors: {:?}",
                    $expected_valid,
                    result.is_valid(),
                    result.errors()
                );
                // When validation fails, verify errors have position info
                if !$expected_valid {
                    result.assert_errors_have_line();
                }
            }

            #[test]
            fn twopass() {
                let result = $crate::common::validate_twopass($xml, $xsd);
                assert_eq!(
                    result.is_valid(),
                    $expected_valid,
                    "TwoPass validation: expected valid={}, got valid={}\nErrors: {:?}",
                    $expected_valid,
                    result.is_valid(),
                    result.errors()
                );
                // When validation fails, verify errors have position info
                if !$expected_valid {
                    result.assert_errors_have_line();
                }
            }

            #[test]
            fn onepass() {
                let result = $crate::common::validate_onepass($xml, $xsd);
                assert_eq!(
                    result.is_valid(),
                    $expected_valid,
                    "OnePass validation: expected valid={}, got valid={}\nErrors: {:?}",
                    $expected_valid,
                    result.is_valid(),
                    result.errors()
                );
                // When validation fails, verify errors have position info
                if !$expected_valid {
                    result.assert_errors_have_line();
                }
            }

            #[test]
            fn consistency() {
                $crate::common::assert_validators_consistent($xml, $xsd);
            }

            #[test]
            fn libxml_comparison() {
                $crate::compare_with_libxml!(validate: $xml, $xsd);
            }
        }
    };
}
