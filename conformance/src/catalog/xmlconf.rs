//! W3C XML Conformance Test Suite catalog parser.
//!
//! Parses the `xmlconf.xml` file that describes the W3C XML Conformance Test Suite.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A test case from the W3C XML Conformance Test Suite.
#[derive(Debug, Clone)]
pub struct XmlConfTest {
    /// Unique test ID.
    pub id: String,
    /// Test type: "valid", "invalid", "not-wf", "error".
    pub test_type: TestType,
    /// Path to the test XML file (relative to catalog).
    pub uri: PathBuf,
    /// Optional path to output file for comparison.
    pub output: Option<PathBuf>,
    /// Optional description of the test.
    pub description: Option<String>,
    /// Test section(s) covered.
    pub sections: Option<String>,
    /// XML version(s) this test applies to (e.g. "1.0", "1.1"). A
    /// whitespace-separated list; absent means unspecified (treated as 1.0).
    pub version: Option<String>,
    /// Edition of XML spec this test applies to.
    pub edition: Option<String>,
    /// Namespace support required: "yes", "no", or "both".
    pub namespace: Option<String>,
    /// Whether the test requires external entity support.
    pub entities: Option<String>,
}

/// Type of conformance test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestType {
    /// Document should parse successfully and be valid.
    Valid,
    /// Document should parse but is invalid per DTD.
    Invalid,
    /// Document is not well-formed (should fail to parse).
    NotWellFormed,
    /// Document causes a recoverable error.
    Error,
}

impl TestType {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "valid" => Some(Self::Valid),
            "invalid" => Some(Self::Invalid),
            "not-wf" => Some(Self::NotWellFormed),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Check if the test expects parsing to succeed.
    pub fn expects_parse_success(&self) -> bool {
        matches!(self, Self::Valid | Self::Invalid)
    }
}

/// A test suite (collection of related tests).
#[derive(Debug, Clone)]
pub struct TestSuite {
    /// Profile name (e.g., "IBM XML Tests", "OASIS").
    pub name: String,
    /// Tests in this suite.
    pub tests: Vec<XmlConfTest>,
}

/// Parsed W3C XML Conformance catalog.
#[derive(Debug, Clone)]
pub struct XmlConfCatalog {
    /// All test suites.
    pub suites: Vec<TestSuite>,
    /// Base path to the catalog directory.
    pub base_path: PathBuf,
}

impl XmlConfCatalog {
    /// Parse the xmlconf.xml catalog file.
    pub fn parse(catalog_path: &Path) -> Result<Self, XmlConfError> {
        let base_path = catalog_path
            .parent()
            .ok_or(XmlConfError::InvalidPath)?
            .to_path_buf();

        let content = fs::read_to_string(catalog_path).map_err(XmlConfError::Io)?;

        // Extract entity definitions from DOCTYPE to find included files
        let entity_files = extract_entity_files(&content);

        let mut suites = Vec::new();

        // Parse the main catalog file
        parse_catalog_content(&content, &base_path, &mut suites)?;

        // If no tests found (due to entity expansion), parse individual test files
        if suites.iter().all(|s| s.tests.is_empty()) {
            // Parse each included test file
            for (entity_name, file_path) in &entity_files {
                let full_path = base_path.join(file_path);
                if full_path.exists()
                    && let Ok(file_content) = fs::read_to_string(&full_path)
                {
                    let file_base = full_path.parent().unwrap_or(&base_path).to_path_buf();
                    let mut file_suites = Vec::new();
                    if parse_catalog_content(&file_content, &file_base, &mut file_suites).is_ok() {
                        for mut suite in file_suites {
                            if suite.name == "Unknown" {
                                suite.name = entity_name.clone();
                            }
                            suites.push(suite);
                        }
                    }
                }
            }
        }

        // Also try to find and parse test files in subdirectories
        if suites.iter().map(|s| s.tests.len()).sum::<usize>() == 0 {
            let known_test_files = [
                ("xmltest", "xmltest/xmltest.xml"),
                ("sun-valid", "sun/sun-valid.xml"),
                ("sun-invalid", "sun/sun-invalid.xml"),
                ("sun-not-wf", "sun/sun-not-wf.xml"),
                ("sun-error", "sun/sun-error.xml"),
                ("oasis", "oasis/oasis.xml"),
                ("ibm-invalid", "ibm/ibm_oasis_invalid.xml"),
                ("ibm-not-wf", "ibm/ibm_oasis_not-wf.xml"),
                ("ibm-valid", "ibm/ibm_oasis_valid.xml"),
                ("japanese", "japanese/japanese.xml"),
            ];

            for (name, rel_path) in &known_test_files {
                let full_path = base_path.join(rel_path);
                if full_path.exists()
                    && let Ok(file_content) = fs::read_to_string(&full_path)
                {
                    let file_base = full_path.parent().unwrap_or(&base_path).to_path_buf();
                    let mut file_suites = Vec::new();
                    if parse_catalog_content(&file_content, &file_base, &mut file_suites).is_ok() {
                        for mut suite in file_suites {
                            if suite.name == "Unknown" {
                                suite.name = name.to_string();
                            }
                            if !suite.tests.is_empty() {
                                suites.push(suite);
                            }
                        }
                    }
                }
            }
        }

        Ok(Self { suites, base_path })
    }

    /// Get all tests across all suites.
    pub fn all_tests(&self) -> impl Iterator<Item = &XmlConfTest> {
        self.suites.iter().flat_map(|s| s.tests.iter())
    }

    /// Get tests by type.
    pub fn tests_by_type(&self, test_type: TestType) -> impl Iterator<Item = &XmlConfTest> {
        self.all_tests().filter(move |t| t.test_type == test_type)
    }

    /// Get the full path to a test file.
    pub fn get_test_path(&self, test: &XmlConfTest) -> PathBuf {
        test.uri.clone()
    }

    /// Get statistics about the catalog.
    pub fn stats(&self) -> CatalogStats {
        let mut stats = CatalogStats::default();
        for test in self.all_tests() {
            stats.total += 1;
            match test.test_type {
                TestType::Valid => stats.valid += 1,
                TestType::Invalid => stats.invalid += 1,
                TestType::NotWellFormed => stats.not_wf += 1,
                TestType::Error => stats.error += 1,
            }
        }
        stats
    }
}

/// Extract entity file paths from DOCTYPE declaration.
fn extract_entity_files(content: &str) -> Vec<(String, String)> {
    let mut entities = Vec::new();

    // Look for ENTITY declarations like: <!ENTITY sun-valid SYSTEM "sun/sun-valid.xml">
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("<!ENTITY") {
            // Parse: <!ENTITY name SYSTEM "path">
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[2] == "SYSTEM" {
                let name = parts[1].to_string();
                // Extract path from quoted string
                if let Some(start) = line.find('"')
                    && let Some(end) = line[start + 1..].find('"')
                {
                    let path = line[start + 1..start + 1 + end].to_string();
                    entities.push((name, path));
                }
            }
        }
    }

    entities
}

/// Parse catalog content and extract test suites.
fn parse_catalog_content(
    content: &str,
    base_path: &Path,
    suites: &mut Vec<TestSuite>,
) -> Result<(), XmlConfError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut current_suite: Option<TestSuite> = None;
    let mut current_base = base_path.to_path_buf();
    let mut base_stack: Vec<PathBuf> = vec![base_path.to_path_buf()];

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                match local_name {
                    "TESTSUITE" => {
                        // Root element, just continue
                    }
                    "TESTCASES" => {
                        // Parse suite/testcases attributes
                        let attrs = parse_attributes(&e)?;

                        // Update base path if xml:base is specified
                        if let Some(base) = attrs.get("xml:base") {
                            current_base = current_base.join(base);
                        }
                        base_stack.push(current_base.clone());

                        // Create new suite
                        let name = attrs
                            .get("PROFILE")
                            .cloned()
                            .unwrap_or_else(|| "Unknown".to_string());

                        // Save previous suite if exists
                        if let Some(suite) = current_suite.take()
                            && !suite.tests.is_empty()
                        {
                            suites.push(suite);
                        }

                        current_suite = Some(TestSuite {
                            name,
                            tests: Vec::new(),
                        });
                    }
                    "TEST" => {
                        // Parse test case
                        if let Some(test) = parse_test(&e, &current_base)? {
                            if let Some(ref mut suite) = current_suite {
                                suite.tests.push(test);
                            } else {
                                // Create a default suite if none exists
                                let mut suite = TestSuite {
                                    name: "Default".to_string(),
                                    tests: Vec::new(),
                                };
                                suite.tests.push(test);
                                current_suite = Some(suite);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                if local_name == "TESTCASES" {
                    if let Some(suite) = current_suite.take()
                        && !suite.tests.is_empty()
                    {
                        suites.push(suite);
                    }
                    base_stack.pop();
                    current_base = base_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(|| base_path.to_path_buf());
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(XmlConfError::Parse(e.to_string())),
            _ => {}
        }
    }

    // Don't forget the last suite
    if let Some(suite) = current_suite.take()
        && !suite.tests.is_empty()
    {
        suites.push(suite);
    }

    Ok(())
}

/// Statistics about the catalog.
#[derive(Debug, Default)]
pub struct CatalogStats {
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub not_wf: usize,
    pub error: usize,
}

/// Parse attributes from an element.
fn parse_attributes(e: &BytesStart) -> Result<HashMap<String, String>, XmlConfError> {
    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| XmlConfError::Parse(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|e| XmlConfError::Parse(e.to_string()))?
            .to_string();
        let value = attr
            .unescape_value()
            .map_err(|e| XmlConfError::Parse(e.to_string()))?
            .to_string();
        attrs.insert(key, value);
    }
    Ok(attrs)
}

/// Parse a TEST element.
fn parse_test(e: &BytesStart, base_path: &Path) -> Result<Option<XmlConfTest>, XmlConfError> {
    let attrs = parse_attributes(e)?;

    // ID and TYPE are required
    let id = match attrs.get("ID") {
        Some(id) => id.clone(),
        None => return Ok(None),
    };

    let test_type = match attrs.get("TYPE").and_then(|s| TestType::from_str(s)) {
        Some(t) => t,
        None => return Ok(None),
    };

    let uri = match attrs.get("URI") {
        Some(uri) => base_path.join(uri),
        None => return Ok(None),
    };

    let output = attrs.get("OUTPUT").map(|s| base_path.join(s));

    Ok(Some(XmlConfTest {
        id,
        test_type,
        uri,
        output,
        description: None,
        sections: attrs.get("SECTIONS").cloned(),
        version: attrs.get("VERSION").cloned(),
        edition: attrs.get("EDITION").cloned(),
        namespace: attrs.get("NAMESPACE").cloned(),
        entities: attrs.get("ENTITIES").cloned(),
    }))
}

/// Error type for catalog parsing.
#[derive(Debug)]
pub enum XmlConfError {
    Io(std::io::Error),
    Parse(String),
    InvalidPath,
}

impl std::fmt::Display for XmlConfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(e) => write!(f, "Parse error: {}", e),
            Self::InvalidPath => write!(f, "Invalid path"),
        }
    }
}

impl std::error::Error for XmlConfError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_parsing() {
        assert_eq!(TestType::from_str("valid"), Some(TestType::Valid));
        assert_eq!(TestType::from_str("invalid"), Some(TestType::Invalid));
        assert_eq!(TestType::from_str("not-wf"), Some(TestType::NotWellFormed));
        assert_eq!(TestType::from_str("error"), Some(TestType::Error));
        assert_eq!(TestType::from_str("unknown"), None);
    }

    #[test]
    fn test_extract_entity_files() {
        let content = r#"<!DOCTYPE TESTSUITE SYSTEM "testcases.dtd" [
    <!ENTITY sun-valid SYSTEM "sun/sun-valid.xml">
    <!ENTITY jclark-xmltest SYSTEM "xmltest/xmltest.xml">
]>"#;
        let entities = extract_entity_files(content);
        assert_eq!(entities.len(), 2);
        assert!(
            entities
                .iter()
                .any(|(n, p)| n == "sun-valid" && p == "sun/sun-valid.xml")
        );
        assert!(
            entities
                .iter()
                .any(|(n, p)| n == "jclark-xmltest" && p == "xmltest/xmltest.xml")
        );
    }
}
