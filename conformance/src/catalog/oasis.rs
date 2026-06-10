//! OASIS XPath 1.0 Test Suite catalog parser.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// An XPath test case.
#[derive(Debug, Clone)]
pub struct XPathTest {
    /// Test ID.
    pub id: String,
    /// Description of the test.
    pub description: Option<String>,
    /// The XPath expression to evaluate.
    pub xpath: String,
    /// Path to the input XML file.
    pub input_file: PathBuf,
    /// Expected result.
    pub expected: ExpectedResult,
    /// Optional context node XPath (where to start evaluation).
    pub context: Option<String>,
}

/// Expected result of an XPath evaluation.
#[derive(Debug, Clone)]
pub enum ExpectedResult {
    /// Expected nodeset (as XPath expressions that should match).
    NodeSet(Vec<String>),
    /// Expected string value.
    String(String),
    /// Expected number value.
    Number(f64),
    /// Expected boolean value.
    Boolean(bool),
    /// Expected error (parsing or evaluation should fail).
    Error,
}

/// A collection of XPath tests.
#[derive(Debug, Clone)]
pub struct XPathTestSuite {
    /// All test cases.
    pub tests: Vec<XPathTest>,
    /// Base path for test files.
    pub base_path: PathBuf,
}

impl XPathTestSuite {
    /// Parse an XPath test catalog file.
    ///
    /// The format is expected to be a simple XML structure with test elements.
    pub fn parse(catalog_path: &Path) -> Result<Self, XPathTestError> {
        let base_path = catalog_path
            .parent()
            .ok_or(XPathTestError::InvalidPath)?
            .to_path_buf();

        let content = fs::read_to_string(catalog_path).map_err(XPathTestError::Io)?;

        let mut reader = Reader::from_str(&content);
        reader.config_mut().trim_text(true);

        let mut tests = Vec::new();
        let mut current_test: Option<XPathTestBuilder> = None;
        let mut in_element: Option<String> = None;
        let mut text_buffer = String::new();

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let local_name_bytes = e.local_name();
                    let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                    match local_name {
                        "test" | "test-case" => {
                            let attrs = parse_attributes(&e)?;
                            current_test = Some(XPathTestBuilder {
                                id: attrs.get("id").cloned(),
                                description: attrs.get("description").cloned(),
                                xpath: None,
                                input_file: attrs.get("input").or(attrs.get("file")).cloned(),
                                expected: None,
                                context: attrs.get("context").cloned(),
                            });
                        }
                        "xpath" | "expression" | "expr" => {
                            in_element = Some("xpath".to_string());
                            text_buffer.clear();
                        }
                        "expected" | "result" => {
                            let attrs = parse_attributes(&e)?;
                            if let Some(ref mut test) = current_test
                                && let Some(type_str) = attrs.get("type")
                            {
                                test.expected = Some(match type_str.as_str() {
                                    "error" => ExpectedResult::Error,
                                    "boolean" => ExpectedResult::Boolean(
                                        attrs.get("value").is_some_and(|v| v == "true" || v == "1"),
                                    ),
                                    "number" => ExpectedResult::Number(
                                        attrs
                                            .get("value")
                                            .and_then(|v| v.parse().ok())
                                            .unwrap_or(0.0),
                                    ),
                                    _ => ExpectedResult::String(
                                        attrs.get("value").cloned().unwrap_or_default(),
                                    ),
                                });
                            }
                            in_element = Some("expected".to_string());
                            text_buffer.clear();
                        }
                        "input" | "input-file" => {
                            in_element = Some("input".to_string());
                            text_buffer.clear();
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_element.is_some() {
                        text_buffer.push_str(&e.unescape().unwrap_or_default());
                    }
                }
                Ok(Event::End(e)) => {
                    let local_name_bytes = e.local_name();
                    let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                    match local_name {
                        "test" | "test-case" => {
                            if let Some(builder) = current_test.take()
                                && let Some(test) = builder.build(&base_path)
                            {
                                tests.push(test);
                            }
                        }
                        "xpath" | "expression" | "expr" => {
                            if let Some(ref mut test) = current_test
                                && !text_buffer.trim().is_empty()
                            {
                                test.xpath = Some(text_buffer.trim().to_string());
                            }
                            in_element = None;
                        }
                        "expected" | "result" => {
                            if let Some(ref mut test) = current_test
                                && test.expected.is_none()
                                && !text_buffer.trim().is_empty()
                            {
                                test.expected =
                                    Some(ExpectedResult::String(text_buffer.trim().to_string()));
                            }
                            in_element = None;
                        }
                        "input" | "input-file" => {
                            if let Some(ref mut test) = current_test
                                && !text_buffer.trim().is_empty()
                            {
                                test.input_file = Some(text_buffer.trim().to_string());
                            }
                            in_element = None;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(XPathTestError::Parse(e.to_string())),
                _ => {}
            }
        }

        Ok(Self { tests, base_path })
    }

    /// Get tests that expect a specific result type.
    pub fn tests_expecting_string(&self) -> impl Iterator<Item = &XPathTest> {
        self.tests
            .iter()
            .filter(|t| matches!(t.expected, ExpectedResult::String(_)))
    }

    /// Get tests that expect an error.
    pub fn tests_expecting_error(&self) -> impl Iterator<Item = &XPathTest> {
        self.tests
            .iter()
            .filter(|t| matches!(t.expected, ExpectedResult::Error))
    }

    /// Get statistics.
    pub fn stats(&self) -> XPathTestStats {
        let mut stats = XPathTestStats::default();
        for test in &self.tests {
            stats.total += 1;
            match test.expected {
                ExpectedResult::NodeSet(_) => stats.nodeset += 1,
                ExpectedResult::String(_) => stats.string += 1,
                ExpectedResult::Number(_) => stats.number += 1,
                ExpectedResult::Boolean(_) => stats.boolean += 1,
                ExpectedResult::Error => stats.error += 1,
            }
        }
        stats
    }
}

/// Statistics about XPath tests.
#[derive(Debug, Default)]
pub struct XPathTestStats {
    pub total: usize,
    pub nodeset: usize,
    pub string: usize,
    pub number: usize,
    pub boolean: usize,
    pub error: usize,
}

/// Builder for XPath tests during parsing.
#[derive(Debug, Default)]
struct XPathTestBuilder {
    id: Option<String>,
    description: Option<String>,
    xpath: Option<String>,
    input_file: Option<String>,
    expected: Option<ExpectedResult>,
    context: Option<String>,
}

impl XPathTestBuilder {
    fn build(self, base_path: &Path) -> Option<XPathTest> {
        let xpath = self.xpath?;
        let input_file = base_path.join(self.input_file.as_deref().unwrap_or("input.xml"));
        let expected = self
            .expected
            .unwrap_or(ExpectedResult::String(String::new()));

        Some(XPathTest {
            id: self.id.unwrap_or_else(|| format!("test_{}", xpath.len())),
            description: self.description,
            xpath,
            input_file,
            expected,
            context: self.context,
        })
    }
}

/// Parse attributes from an element.
fn parse_attributes(e: &BytesStart) -> Result<HashMap<String, String>, XPathTestError> {
    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| XPathTestError::Parse(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|e| XPathTestError::Parse(e.to_string()))?
            .to_string();
        let value = attr
            .unescape_value()
            .map_err(|e| XPathTestError::Parse(e.to_string()))?
            .to_string();
        attrs.insert(key, value);
    }
    Ok(attrs)
}

/// Error type for XPath test parsing.
#[derive(Debug)]
pub enum XPathTestError {
    Io(std::io::Error),
    Parse(String),
    InvalidPath,
}

impl std::fmt::Display for XPathTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(e) => write!(f, "Parse error: {}", e),
            Self::InvalidPath => write!(f, "Invalid path"),
        }
    }
}

impl std::error::Error for XPathTestError {}
