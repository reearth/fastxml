//! Single-pass streaming processor for XML transformation.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::{BytesEnd, BytesStart, Event};

use crate::namespace::Namespace;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};
use crate::xpath::parser::ComparisonOp;

use super::editable::{EditableNode, EditableNodeBuilder};
use super::error::{ErrorLocation, TransformError, TransformResult};
use super::xpath_analyze::{AttributePredicate, PositionPredicate, StreamableXPath};

/// Creates an XML parse error with location information.
fn xml_parse_error_with_location(
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
    pub fn to_context(&self) -> super::context::TransformContext {
        use super::context::{AncestorInfo, TransformContext};

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
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

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
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

fn extract_element_info(
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

/// Processes XML with streaming transformation and context.
pub fn process_streaming_with_context<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode, &super::context::TransformContext),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut prev_written: usize = 0;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    // Store context at match start for use when processing complete
    let mut match_context: Option<super::context::TransformContext> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here!
                    // 1. Write everything before this point (zero-copy)
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    // 2. Capture context at match point
                    match_context = Some(tracker.to_context());

                    // 3. Start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    let ctx = tracker.to_context();

                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    // Process immediately since it's complete
                    let mut editable = builder.build()?;
                    transform_fn(&mut editable, &ctx);
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
                        let ctx = match_context.take().unwrap_or_else(|| tracker.to_context());
                        transform_fn(&mut editable, &ctx);
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
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
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here, start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML with streaming iteration and context (no transformation output).
pub fn process_for_each_with_context<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode, &super::context::TransformContext),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Store context at match start for use when processing complete
    let mut match_context: Option<super::context::TransformContext> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here, capture context
                    match_context = Some(tracker.to_context());
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    let ctx = tracker.to_context();
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    let mut editable = builder.build()?;
                    callback(&mut editable, &ctx);
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
                        let ctx = match_context.take().unwrap_or_else(|| tracker.to_context());
                        callback(&mut editable, &ctx);
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// State for tracking a single XPath handler during multi-xpath processing.
struct HandlerState<'a> {
    xpath: &'a StreamableXPath,
    builder: Option<EditableNodeBuilder>,
    match_context: Option<super::context::TransformContext>,
}

/// State for tracking a single XPath handler during multi-xpath transform processing.
/// Includes match_start_offset for zero-copy output.
struct TransformHandlerState<'a> {
    xpath: &'a StreamableXPath,
    builder: Option<EditableNodeBuilder>,
    match_context: Option<super::context::TransformContext>,
    /// Byte offset where the match started (for zero-copy output).
    match_start_offset: usize,
}

/// Handler pair for multi-xpath processing: (xpath, callback).
pub type MultiHandler<'a> = (&'a StreamableXPath, &'a mut dyn FnMut(&mut EditableNode));

/// Handler pair with context for multi-xpath processing: (xpath, callback).
pub type MultiHandlerWithContext<'a> = (
    &'a StreamableXPath,
    &'a mut dyn FnMut(&mut EditableNode, &super::context::TransformContext),
);

/// Handler pair for multi-xpath transform processing: (xpath, callback).
pub type MultiTransformHandler<'a> = (&'a StreamableXPath, &'a mut dyn FnMut(&mut EditableNode));

/// Handler pair with context for multi-xpath transform processing: (xpath, callback).
pub type MultiTransformHandlerWithContext<'a> = (
    &'a StreamableXPath,
    &'a mut dyn FnMut(&mut EditableNode, &super::context::TransformContext),
);

/// Processes XML with streaming iteration for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_for_each` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi<'a>(
    input: &str,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Initialize handler states
    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
            match_context: None,
        })
        .collect();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Already inside matched subtree, keep buffering
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // Match starts here, start buffering subtree
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        states[i].builder = Some(builder);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Inside matched subtree
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // This empty element is a match
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_empty_to_builder(&mut builder, &e, namespaces)?;

                        let mut editable = builder.build()?;
                        handlers[i].1(&mut editable);
                        match_count += 1;
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                // Check each handler for completed subtrees
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, call callback
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            match_count += 1;
                        } else {
                            // Not complete yet, put back
                            states[i].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML with streaming iteration and context for multiple XPath handlers in a single pass.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi_with_context<'a>(
    input: &str,
    handlers: &mut [MultiHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Initialize handler states
    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
            match_context: None,
        })
        .collect();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Already inside matched subtree, keep buffering
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // Match starts here, capture context and start buffering subtree
                        states[i].match_context = Some(tracker.to_context());
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        states[i].builder = Some(builder);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Inside matched subtree
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // This empty element is a match
                        let ctx = tracker.to_context();
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_empty_to_builder(&mut builder, &e, namespaces)?;

                        let mut editable = builder.build()?;
                        handlers[i].1(&mut editable, &ctx);
                        match_count += 1;
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                // Check each handler for completed subtrees
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, call callback
                            let mut editable = builder.build()?;
                            let ctx = states[i]
                                .match_context
                                .take()
                                .unwrap_or_else(|| tracker.to_context());
                            handlers[i].1(&mut editable, &ctx);
                            match_count += 1;
                        } else {
                            // Not complete yet, put back
                            states[i].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML with streaming transformation for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
///
/// # Strategy: First-Match-Wins
///
/// When multiple handlers could match nested elements, only the **first matching handler**
/// (in registration order) is active at a time. While a handler is processing an element's
/// subtree, other handlers cannot match elements within that subtree.
///
/// ## Example: Nested elements
///
/// ```text
/// XML: <outer><inner>content</inner></outer>
/// Handlers: [("//outer", handler1), ("//inner", handler2)]
///
/// Result: Only handler1 is called for <outer>. handler2 is NOT called for <inner>
/// because <inner> is inside the already-matched <outer> subtree.
/// ```
///
/// ## Example: Non-overlapping elements
///
/// ```text
/// XML: <item>A</item><other>B</other>
/// Handlers: [("//item", handler1), ("//other", handler2)]
///
/// Result: Both handlers are called - handler1 for <item>, handler2 for <other>.
/// ```
///
/// This behavior ensures predictable output ordering and prevents duplicate processing,
/// but means you cannot process both a container and its children with separate handlers.
/// If you need to process nested elements independently, use `process_for_each_multi` instead.
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();
    let mut prev_written: usize = 0;

    // Initialize handler states
    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
            match_context: None,
            match_start_offset: 0,
        })
        .collect();

    // Track which handler is currently active (first-match-wins strategy)
    let mut active_handler: Option<usize> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Already inside matched subtree, keep buffering
                    if let Some(ref mut builder) = states[idx].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // Match starts here!
                            // 1. Write everything before this point (zero-copy)
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            // 2. Start buffering subtree
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_start_to_builder(&mut builder, &e, namespaces)?;
                            states[i].builder = Some(builder);
                            states[i].match_start_offset = before_pos;
                            active_handler = Some(i);
                            break; // First match wins
                        }
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Inside matched subtree
                    if let Some(ref mut builder) = states[idx].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // This empty element is a match
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_empty_to_builder(&mut builder, &e, namespaces)?;

                            // Process immediately since it's complete
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            break; // First match wins
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(idx) = active_handler {
                    if let Some(mut builder) = states[idx].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, process it
                            let mut editable = builder.build()?;
                            handlers[idx].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            active_handler = None;
                        } else {
                            // Not complete yet, put back
                            states[idx].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

/// Processes XML with streaming transformation and context for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming_with_context` multiple times.
///
/// # Strategy: First-Match-Wins
///
/// Uses the same first-match-wins strategy as [`process_streaming_multi`].
/// When a handler matches an element, other handlers cannot match elements
/// within that subtree until the first handler's subtree is complete.
///
/// See [`process_streaming_multi`] for detailed examples and behavior documentation.
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi_with_context<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();
    let mut prev_written: usize = 0;

    // Initialize handler states
    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
            match_context: None,
            match_start_offset: 0,
        })
        .collect();

    // Track which handler is currently active (first-match-wins strategy)
    let mut active_handler: Option<usize> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Already inside matched subtree, keep buffering
                    if let Some(ref mut builder) = states[idx].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // Match starts here!
                            // 1. Write everything before this point (zero-copy)
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            // 2. Capture context at match point
                            states[i].match_context = Some(tracker.to_context());

                            // 3. Start buffering subtree
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_start_to_builder(&mut builder, &e, namespaces)?;
                            states[i].builder = Some(builder);
                            states[i].match_start_offset = before_pos;
                            active_handler = Some(i);
                            break; // First match wins
                        }
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Inside matched subtree
                    if let Some(ref mut builder) = states[idx].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // This empty element is a match
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            let ctx = tracker.to_context();

                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_empty_to_builder(&mut builder, &e, namespaces)?;

                            // Process immediately since it's complete
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable, &ctx);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            break; // First match wins
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(idx) = active_handler {
                    if let Some(mut builder) = states[idx].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, process it
                            let mut editable = builder.build()?;
                            let ctx = states[idx]
                                .match_context
                                .take()
                                .unwrap_or_else(|| tracker.to_context());
                            handlers[idx].1(&mut editable, &ctx);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            active_handler = None;
                        } else {
                            // Not complete yet, put back
                            states[idx].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
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

    // =============================================================================
    // Multi Transform Tests
    // =============================================================================

    #[test]
    fn test_multi_transform_non_overlapping() {
        // //item and //other match different elements
        let input = r#"<root><item id="1">A</item><other id="2">B</other></root>"#;
        let xpath_item = get_streamable_xpath("//item");
        let xpath_other = get_streamable_xpath("//other");

        let mut handler1 = |node: &mut EditableNode| {
            node.set_attribute("type", "item");
        };
        let mut handler2 = |node: &mut EditableNode| {
            node.set_attribute("type", "other");
        };

        let mut handlers: Vec<MultiTransformHandler<'_>> =
            vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 2);
        assert!(result.contains(r#"type="item""#));
        assert!(result.contains(r#"type="other""#));
    }

    #[test]
    fn test_multi_transform_interleaved() {
        // <item/><other/><item/> order is preserved
        let input = r#"<root><item>1</item><other>2</other><item>3</item></root>"#;
        let xpath_item = get_streamable_xpath("//item");
        let xpath_other = get_streamable_xpath("//other");

        let mut handler1 = |node: &mut EditableNode| {
            node.set_attribute("type", "item");
        };
        let mut handler2 = |node: &mut EditableNode| {
            node.set_attribute("type", "other");
        };

        let mut handlers: Vec<MultiTransformHandler<'_>> =
            vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 3);

        // Check that order is preserved
        let item1_pos = result.find("<item type=\"item\">1</item>").unwrap();
        let other_pos = result.find("<other type=\"other\">2</other>").unwrap();
        let item2_pos = result.rfind("<item type=\"item\">3</item>").unwrap();

        assert!(item1_pos < other_pos);
        assert!(other_pos < item2_pos);
    }

    #[test]
    fn test_multi_transform_zero_copy() {
        // Non-matched parts are preserved exactly
        let input = r#"<?xml version="1.0"?>
<root>
  <!-- comment -->
  <unchanged>keep me</unchanged>
  <item>transform</item>
  <also-unchanged attr="value">keep this too</also-unchanged>
</root>"#;
        let xpath = get_streamable_xpath("//item");

        let mut handler = |node: &mut EditableNode| {
            node.set_attribute("modified", "true");
        };

        let mut handlers: Vec<MultiTransformHandler<'_>> = vec![(&xpath, &mut handler)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 1);

        // Check that XML declaration is preserved
        assert!(result.starts_with(r#"<?xml version="1.0"?>"#));

        // Check that unchanged elements are preserved exactly
        assert!(result.contains("<unchanged>keep me</unchanged>"));
        assert!(result.contains(r#"<also-unchanged attr="value">keep this too</also-unchanged>"#));

        // Check that comment is preserved
        assert!(result.contains("<!-- comment -->"));

        // Check that transformation was applied
        assert!(result.contains(r#"<item modified="true">transform</item>"#));
    }

    #[test]
    fn test_multi_transform_empty_elements() {
        // Test with empty elements
        let input = r#"<root><item/><other/><item/></root>"#;
        let xpath_item = get_streamable_xpath("//item");
        let xpath_other = get_streamable_xpath("//other");

        let mut handler1 = |node: &mut EditableNode| {
            node.set_attribute("type", "item");
        };
        let mut handler2 = |node: &mut EditableNode| {
            node.set_attribute("type", "other");
        };

        let mut handlers: Vec<MultiTransformHandler<'_>> =
            vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 3);
        assert_eq!(result.matches(r#"type="item""#).count(), 2);
        assert_eq!(result.matches(r#"type="other""#).count(), 1);
    }

    #[test]
    fn test_multi_transform_with_context() {
        // Test context-aware multi transform
        use crate::transform::context::TransformContext;

        let input = r#"<root><items><item>A</item><item>B</item></items></root>"#;
        let xpath = get_streamable_xpath("//item");

        let mut handler = |node: &mut EditableNode, ctx: &TransformContext| {
            node.set_attribute("pos", &ctx.position().to_string());
            node.set_attribute("depth", &ctx.depth().to_string());
        };

        let mut handlers: Vec<MultiTransformHandlerWithContext<'_>> = vec![(&xpath, &mut handler)];

        let mut output = Vec::new();
        let count = process_streaming_multi_with_context(
            input,
            &mut handlers,
            &HashMap::new(),
            &mut output,
        )
        .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 2);
        assert!(result.contains(r#"pos="1""#));
        assert!(result.contains(r#"pos="2""#));
        assert!(result.contains(r#"depth="3""#)); // root=1, items=2, item=3
    }

    #[test]
    fn test_multi_transform_first_match_wins() {
        // Test first-match-wins: if //items matches, //item inside it is ignored
        let input = r#"<root><items><item>A</item></items></root>"#;
        let xpath_items = get_streamable_xpath("//items");
        let xpath_item = get_streamable_xpath("//item");

        let mut items_matched = false;
        let mut item_matched = false;

        let mut handler1 = |_node: &mut EditableNode| {
            items_matched = true;
        };
        let mut handler2 = |_node: &mut EditableNode| {
            item_matched = true;
        };

        // items handler is first, so it wins for nested elements
        let mut handlers: Vec<MultiTransformHandler<'_>> =
            vec![(&xpath_items, &mut handler1), (&xpath_item, &mut handler2)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        // Only items should match (first-match-wins), item is inside items
        assert_eq!(count, 1);
        assert!(items_matched);
        assert!(!item_matched); // Nested item is ignored because items matched first
    }

    #[test]
    fn test_multi_transform_remove_elements() {
        // Test removing elements
        let input = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;
        let xpath_remove = get_streamable_xpath("//remove");

        let mut handler = |node: &mut EditableNode| {
            node.remove();
        };

        let mut handlers: Vec<MultiTransformHandler<'_>> = vec![(&xpath_remove, &mut handler)];

        let mut output = Vec::new();
        let count =
            process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 1);
        assert!(!result.contains("<remove>"));
        assert!(result.contains("<keep>A</keep>"));
        assert!(result.contains("<keep>C</keep>"));
    }
}
