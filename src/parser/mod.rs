//! XML parsing with quick-xml backend.

pub mod error;

use std::collections::HashMap;
use std::io::BufRead;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::document::{DocumentBuilder, XmlDocument};
use crate::error::Result;
use crate::namespace::{Namespace, split_qname};
use crate::position::PositionTrackingReader;

/// Stack of namespace bindings for tracking scope during parsing.
///
/// Each scope contains a map of prefix -> URI bindings. When entering a new
/// element, a new scope is pushed. When leaving an element, the scope is popped.
#[derive(Debug, Default)]
struct NamespaceStack {
    /// Stack of scopes, each containing prefix -> URI mappings
    scopes: Vec<HashMap<String, String>>,
}

impl NamespaceStack {
    /// Creates a new empty namespace stack.
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()], // Start with one scope
        }
    }

    /// Pushes a new scope onto the stack.
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pops the current scope from the stack.
    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Registers a namespace binding in the current scope.
    fn register(&mut self, prefix: &str, uri: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(prefix.to_string(), uri.to_string());
        }
    }

    /// Resolves a prefix to its namespace URI by searching from current scope up to root.
    fn resolve(&self, prefix: &str) -> Option<&str> {
        // Search from innermost to outermost scope
        for scope in self.scopes.iter().rev() {
            if let Some(uri) = scope.get(prefix) {
                return Some(uri.as_str());
            }
        }
        None
    }
}

/// Parser options for controlling XML parsing behavior.
#[derive(Debug, Clone)]
pub struct ParserOptions {
    /// Buffer size for reading (default: 8KB)
    pub buffer_size: usize,
    /// Maximum memory to use (None = unlimited)
    pub max_memory: Option<usize>,
    /// Whether to trim text content
    pub trim_text: bool,
    /// Whether to expand empty elements
    pub expand_empty_elements: bool,
    /// Whether to check end tags match start tags
    pub check_end_names: bool,
    /// Whether to check comments are valid
    pub check_comments: bool,
}

impl Default for ParserOptions {
    fn default() -> Self {
        Self {
            buffer_size: 8 * 1024, // 8KB
            max_memory: None,
            trim_text: false,
            expand_empty_elements: true,
            check_end_names: true,
            check_comments: true,
        }
    }
}

impl ParserOptions {
    /// Creates options similar to libxml's default parsing.
    pub fn libxml_compat() -> Self {
        Self {
            buffer_size: 8 * 1024,
            max_memory: None,
            trim_text: false,
            expand_empty_elements: true,
            check_end_names: true,
            check_comments: true,
        }
    }
}

/// Parses XML content from a byte slice.
///
/// This is the main entry point for parsing XML.
///
/// # Example
/// ```
/// use fastxml::parse;
///
/// let xml = r#"<root><child>Hello</child></root>"#;
/// let doc = parse(xml).unwrap();
/// let root = doc.get_root_element().unwrap();
/// assert_eq!(root.get_name(), "root");
/// ```
pub fn parse<T: AsRef<[u8]>>(xml: T) -> Result<XmlDocument> {
    parse_with_options(xml, &ParserOptions::default())
}

/// Parses XML content with custom options.
pub fn parse_with_options<T: AsRef<[u8]>>(xml: T, options: &ParserOptions) -> Result<XmlDocument> {
    let tracking_reader = PositionTrackingReader::new(xml.as_ref());
    let mut reader = Reader::from_reader(tracking_reader);
    configure_reader(&mut reader, options);
    parse_from_reader(&mut reader, options)
}

/// Parses XML from a BufRead source.
pub fn parse_from_bufread<R: BufRead>(reader: R, options: &ParserOptions) -> Result<XmlDocument> {
    let tracking_reader = PositionTrackingReader::new(reader);
    let mut xml_reader = Reader::from_reader(tracking_reader);
    configure_reader(&mut xml_reader, options);
    parse_from_reader(&mut xml_reader, options)
}

fn configure_reader<R: BufRead>(reader: &mut Reader<R>, options: &ParserOptions) {
    reader.config_mut().trim_text(options.trim_text);
    reader.config_mut().expand_empty_elements = options.expand_empty_elements;
    reader.config_mut().check_end_names = options.check_end_names;
    reader.config_mut().check_comments = options.check_comments;
}

fn parse_from_reader<R: BufRead>(
    reader: &mut Reader<PositionTrackingReader<R>>,
    options: &ParserOptions,
) -> Result<XmlDocument> {
    let mut builder = DocumentBuilder::new();
    let mut buf = Vec::with_capacity(options.buffer_size);
    let mut memory_used = 0usize;
    let mut ns_stack = NamespaceStack::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                check_memory(options, &mut memory_used, e.len())?;
                let line = reader.get_ref().line();
                let column = reader.get_ref().column();
                process_start_element(&mut builder, e, reader, &mut ns_stack, line, column)?;
            }
            Ok(Event::Empty(ref e)) => {
                check_memory(options, &mut memory_used, e.len())?;
                let line = reader.get_ref().line();
                let column = reader.get_ref().column();
                process_start_element(&mut builder, e, reader, &mut ns_stack, line, column)?;
                ns_stack.pop_scope();
                builder.end_element();
            }
            Ok(Event::End(_)) => {
                ns_stack.pop_scope();
                builder.end_element();
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().map_err(|e| {
                    crate::parser::error::ParseError::TextDecodeError {
                        message: e.to_string(),
                    }
                })?;
                if !text.is_empty() {
                    check_memory(options, &mut memory_used, text.len())?;
                    builder.text(&text);
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = std::str::from_utf8(e.as_ref())?;
                check_memory(options, &mut memory_used, text.len())?;
                builder.cdata(text);
            }
            Ok(Event::Comment(ref e)) => {
                let text = std::str::from_utf8(e.as_ref())?;
                builder.comment(text);
            }
            Ok(Event::PI(ref e)) => {
                let content = std::str::from_utf8(e.as_ref())?;
                // Parse PI: target followed by content
                let parts: Vec<&str> = content.splitn(2, char::is_whitespace).collect();
                let target = parts.first().unwrap_or(&"");
                let pi_content = parts.get(1).map(|s| s.trim());
                builder.processing_instruction(target, pi_content);
            }
            Ok(Event::Decl(ref _e)) => {
                // XML declaration - we could extract version/encoding if needed
            }
            Ok(Event::DocType(_)) => {
                // DOCTYPE - skip for now
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(crate::parser::error::ParseError::AtPosition {
                    position: reader.buffer_position(),
                    message: e.to_string(),
                }
                .into());
            }
        }
        buf.clear();
    }

    Ok(builder.build())
}

fn check_memory(options: &ParserOptions, used: &mut usize, additional: usize) -> Result<()> {
    *used += additional;
    if let Some(max) = options.max_memory
        && *used > max
    {
        return Err(
            crate::parser::error::ParseError::MemoryLimitExceeded { used: *used, max }.into(),
        );
    }
    Ok(())
}

fn process_start_element<R: BufRead>(
    builder: &mut DocumentBuilder,
    e: &BytesStart<'_>,
    _reader: &Reader<PositionTrackingReader<R>>,
    ns_stack: &mut NamespaceStack,
    line: usize,
    column: usize,
) -> Result<()> {
    // Push a new scope for this element
    ns_stack.push_scope();

    let qname_bytes = e.name().as_ref().to_vec();
    let (prefix, local_name) = extract_name_parts(&qname_bytes)?;

    // First pass: collect namespace declarations and register them
    let mut namespace_decls = Vec::new();
    let mut raw_attributes = Vec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let value = attr.unescape_value().map_err(|e| {
            crate::parser::error::ParseError::AttributeDecodeError {
                message: e.to_string(),
            }
        })?;

        if key == "xmlns" {
            // Default namespace declaration
            ns_stack.register("", value.as_ref());
            namespace_decls.push(Namespace::default_ns(value.as_ref()));
        } else if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            // Prefixed namespace declaration
            ns_stack.register(ns_prefix, value.as_ref());
            namespace_decls.push(Namespace::new(ns_prefix, value.as_ref()));
        } else {
            raw_attributes.push((key.to_string(), value.to_string()));
        }
    }

    // Resolve namespace URI for the element
    let namespace_uri = if let Some(p) = prefix {
        ns_stack.resolve(p).map(|s| s.to_string())
    } else {
        // For elements without prefix, use default namespace if declared
        ns_stack.resolve("").map(|s| s.to_string())
    };

    // Second pass: process attributes and resolve their namespaces
    // Store attributes with local names as keys (libxml compatible)
    let mut attributes = Vec::new();
    let mut attr_ns_info = Vec::new();
    for (key, value) in &raw_attributes {
        let (attr_prefix, attr_local_name) = split_qname(key);

        // Only use local name as key (libxml compatible behavior)
        // Attributes with different namespace prefixes but same local name
        // will overwrite each other, but this matches libxml behavior
        if let Some(ap) = attr_prefix {
            // Namespaced attribute: store with local name only
            attributes.push((attr_local_name.to_string(), value.clone()));
            // Resolve and store the namespace URI for this attribute
            if let Some(uri) = ns_stack.resolve(ap) {
                attr_ns_info.push((attr_local_name.to_string(), ap.to_string(), uri.to_string()));
            }
        } else {
            // Non-namespaced attribute
            attributes.push((key.clone(), value.clone()));
        }
    }

    // Convert attributes to borrowed form for builder
    let attr_refs: Vec<(&str, &str)> = attributes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let attr_ns_refs: Vec<(&str, &str, &str)> = attr_ns_info
        .iter()
        .map(|(l, p, u)| (l.as_str(), p.as_str(), u.as_str()))
        .collect();

    builder.start_element(
        local_name,
        prefix,
        namespace_uri.as_deref(),
        attr_refs,
        attr_ns_refs,
        namespace_decls,
        Some(line),
        Some(column),
    );

    Ok(())
}

fn extract_name_parts(qname_bytes: &[u8]) -> Result<(Option<&str>, &str)> {
    let full_name = std::str::from_utf8(qname_bytes)?;
    let (prefix, local_name) = split_qname(full_name);
    Ok((prefix, local_name))
}

/// Parses xsi:schemaLocation from the root element of an XML stream without building a DOM.
///
/// Reads only until the first `Start` or `Empty` element event, extracts the
/// `xsi:schemaLocation` (or `schemaLocation`) attribute, and returns
/// (namespace, location) pairs. This avoids allocating a full DOM tree and is
/// suitable for large XML files where only the schema locations are needed.
pub fn parse_schema_locations_from_reader<R: BufRead>(reader: R) -> Result<Vec<(String, String)>> {
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                for attr in e.attributes().flatten() {
                    let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                    // Match the prefixed form (xsi:schemaLocation) or bare form
                    if key == "xsi:schemaLocation" || key == "schemaLocation" {
                        let value = attr.unescape_value().map_err(|e| {
                            crate::parser::error::ParseError::AttributeDecodeError {
                                message: e.to_string(),
                            }
                        })?;
                        return parse_schema_location_value(&value);
                    }
                }
                // Root element had no schemaLocation attribute
                return Ok(Vec::new());
            }
            Ok(Event::Eof) => return Ok(Vec::new()),
            Ok(_) => {
                // Skip declarations, PIs, comments, etc.
            }
            Err(e) => {
                return Err(crate::parser::error::ParseError::AtPosition {
                    position: xml_reader.buffer_position(),
                    message: e.to_string(),
                }
                .into());
            }
        }
        buf.clear();
    }
}

/// Parses xsi:schemaLocation attribute and returns (namespace, location) pairs.
///
/// The schemaLocation attribute value is a whitespace-separated list of
/// namespace/location pairs.
pub fn parse_schema_locations(doc: &XmlDocument) -> Result<Vec<(String, String)>> {
    let root = doc.get_root_element()?;
    let attrs = root.get_attributes();

    // Look for xsi:schemaLocation
    let schema_location = attrs
        .get("xsi:schemaLocation")
        .or_else(|| attrs.get("schemaLocation"));

    match schema_location {
        Some(value) => parse_schema_location_value(value),
        None => Ok(Vec::new()),
    }
}

/// Parses a schemaLocation attribute value into (namespace, location) pairs.
pub fn parse_schema_location_value(value: &str) -> Result<Vec<(String, String)>> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let mut result = Vec::new();

    for chunk in parts.chunks(2) {
        if chunk.len() == 2 {
            result.push((chunk[0].to_string(), chunk[1].to_string()));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let xml = r#"<root attr="value"><child>text</child></root>"#;
        let doc = parse(xml).unwrap();

        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name(), "root");
        assert_eq!(root.get_attribute("attr"), Some("value".into()));

        let children = root.get_child_elements();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get_name(), "child");
        assert_eq!(children[0].get_content(), Some("text".into()));
    }

    #[test]
    fn test_parse_namespaced() {
        let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:child>text</gml:child>
        </gml:root>"#;
        let doc = parse(xml).unwrap();

        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name(), "root");
        assert_eq!(root.get_prefix(), Some("gml".into()));
        assert_eq!(root.qname(), "gml:root");

        let ns_decls = root.get_namespace_declarations();
        assert_eq!(ns_decls.len(), 1);
        assert_eq!(ns_decls[0].prefix(), "gml");
        assert_eq!(ns_decls[0].uri(), "http://www.opengis.net/gml");
    }

    #[test]
    fn test_parse_cdata() {
        let xml = r#"<root><![CDATA[<not xml>]]></root>"#;
        let doc = parse(xml).unwrap();

        let root = doc.get_root_element().unwrap();
        let children = root.get_child_nodes();
        assert!(!children.is_empty());
        assert_eq!(children[0].get_content(), Some("<not xml>".into()));
    }

    #[test]
    fn test_parse_schema_locations() {
        let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                          xsi:schemaLocation="http://ns1 schema1.xsd http://ns2 schema2.xsd">
        </root>"#;
        let doc = parse(xml).unwrap();
        let locations = parse_schema_locations(&doc).unwrap();

        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0], ("http://ns1".into(), "schema1.xsd".into()));
        assert_eq!(locations[1], ("http://ns2".into(), "schema2.xsd".into()));
    }

    #[test]
    fn test_memory_limit() {
        let xml = "<root>".to_string() + &"x".repeat(1000) + "</root>";
        let options = ParserOptions {
            max_memory: Some(100),
            ..Default::default()
        };

        let result = parse_with_options(&xml, &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_schema_locations_from_reader() {
        let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                          xsi:schemaLocation="http://ns1 schema1.xsd http://ns2 schema2.xsd">
            <child/>
        </root>"#;
        let locations = parse_schema_locations_from_reader(xml.as_bytes()).unwrap();

        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0], ("http://ns1".into(), "schema1.xsd".into()));
        assert_eq!(locations[1], ("http://ns2".into(), "schema2.xsd".into()));
    }

    #[test]
    fn test_parse_schema_locations_from_reader_no_attribute() {
        let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
            <child/>
        </root>"#;
        let locations = parse_schema_locations_from_reader(xml.as_bytes()).unwrap();
        assert!(locations.is_empty());
    }

    #[test]
    fn test_parse_schema_locations_from_reader_empty_element() {
        let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                          xsi:schemaLocation="http://ns1 schema1.xsd" />"#;
        let locations = parse_schema_locations_from_reader(xml.as_bytes()).unwrap();

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0], ("http://ns1".into(), "schema1.xsd".into()));
    }
}
