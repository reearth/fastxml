//! Single-pass streaming processor for XML transformation.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::{BytesEnd, BytesStart, Event};

use crate::namespace::Namespace;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};
use crate::xpath::parser::ComparisonOp;

use super::editable::{EditableNode, EditableNodeBuilder};
use super::error::{TransformError, TransformResult};
use super::xpath_analyze::{AttributePredicate, PositionPredicate, StreamableXPath};

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

    fn matches_step(&self, step: &super::xpath_analyze::StreamableStep, path_index: usize) -> bool {
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

        // Check prefix match
        if let Some(ref prefix) = step.prefix {
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

/// Processes XML with streaming transformation.
pub fn process_streaming<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut prev_written: usize = 0;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here!
                    // 1. Write everything before this point (zero-copy)
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    // 2. Start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    let mut builder = EditableNodeBuilder::new();
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    // Process immediately since it's complete
                    let mut editable = builder.build()?;
                    transform_fn(&mut editable);
                    transform_count += 1;

                    if !editable.is_removed() {
                        serialize_editable(&editable, writer)?;
                    }

                    prev_written = after_pos;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, process it
                        let mut editable = builder.build()?;
                        transform_fn(&mut editable);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, writer)?;
                        }

                        prev_written = after_pos;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                // Write remaining (zero-copy)
                writer.write_all(&input.as_bytes()[prev_written..])?;
                break;
            }

            Ok(_) => {
                // PI, Decl, DocType - pass through (handled by writing remaining)
            }

            Err(e) => {
                return Err(TransformError::XmlParse(format!(
                    "Error at position {}: {:?}",
                    reader.buffer_position(),
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

fn extract_element_info(e: &BytesStart, start_offset: usize) -> TransformResult<ElementInfo> {
    let name_bytes = e.name();
    let full_name = std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

    let (prefix, name) = match full_name.split_once(':') {
        Some((p, n)) => (Some(p.to_string()), n.to_string()),
        None => (None, full_name.to_string()),
    };

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
        attributes,
        start_offset,
    })
}

fn add_start_to_builder(
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
            let local_name = match key.split_once(':') {
                Some((_, local)) => local,
                None => key,
            };
            attributes.push((local_name.to_string(), value.to_string()));
        }
    }

    // Convert to references for the builder
    let attr_refs: Vec<(&str, &str)> = attributes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    builder.start_element(name, prefix, namespace_uri, attr_refs, ns_decls);

    Ok(())
}

fn add_empty_to_builder(
    builder: &mut EditableNodeBuilder,
    e: &BytesStart,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    add_start_to_builder(builder, e, namespaces)?;
    builder.end_element();
    Ok(())
}

fn add_end_to_builder(builder: &mut EditableNodeBuilder, _e: &BytesEnd) -> TransformResult<()> {
    builder.end_element();
    Ok(())
}

fn serialize_editable<W: Write>(editable: &EditableNode, writer: &mut W) -> TransformResult<()> {
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

/// Processes XML with streaming iteration (no transformation output).
pub fn process_for_each<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here, start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    let mut builder = EditableNodeBuilder::new();
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    let mut editable = builder.build()?;
                    callback(&mut editable);
                    match_count += 1;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, call callback
                        let mut editable = builder.build()?;
                        callback(&mut editable);
                        match_count += 1;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                return Err(TransformError::XmlParse(format!(
                    "Error at position {}: {:?}",
                    reader.buffer_position(),
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::xpath_analyze::{XPathAnalysis, analyze_xpath};
    use crate::xpath::parser::parse_xpath;

    fn get_streamable_xpath(xpath_str: &str) -> StreamableXPath {
        let expr = parse_xpath(xpath_str).unwrap();
        match analyze_xpath(&expr) {
            XPathAnalysis::Streamable(s) => s,
            XPathAnalysis::NotStreamable(r) => panic!("Expected streamable xpath: {:?}", r),
        }
    }

    #[test]
    fn test_simple_transform() {
        let input = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
        let xpath = get_streamable_xpath("//item[@id='2']");

        let mut output = Vec::new();
        let count = process_streaming(
            input,
            &xpath,
            &HashMap::new(),
            |node| {
                node.set_attribute("modified", "true");
            },
            &mut output,
        )
        .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 1);
        assert!(result.contains(r#"item id="1""#));
        assert!(result.contains(r#"modified="true""#));
    }

    #[test]
    fn test_no_match() {
        let input = r#"<root><item id="1">A</item></root>"#;
        let xpath = get_streamable_xpath("//item[@id='999']");

        let mut output = Vec::new();
        let count =
            process_streaming(input, &xpath, &HashMap::new(), |_node| {}, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 0);
        assert_eq!(result, input);
    }
}
