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

    impl LibxmlDoc {
        pub fn root_name(&self) -> Option<String> {
            self.doc.get_root_element().map(|n| n.get_name())
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

            let result = ctx
                .evaluate(xpath)
                .map_err(|_| format!("XPath evaluation failed: {}", xpath))?;

            Ok(result
                .get_nodes_as_vec()
                .into_iter()
                .map(|n| n.get_content())
                .collect())
        }
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
    pub fn compare_xpath(
        xml: &str,
        xpath: &str,
        fastxml_doc: &fastxml::XmlDocument,
    ) -> CompareResult {
        let libxml_doc = match libxml_parse(xml) {
            Ok(d) => d,
            Err(e) => return CompareResult::diff(format!("libxml failed to parse: {}", e)),
        };

        // Get fastxml results - both node count and text values
        let (fastxml_count, fastxml_texts) = match fastxml::xpath::evaluate(fastxml_doc, xpath) {
            Ok(r) => {
                let nodes = r.clone().into_nodes();
                let count = nodes.len();
                let texts = fastxml::xpath::collect_text_values(&r);
                (count, texts)
            }
            Err(e) => {
                // Check if libxml also fails
                if libxml_doc.xpath_string_results(xpath).is_err() {
                    return CompareResult::ok(); // Both failed, that's consistent
                }
                return CompareResult::diff(format!(
                    "fastxml XPath failed but libxml succeeded: {}",
                    e
                ));
            }
        };

        // Get libxml results
        let libxml_result = match libxml_doc.xpath_string_results(xpath) {
            Ok(r) => r,
            Err(e) => {
                return CompareResult::diff(format!(
                    "libxml XPath failed but fastxml succeeded: {}",
                    e
                ));
            }
        };

        // Filter out empty strings from libxml results for fair comparison
        // libxml returns "" for element nodes with no text content
        let libxml_texts: Vec<String> = libxml_result
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        let libxml_count = libxml_result.len();

        // First compare node counts (most important for element selections)
        if fastxml_count != libxml_count {
            return CompareResult::diff(format!(
                "XPath '{}' node count differs: fastxml={}, libxml={}",
                xpath, fastxml_count, libxml_count
            ));
        }

        // Compare non-empty text values (should be in document order)
        if fastxml_texts != libxml_texts {
            CompareResult::diff(format!(
                "XPath '{}' text values differ:\n  fastxml: {:?}\n  libxml:  {:?}",
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
    (consistency: $xml:expr) => {
        $crate::common::libxml_compare::assert_parse_consistency($xml);
    };
}

#[macro_export]
#[cfg(not(feature = "compare-libxml"))]
macro_rules! compare_with_libxml {
    (parse: $xml:expr, $doc:expr) => {};
    (xpath: $xml:expr, $xpath:expr, $doc:expr) => {};
    (consistency: $xml:expr) => {};
}
