//! XSD Parser using XmlEventHandler.
//!
//! This parser converts XSD XML into AST types using a stack-based approach.

mod finishers;
mod handlers;
mod helpers;
mod rules;
mod stack_frame;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::error::Result;
use crate::event::{RawEvent, XmlEventHandler};
use crate::position::PositionTrackingReader;

use super::types::*;

pub use helpers::XSD_NAMESPACE;
use helpers::split_start_event;
use stack_frame::StackFrame;

/// XSD Parser that implements XmlEventHandler.
pub struct XsdParser {
    /// Stack of parsing states
    stack: Vec<StackFrame>,
    /// The schema being built
    schema: XsdSchema,
    /// Detected XSD namespace prefix.
    /// - `None` = not yet detected
    /// - `Some(None)` = default namespace (no prefix)
    /// - `Some(Some("xs"))` = prefix "xs"
    xsd_prefix: Option<Option<String>>,
    /// Current text content being collected
    current_text: String,
    /// Depth counter for skipping annotation content
    skip_depth: usize,
    /// Default-namespace URI in scope per open element (None = no default
    /// namespace). Tracks `xmlns="..."` overrides so unprefixed XSD elements
    /// are recognized even when the schema root uses a prefix.
    default_ns_stack: Vec<Option<String>>,
    /// Per-open-XSD-element child bookkeeping for structural rules
    /// (annotation placement, notation placement).
    child_state_stack: Vec<ChildState>,
    /// `id` attribute values seen in this schema document (must be unique).
    seen_ids: std::collections::HashSet<String>,
    /// Identity constraint names seen (unique/key/keyref share one symbol
    /// space and must be unique schema-wide).
    seen_constraint_names: std::collections::HashSet<String>,
    /// Top-level component names seen (`"space:name"` keys), for duplicate
    /// detection within one document.
    seen_top_level: std::collections::HashSet<String>,
}

/// Child bookkeeping for one open XSD element.
#[derive(Debug)]
pub(super) struct ChildState {
    /// Local name of the element
    pub(super) name: String,
    /// Number of non-annotation children seen so far
    pub(super) children_seen: u32,
    /// Number of annotation children seen so far
    pub(super) annotations: u32,
    /// Number of particle children (group/all/choice/sequence)
    pub(super) particles: u32,
    /// Number of content-derivation children (simpleContent/complexContent
    /// in a complexType, restriction/extension in a content element)
    pub(super) derivations: u32,
    /// Number of anyAttribute children (at most one per scope)
    pub(super) any_attributes: u32,
    /// Number of selector children (identity constraints)
    pub(super) selectors: u32,
    /// Number of field children (identity constraints)
    pub(super) fields: u32,
    /// Whether the element carries a `ref` attribute (references admit no
    /// non-annotation children).
    pub(super) is_ref: bool,
    /// Whether the element carries a `type` attribute (excludes an inline
    /// simpleType/complexType child).
    pub(super) has_type: bool,
    /// Keys of attribute uses / attributeGroup refs seen in this scope, for
    /// duplicate detection.
    pub(super) attr_keys: std::collections::HashSet<String>,
}

impl XsdParser {
    /// Creates a new XSD parser.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            schema: XsdSchema::new(),
            xsd_prefix: None, // Not yet detected
            current_text: String::new(),
            skip_depth: 0,
            default_ns_stack: Vec::new(),
            child_state_stack: Vec::new(),
            seen_ids: std::collections::HashSet::new(),
            seen_constraint_names: std::collections::HashSet::new(),
            seen_top_level: std::collections::HashSet::new(),
        }
    }

    /// Consumes the parser and returns the parsed schema.
    pub fn into_schema(self) -> XsdSchema {
        self.schema
    }

    /// Returns a reference to the parsed schema.
    pub fn schema(&self) -> &XsdSchema {
        &self.schema
    }

    /// Checks if the element name matches an XSD element.
    fn is_xsd_element(&self, _name: &str, prefix: Option<&str>) -> bool {
        // An unprefixed element with an explicit default namespace in scope
        // is judged by that namespace (handles `xmlns="...XMLSchema"`
        // overrides on individual declarations).
        if prefix.is_none() {
            if let Some(Some(uri)) = self.default_ns_stack.last() {
                return uri == helpers::XSD_NAMESPACE;
            }
        }
        match (&self.xsd_prefix, prefix) {
            // XSD prefix detected as "xs" or "xsd", element has same prefix
            (Some(Some(xsd_prefix)), Some(p)) => p == xsd_prefix,
            // XSD uses default namespace (no prefix), element has no prefix
            (Some(None), None) => true,
            // XSD not yet detected - accept elements with no prefix (schema element)
            // This allows the initial schema element to be processed
            (None, None) => true,
            // Mismatched prefixes
            (Some(Some(_)), None) | (Some(None), Some(_)) | (None, Some(_)) => false,
        }
    }

    /// Gets the local name of an XSD element.
    fn xsd_local_name<'a>(&self, name: &'a str) -> &'a str {
        name
    }

    /// Parses attributes from a start element event.
    /// Wrapper for test compatibility.
    pub fn parse_attributes(attrs: &[(&str, &str)]) -> HashMap<String, String> {
        helpers::parse_attributes(attrs)
    }

    /// Parses and validates minOccurs/maxOccurs from attributes.
    /// Wrapper for test compatibility.
    pub fn parse_occurs(attrs: &HashMap<String, String>) -> Result<(Occurs, Occurs)> {
        helpers::parse_occurs(attrs)
    }
}

impl Default for XsdParser {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlEventHandler for XsdParser {
    fn handle(&mut self, event: &RawEvent<'_>) -> Result<()> {
        match event {
            RawEvent::StartElement {
                name,
                prefix,
                attributes,
                namespace_decls,
                ..
            } => {
                let attrs: Vec<(&str, &str)> =
                    attributes.iter().map(|(k, v)| (*k, v.as_ref())).collect();
                self.handle_start(name, *prefix, &attrs, namespace_decls)?;
            }
            RawEvent::EndElement { name, prefix } => {
                self.handle_end(name, *prefix)?;
            }
            RawEvent::Text(text) => {
                self.current_text.push_str(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if !self.stack.is_empty() {
            return Err(
                crate::schema::xsd::error::XsdParseError::UnexpectedEndOfSchema {
                    remaining_frames: self.stack.len(),
                }
                .into(),
            );
        }
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

/// Parses XSD content into an AST.
pub fn parse_xsd_ast(content: &[u8]) -> Result<XsdSchema> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let tracking_reader = PositionTrackingReader::new(content);
    let mut reader = Reader::from_reader(tracking_reader);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;

    let mut xsd_parser = XsdParser::new();
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        let event_result = reader.read_event_into(&mut buf);
        let line = reader.get_ref().line();
        let column = reader.get_ref().column();

        match event_result {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let is_empty = matches!(event_result, Ok(Event::Empty(_)));
                let (name, prefix, attributes, namespace_decls) = split_start_event(e)?;
                xsd_parser.handle(&RawEvent::StartElement {
                    name,
                    prefix,
                    attributes: &attributes,
                    namespace_decls: &namespace_decls,
                    line: Some(line),
                    column: Some(column),
                })?;

                if is_empty {
                    xsd_parser.handle(&RawEvent::EndElement { name, prefix })?;
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let full_name = std::str::from_utf8(qname.as_ref())?;
                let (prefix, name) = crate::namespace::split_qname(full_name);
                xsd_parser.handle(&RawEvent::EndElement { name, prefix })?;
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().map_err(|e| {
                    crate::parser::error::ParseError::TextDecodeError {
                        message: e.to_string(),
                    }
                })?;
                if !text.is_empty() {
                    xsd_parser.handle(&RawEvent::Text(&text))?;
                }
            }
            Ok(Event::Eof) => {
                xsd_parser.handle(&RawEvent::Eof)?;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                return Err(crate::schema::xsd::error::XsdParseError::ParseError {
                    position: reader.buffer_position() as usize,
                    message: e.to_string(),
                }
                .into());
            }
        }
        buf.clear();
    }

    xsd_parser.finish()?;
    Ok(xsd_parser.into_schema())
}
