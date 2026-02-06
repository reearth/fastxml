//! Helper types and functions for streaming processing.

use std::collections::HashMap;
use std::io::Write;

use crate::namespace::Namespace;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};
use crate::xpath::parser::ComparisonOp;

use super::super::context::{AncestorInfo, TransformContext};
use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{ErrorLocation, TransformError, TransformResult};
use super::super::xpath_analyze::{AttributePredicate, PositionPredicate, StreamableXPath};
use quick_xml::events::{BytesEnd, BytesStart};

/// Creates an XML parse error with location information.
pub(crate) fn xml_parse_error_with_location(
    message: impl Into<String>,
    byte_offset: usize,
    input: &str,
    xpath: Option<String>,
) -> TransformError {
    let mut location = ErrorLocation::from_offset_with_input(byte_offset, input);
    if let Some(path) = xpath {
        location = location.with_xpath(path);
    }
    TransformError::XmlParseWithLocation {
        message: message.into(),
        location,
    }
}

/// Creates an XML parse error with byte offset only (no input string for line calculation).
pub(crate) fn xml_parse_error_at_offset(
    message: impl Into<String>,
    byte_offset: usize,
    xpath: Option<String>,
) -> TransformError {
    let mut location = ErrorLocation::from_offset(byte_offset);
    if let Some(path) = xpath {
        location = location.with_xpath(path);
    }
    TransformError::XmlParseWithLocation {
        message: message.into(),
        location,
    }
}

/// Tracks the current element path for XPath matching.
pub struct PathTracker {
    /// Stack of element info for current path
    path: Vec<ElementInfo>,
    /// Position counters for each level (for position predicates)
    position_counters: Vec<HashMap<String, usize>>,
}

/// Information about an element in the path.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    /// Local name
    pub name: String,
    /// Namespace prefix
    pub prefix: Option<String>,
    /// Namespace URI (resolved from prefix using registered namespaces)
    pub namespace_uri: Option<String>,
    /// Attributes (name -> value)
    pub attributes: HashMap<String, String>,
    /// Byte offset where this element starts (position of '<')
    pub start_offset: usize,
}

impl PathTracker {
    /// Creates a new path tracker.
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            position_counters: vec![HashMap::new()], // Root level counter
        }
    }

    /// Pushes a new element onto the path stack.
    pub fn push_element(&mut self, info: ElementInfo) {
        // Update position counter for this element name at current level
        let level = self.position_counters.last_mut().unwrap();
        let qname = match &info.prefix {
            Some(p) => format!("{}:{}", p, info.name),
            None => info.name.clone(),
        };
        *level.entry(qname).or_insert(0) += 1;

        self.path.push(info);
        // Add new level for children
        self.position_counters.push(HashMap::new());
    }

    /// Pops the current element from the path stack.
    pub fn pop_element(&mut self) {
        self.path.pop();
        self.position_counters.pop();
    }

    /// Returns the current depth.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Returns the current element info (if any).
    pub fn current(&self) -> Option<&ElementInfo> {
        self.path.last()
    }

    /// Returns the current position of the latest element among siblings with the same name.
    pub fn current_position(&self) -> usize {
        if let Some(current) = self.current() {
            let qname = match &current.prefix {
                Some(p) => format!("{}:{}", p, current.name),
                None => current.name.clone(),
            };
            // Position counter is at parent level
            if self.position_counters.len() >= 2 {
                let parent_level = &self.position_counters[self.position_counters.len() - 2];
                return *parent_level.get(&qname).unwrap_or(&0);
            }
        }
        0
    }

    /// Returns an XPath-like string representing the current path.
    ///
    /// The path includes position predicates for elements with siblings of the same name.
    /// Example: `/root/items[1]/item[3]`
    pub fn current_xpath(&self) -> String {
        if self.path.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        for (i, info) in self.path.iter().enumerate() {
            let qname = match &info.prefix {
                Some(p) => format!("{}:{}", p, info.name),
                None => info.name.clone(),
            };

            // Get position for this element
            let position = if i == 0 {
                1 // Root is always position 1
            } else {
                *self
                    .position_counters
                    .get(i)
                    .and_then(|m| m.get(&qname))
                    .unwrap_or(&1)
            };

            // Add position predicate only if there could be siblings
            // For simplicity, always add position predicate
            parts.push(format!("{}[{}]", qname, position));
        }

        format!("/{}", parts.join("/"))
    }

    /// Creates a TransformContext from the current state.
    ///
    /// The context includes all ancestors (excluding the current element),
    /// the current position, and depth.
    pub fn to_context(&self) -> TransformContext {
        // Build ancestors (all elements except the current one)
        let ancestors: Vec<AncestorInfo> = self.path[..self.path.len().saturating_sub(1)]
            .iter()
            .enumerate()
            .map(|(i, info)| {
                // Get position for this ancestor
                let position = if i == 0 {
                    1 // Root is always position 1
                } else {
                    let qname = match &info.prefix {
                        Some(p) => format!("{}:{}", p, info.name),
                        None => info.name.clone(),
                    };
                    // Position counter is at the parent's level
                    *self
                        .position_counters
                        .get(i)
                        .and_then(|m| m.get(&qname))
                        .unwrap_or(&1)
                };

                AncestorInfo::new(
                    info.name.clone(),
                    info.prefix.clone(),
                    info.attributes.clone(),
                    position,
                    i + 1, // depth is 1-indexed
                )
            })
            .collect();

        TransformContext::new(ancestors, self.current_position(), self.depth())
    }

    /// Checks if the current path matches the streamable XPath.
    pub fn matches(&self, xpath: &StreamableXPath) -> bool {
        if xpath.steps.is_empty() {
            return false;
        }

        // For descendant searches, we need to check if any suffix of the path matches
        let first_step = &xpath.steps[0];
        if first_step.descendant_or_self {
            // For //element patterns, check if current element matches
            if xpath.steps.len() == 1 {
                return self.matches_step(&xpath.steps[0], self.depth() - 1);
            }
            // For more complex patterns like //a/b, check tail matching
            // This is a simplification - full implementation would track all potential matches
            return self.matches_step(&xpath.steps[0], self.depth() - 1);
        }

        // For absolute paths, match from root
        if xpath.absolute {
            if self.path.len() != xpath.steps.len() {
                return false;
            }
            for (i, step) in xpath.steps.iter().enumerate() {
                if !self.matches_step(step, i) {
                    return false;
                }
            }
            return true;
        }

        // For relative paths (not common in transform context)
        false
    }

    fn matches_step(
        &self,
        step: &super::super::xpath_analyze::StreamableStep,
        path_index: usize,
    ) -> bool {
        if path_index >= self.path.len() {
            return false;
        }

        let element = &self.path[path_index];

        // Check name match
        if let Some(ref name) = step.name {
            if element.name != *name {
                return false;
            }
        }

        // Check namespace URI match (takes precedence over prefix match)
        if let Some(ref expected_uri) = step.namespace_uri {
            match &element.namespace_uri {
                Some(uri) if uri == expected_uri => {}
                _ => return false,
            }
        } else if let Some(ref prefix) = step.prefix {
            // Check prefix match only if no namespace_uri is specified
            match &element.prefix {
                Some(p) if p == prefix => {}
                _ => return false,
            }
        }

        // Check attribute predicates
        for attr_pred in &step.attribute_predicates {
            if !self.matches_attribute_predicate(element, attr_pred) {
                return false;
            }
        }

        // Check position predicate
        if let Some(ref pos_pred) = step.position_predicate {
            let position = self.current_position();
            if !self.matches_position_predicate(position, pos_pred) {
                return false;
            }
        }

        true
    }

    fn matches_attribute_predicate(
        &self,
        element: &ElementInfo,
        pred: &AttributePredicate,
    ) -> bool {
        match element.attributes.get(&pred.name) {
            Some(value) => match pred.op {
                ComparisonOp::Equal => *value == pred.value,
                ComparisonOp::NotEqual => {
                    if pred.value.is_empty() {
                        // Existence check
                        true
                    } else {
                        *value != pred.value
                    }
                }
                _ => false, // Other comparisons not supported for attributes
            },
            None => {
                // Attribute doesn't exist
                pred.op == ComparisonOp::NotEqual && !pred.value.is_empty()
            }
        }
    }

    fn matches_position_predicate(&self, position: usize, pred: &PositionPredicate) -> bool {
        match pred {
            PositionPredicate::Exact(n) => position == *n,
            PositionPredicate::LessOrEqual(n) => position <= *n,
            PositionPredicate::LessThan(n) => position < *n,
            PositionPredicate::GreaterOrEqual(n) => position >= *n,
            PositionPredicate::GreaterThan(n) => position > *n,
        }
    }
}

impl Default for PathTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn extract_element_info(
    e: &BytesStart,
    start_offset: usize,
    namespaces: &HashMap<String, String>,
) -> TransformResult<ElementInfo> {
    let name_bytes = e.name();
    let full_name = std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

    let (prefix, name) = match full_name.split_once(':') {
        Some((p, n)) => (Some(p.to_string()), n.to_string()),
        None => (None, full_name.to_string()),
    };

    // Resolve namespace URI from prefix using registered namespaces
    let namespace_uri = prefix
        .as_ref()
        .and_then(|p| namespaces.get(p).cloned())
        .or_else(|| {
            // Check for default namespace (empty prefix)
            namespaces.get("").cloned()
        });

    let mut attributes = HashMap::new();
    for attr in e.attributes().filter_map(|a| a.ok()) {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
        let value = attr
            .unescape_value()
            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
        attributes.insert(key.to_string(), value.to_string());
    }

    Ok(ElementInfo {
        name,
        prefix,
        namespace_uri,
        attributes,
        start_offset,
    })
}

pub(crate) fn add_start_to_builder(
    builder: &mut EditableNodeBuilder,
    e: &BytesStart,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    let name_bytes = e.name();
    let full_name = std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

    let (prefix, name) = match full_name.split_once(':') {
        Some((p, n)) => (Some(p), n),
        None => (None, full_name),
    };

    let namespace_uri = prefix.and_then(|p| namespaces.get(p).map(|s| s.as_str()));

    let mut attributes = Vec::new();
    let mut attr_ns_info = Vec::new();
    let mut ns_decls = Vec::new();

    for attr in e.attributes().filter_map(|a| a.ok()) {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
        let value = attr
            .unescape_value()
            .map_err(|err| TransformError::XmlParse(err.to_string()))?;

        if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            ns_decls.push(Namespace::new(ns_prefix, value.as_ref()));
        } else if key == "xmlns" {
            ns_decls.push(Namespace::new("", value.as_ref()));
        } else {
            // Store attributes with local names only (libxml compatible)
            let (attr_prefix, local_name) = match key.split_once(':') {
                Some((p, local)) => (Some(p), local),
                None => (None, key),
            };
            attributes.push((local_name.to_string(), value.to_string()));
            if let Some(p) = attr_prefix {
                if let Some(uri) = namespaces.get(p) {
                    attr_ns_info.push((local_name.to_string(), p.to_string(), uri.clone()));
                }
            }
        }
    }

    // Convert to references for the builder
    let attr_refs: Vec<(&str, &str)> = attributes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let attr_ns_refs: Vec<(&str, &str, &str)> = attr_ns_info
        .iter()
        .map(|(l, p, u)| (l.as_str(), p.as_str(), u.as_str()))
        .collect();

    builder.start_element(
        name,
        prefix,
        namespace_uri,
        attr_refs,
        attr_ns_refs,
        ns_decls,
    );

    Ok(())
}

pub(crate) fn add_empty_to_builder(
    builder: &mut EditableNodeBuilder,
    e: &BytesStart,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    add_start_to_builder(builder, e, namespaces)?;
    builder.end_element();
    Ok(())
}

pub(crate) fn add_end_to_builder(
    builder: &mut EditableNodeBuilder,
    _e: &BytesEnd,
) -> TransformResult<()> {
    builder.end_element();
    Ok(())
}

pub(crate) fn serialize_editable<W: Write>(
    editable: &EditableNode,
    writer: &mut W,
) -> TransformResult<()> {
    let root = editable
        .document()
        .get_root_element()
        .map_err(|e| TransformError::Serialization(e.to_string()))?;

    let xml =
        node_to_xml_string_with_options(editable.document(), &root, &SerializeOptions::default())
            .map_err(|e| TransformError::Serialization(e.to_string()))?;

    writer.write_all(xml.as_bytes())?;
    Ok(())
}
