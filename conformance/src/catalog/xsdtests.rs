//! W3C XML Schema Test Suite catalog parser.
//!
//! Parses the suite.xml and .testSet files from the W3C XSD Test Suite.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A schema test group containing a schema and instance documents.
#[derive(Debug, Clone)]
pub struct SchemaTestGroup {
    /// Group name/ID.
    pub name: String,
    /// Description.
    pub description: Option<String>,
    /// Schema documents for this group.
    pub schemas: Vec<SchemaDocument>,
    /// Instance tests for this group.
    pub instances: Vec<InstanceTest>,
}

/// A schema document in a test group.
#[derive(Debug, Clone)]
pub struct SchemaDocument {
    /// Path to the schema file.
    pub path: PathBuf,
    /// Expected validity: "valid", "invalid", "indeterminate".
    pub expected: SchemaValidity,
}

/// Expected validity of a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaValidity {
    Valid,
    Invalid,
    Indeterminate,
}

impl SchemaValidity {
    fn from_str(s: &str) -> Self {
        match s {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            _ => Self::Indeterminate,
        }
    }
}

/// An instance document test.
#[derive(Debug, Clone)]
pub struct InstanceTest {
    /// Test name/ID.
    pub name: String,
    /// Path to the instance document.
    pub path: PathBuf,
    /// Expected validity.
    pub expected: InstanceValidity,
}

/// Expected validity of an instance document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceValidity {
    Valid,
    Invalid,
    Indeterminate,
}

impl InstanceValidity {
    fn from_str(s: &str) -> Self {
        match s {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            _ => Self::Indeterminate,
        }
    }
}

/// A test set containing multiple test groups.
#[derive(Debug, Clone)]
pub struct TestSet {
    /// Name of the test set.
    pub name: String,
    /// Path to the test set file.
    pub path: PathBuf,
    /// Test groups in this set.
    pub groups: Vec<SchemaTestGroup>,
}

/// The complete XSD test suite.
#[derive(Debug, Clone)]
pub struct XsdTestSuite {
    /// All test sets.
    pub test_sets: Vec<TestSet>,
    /// Base path to the suite directory.
    pub base_path: PathBuf,
}

impl XsdTestSuite {
    /// Parse the suite.xml file and all referenced test sets.
    pub fn parse(suite_path: &Path) -> Result<Self, XsdTestError> {
        let base_path = suite_path
            .parent()
            .ok_or(XsdTestError::InvalidPath)?
            .to_path_buf();

        let content = fs::read_to_string(suite_path).map_err(XsdTestError::Io)?;

        // Parse suite.xml to get list of testSet files
        let test_set_refs = parse_suite_xml(&content)?;

        let mut test_sets = Vec::new();
        for ts_ref in test_set_refs {
            let ts_path = base_path.join(&ts_ref);
            if ts_path.exists() {
                match parse_test_set(&ts_path) {
                    Ok(ts) => test_sets.push(ts),
                    Err(e) => {
                        eprintln!("Warning: Failed to parse {}: {}", ts_path.display(), e);
                    }
                }
            }
        }

        Ok(Self {
            test_sets,
            base_path,
        })
    }

    /// Get all test groups across all sets.
    pub fn all_groups(&self) -> impl Iterator<Item = &SchemaTestGroup> {
        self.test_sets.iter().flat_map(|ts| ts.groups.iter())
    }

    /// Get statistics.
    pub fn stats(&self) -> XsdTestStats {
        let mut stats = XsdTestStats::default();
        for group in self.all_groups() {
            stats.groups += 1;
            stats.schemas += group.schemas.len();
            stats.instances += group.instances.len();
            for instance in &group.instances {
                match instance.expected {
                    InstanceValidity::Valid => stats.valid_instances += 1,
                    InstanceValidity::Invalid => stats.invalid_instances += 1,
                    InstanceValidity::Indeterminate => {}
                }
            }
        }
        stats
    }
}

/// Statistics about the XSD test suite.
#[derive(Debug, Default)]
pub struct XsdTestStats {
    pub groups: usize,
    pub schemas: usize,
    pub instances: usize,
    pub valid_instances: usize,
    pub invalid_instances: usize,
}

/// Parse the suite.xml file to get test set references.
fn parse_suite_xml(content: &str) -> Result<Vec<String>, XsdTestError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut refs = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                if local_name == "testSetRef" {
                    let attrs = parse_attributes(&e)?;
                    if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                        refs.push(href.clone());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(XsdTestError::Parse(e.to_string())),
            _ => {}
        }
    }

    Ok(refs)
}

/// Parse a test set file.
fn parse_test_set(path: &Path) -> Result<TestSet, XsdTestError> {
    let base_path = path.parent().unwrap_or(Path::new("."));
    let content = fs::read_to_string(path).map_err(XsdTestError::Io)?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut groups = Vec::new();
    let mut current_group: Option<SchemaTestGroup> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                match local_name {
                    "testGroup" => {
                        let attrs = parse_attributes(&e)?;
                        current_group = Some(SchemaTestGroup {
                            name: attrs.get("name").cloned().unwrap_or_default(),
                            description: None,
                            schemas: Vec::new(),
                            instances: Vec::new(),
                        });
                    }
                    "schemaTest" | "schemaDocument" => {
                        if let Some(ref mut group) = current_group {
                            let attrs = parse_attributes(&e)?;
                            if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                                let expected = attrs
                                    .get("validity")
                                    .map_or(SchemaValidity::Valid, |s| SchemaValidity::from_str(s));
                                group.schemas.push(SchemaDocument {
                                    path: base_path.join(href),
                                    expected,
                                });
                            }
                        }
                    }
                    "instanceTest" | "instanceDocument" => {
                        if let Some(ref mut group) = current_group {
                            let attrs = parse_attributes(&e)?;
                            if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                                let name = attrs.get("name").cloned().unwrap_or_default();
                                let expected =
                                    attrs.get("validity").map_or(InstanceValidity::Valid, |s| {
                                        InstanceValidity::from_str(s)
                                    });
                                group.instances.push(InstanceTest {
                                    name,
                                    path: base_path.join(href),
                                    expected,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                // Handle self-closing elements
                if local_name == "schemaDocument" {
                    if let Some(ref mut group) = current_group {
                        let attrs = parse_attributes(&e)?;
                        if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                            let expected = attrs
                                .get("validity")
                                .map_or(SchemaValidity::Valid, |s| SchemaValidity::from_str(s));
                            group.schemas.push(SchemaDocument {
                                path: base_path.join(href),
                                expected,
                            });
                        }
                    }
                } else if local_name == "instanceDocument" {
                    if let Some(ref mut group) = current_group {
                        let attrs = parse_attributes(&e)?;
                        if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                            let name = attrs.get("name").cloned().unwrap_or_default();
                            let expected = attrs
                                .get("validity")
                                .map_or(InstanceValidity::Valid, |s| InstanceValidity::from_str(s));
                            group.instances.push(InstanceTest {
                                name,
                                path: base_path.join(href),
                                expected,
                            });
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                if local_name == "testGroup" {
                    if let Some(group) = current_group.take() {
                        if !group.schemas.is_empty() || !group.instances.is_empty() {
                            groups.push(group);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(XsdTestError::Parse(e.to_string())),
            _ => {}
        }
    }

    Ok(TestSet {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        path: path.to_path_buf(),
        groups,
    })
}

/// Parse attributes from an element.
fn parse_attributes(e: &BytesStart) -> Result<HashMap<String, String>, XsdTestError> {
    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| XsdTestError::Parse(e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|e| XsdTestError::Parse(e.to_string()))?
            .to_string();
        let value = attr
            .unescape_value()
            .map_err(|e| XsdTestError::Parse(e.to_string()))?
            .to_string();
        attrs.insert(key, value);
    }
    Ok(attrs)
}

/// Error type for XSD test parsing.
#[derive(Debug)]
pub enum XsdTestError {
    Io(std::io::Error),
    Parse(String),
    InvalidPath,
}

impl std::fmt::Display for XsdTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(e) => write!(f, "Parse error: {}", e),
            Self::InvalidPath => write!(f, "Invalid path"),
        }
    }
}

impl std::error::Error for XsdTestError {}
