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
///
/// The W3C XSD test suite format nests documents and expectations inside
/// `schemaTest`/`instanceTest` elements:
///
/// ```xml
/// <testGroup name="...">
///   <schemaTest name="...">
///     <schemaDocument xlink:href="..."/>
///     <expected validity="valid"/>
///   </schemaTest>
///   <instanceTest name="...">
///     <instanceDocument xlink:href="..."/>
///     <expected validity="invalid"/>
///   </instanceTest>
/// </testGroup>
/// ```
///
/// `expected` may carry a `version` attribute (e.g. "1.0", "1.1") when the
/// outcome differs between XSD versions; we target XSD 1.0.
fn parse_test_set(path: &Path) -> Result<TestSet, XsdTestError> {
    let base_path = path.parent().unwrap_or(Path::new("."));
    let content = fs::read_to_string(path).map_err(XsdTestError::Io)?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    #[derive(PartialEq)]
    enum TestCtx {
        None,
        Schema,
        Instance,
    }

    let mut groups = Vec::new();
    let mut current_group: Option<SchemaTestGroup> = None;
    let mut ctx = TestCtx::None;
    let mut pending_docs: Vec<PathBuf> = Vec::new();
    let mut pending_name = String::new();
    // (version, validity) pairs from <expected> elements
    let mut pending_expected: Vec<(Option<String>, String)> = Vec::new();

    loop {
        let event = reader.read_event();
        match event {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                match local_name {
                    "testSet" => {
                        // A 1.1-only test set doesn't apply to an XSD 1.0
                        // processor at all.
                        let attrs = parse_attributes(e)?;
                        if attrs
                            .get("version")
                            .is_some_and(|v| !v.split_whitespace().any(|t| t == "1.0"))
                        {
                            return Ok(TestSet {
                                name: path
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                path: path.to_path_buf(),
                                groups: Vec::new(),
                            });
                        }
                    }
                    "testGroup" => {
                        let attrs = parse_attributes(e)?;
                        // Skip groups that don't apply to XSD 1.0 (the suite
                        // marks 1.1-only groups with version="1.1").
                        let applies = attrs
                            .get("version")
                            .is_none_or(|v| v.split_whitespace().any(|t| t == "1.0"));
                        current_group = applies.then(|| SchemaTestGroup {
                            name: attrs.get("name").cloned().unwrap_or_default(),
                            description: None,
                            schemas: Vec::new(),
                            instances: Vec::new(),
                        });
                    }
                    "schemaTest" => {
                        ctx = TestCtx::Schema;
                        pending_docs.clear();
                        pending_expected.clear();
                    }
                    "instanceTest" => {
                        ctx = TestCtx::Instance;
                        let attrs = parse_attributes(e)?;
                        pending_name = attrs.get("name").cloned().unwrap_or_default();
                        pending_docs.clear();
                        pending_expected.clear();
                    }
                    "schemaDocument" | "instanceDocument" => {
                        let attrs = parse_attributes(e)?;
                        if let Some(href) = attrs.get("href").or(attrs.get("xlink:href")) {
                            pending_docs.push(base_path.join(href));
                        }
                    }
                    "expected" => {
                        let attrs = parse_attributes(e)?;
                        if let Some(validity) = attrs.get("validity") {
                            pending_expected
                                .push((attrs.get("version").cloned(), validity.clone()));
                        }
                    }
                    _ => {}
                }

                // Self-closing schemaTest/instanceTest cannot contain documents;
                // reset context so stray expected elements are not misattributed.
                if matches!(event, Ok(Event::Empty(_)))
                    && (local_name == "schemaTest" || local_name == "instanceTest")
                {
                    ctx = TestCtx::None;
                }
            }
            Ok(Event::End(e)) => {
                let local_name_bytes = e.local_name();
                let local_name = std::str::from_utf8(local_name_bytes.as_ref()).unwrap_or("");

                match local_name {
                    "schemaTest" => {
                        if ctx == TestCtx::Schema
                            && let Some(ref mut group) = current_group
                        {
                            let validity = select_expected(&pending_expected);
                            for doc in pending_docs.drain(..) {
                                group.schemas.push(SchemaDocument {
                                    path: doc,
                                    expected: validity.map_or(
                                        SchemaValidity::Indeterminate,
                                        SchemaValidity::from_str,
                                    ),
                                });
                            }
                        }
                        ctx = TestCtx::None;
                    }
                    "instanceTest" => {
                        if ctx == TestCtx::Instance
                            && let Some(ref mut group) = current_group
                        {
                            let validity = select_expected(&pending_expected);
                            for doc in pending_docs.drain(..) {
                                group.instances.push(InstanceTest {
                                    name: pending_name.clone(),
                                    path: doc,
                                    expected: validity.map_or(
                                        InstanceValidity::Indeterminate,
                                        InstanceValidity::from_str,
                                    ),
                                });
                            }
                        }
                        ctx = TestCtx::None;
                    }
                    "testGroup" => {
                        if let Some(group) = current_group.take()
                            && (!group.schemas.is_empty() || !group.instances.is_empty())
                        {
                            groups.push(group);
                        }
                    }
                    _ => {}
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

/// Select the expected validity that applies to XSD 1.0.
///
/// Preference order: an unversioned `<expected>`, then one whose `version`
/// mentions "1.0". Entries for other versions only (e.g. "1.1") yield `None`,
/// which callers treat as indeterminate (skipped).
fn select_expected(expected: &[(Option<String>, String)]) -> Option<&str> {
    if let Some((_, v)) = expected.iter().find(|(ver, _)| ver.is_none()) {
        return Some(v);
    }
    if let Some((_, v)) = expected.iter().find(|(ver, _)| {
        ver.as_deref()
            .is_some_and(|s| s.split_whitespace().any(|t| t == "1.0"))
    }) {
        return Some(v);
    }
    None
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
