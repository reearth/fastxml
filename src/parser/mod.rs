//! XML parsing with quick-xml backend.

pub mod error;
mod unified;

pub use unified::Parser;

pub(crate) mod checks;
pub(crate) mod dtd;
pub(crate) mod encoding;
pub(crate) mod entities;
pub(crate) mod eol;
pub(crate) mod expand;
pub(crate) mod source;
pub(crate) mod wellformed;

use std::io::BufRead;
use std::sync::Arc;

use quick_xml::Reader;
use quick_xml::events::Event;
use smallvec::SmallVec;

use crate::document::{DocumentBuilder, ParsedAttr, XmlDocument};
use crate::error::Result;
use crate::namespace::common::XSI_NS;
use crate::namespace::{Namespace, split_qname};
use checks::StartTag;
use expand::{EntityExpander, Segment, TextExpansion};
use source::{EventSource, SliceSource};

/// Namespace bindings in scope while parsing, kept as an undo log.
///
/// `bindings` holds the declarations of every open element, innermost last,
/// and `marks` records where each open element's declarations start, so
/// leaving an element truncates back to its mark. A lookup scans from the end,
/// so the innermost binding of a prefix wins. Documents bind a few dozen
/// prefixes at most, which a backwards scan resolves faster than hashing the
/// prefix once per open scope. URIs are interned by the document builder, so
/// resolving hands out a shared string without another lookup.
#[derive(Debug)]
struct NamespaceStack {
    bindings: Vec<(Box<str>, Arc<str>)>,
    marks: Vec<usize>,
    /// The XML namespace, which the `xml` prefix is always bound to.
    xml_ns: Arc<str>,
    /// Prefixes resolved since the bindings last changed. Elements reuse a
    /// handful of prefixes while the bindings, mostly declared on the root,
    /// rarely change, so this saves scanning past dozens of them per element.
    resolved: rustc_hash::FxHashMap<Box<str>, Option<Arc<str>>>,
}

impl NamespaceStack {
    /// Creates a stack with no bindings.
    fn new() -> Self {
        Self {
            bindings: Vec::new(),
            marks: Vec::new(),
            xml_ns: Arc::from(crate::namespace::common::XML_NS),
            resolved: rustc_hash::FxHashMap::default(),
        }
    }

    /// Opens the scope of a new element.
    fn push_scope(&mut self) {
        self.marks.push(self.bindings.len());
    }

    /// Closes the innermost element's scope, dropping its declarations.
    fn pop_scope(&mut self) {
        if let Some(mark) = self.marks.pop() {
            if mark < self.bindings.len() {
                self.bindings.truncate(mark);
                self.resolved.clear();
            }
        }
    }

    /// Binds `prefix` (`""` for the default namespace) to `uri` in the
    /// innermost scope.
    fn register(&mut self, prefix: &str, uri: Arc<str>) {
        self.bindings.push((Box::from(prefix), uri));
        self.resolved.clear();
    }

    /// Resolves a prefix to its namespace URI, innermost binding first. The
    /// `xml` prefix is always bound to the XML namespace.
    fn resolve(&mut self, prefix: &str) -> Option<Arc<str>> {
        if prefix == "xml" {
            return Some(self.xml_ns.clone());
        }
        if let Some(uri) = self.resolved.get(prefix) {
            return uri.clone();
        }
        let uri = self
            .bindings
            .iter()
            .rev()
            .find(|(p, _)| &**p == prefix)
            .map(|(_, uri)| uri.clone());
        self.resolved.insert(Box::from(prefix), uri.clone());
        uri
    }
}

/// Parser options for controlling XML parsing behavior.
#[derive(Debug, Clone)]
pub struct ParserOptions {
    /// Initial capacity of the DOM parser's event buffer (default: 8KB).
    /// The buffer grows as needed; this does not limit or set how much is
    /// read at a time, and the streaming parser does not use it.
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
/// Internal helper; the public entry point is [`Parser`](crate::Parser)
/// (`Parser::from(xml).parse()`).
pub(crate) fn parse<T: AsRef<[u8]>>(xml: T) -> Result<XmlDocument> {
    parse_with_options(xml, &ParserOptions::default())
}

/// Parses XML content with custom options.
pub(crate) fn parse_with_options<T: AsRef<[u8]>>(
    xml: T,
    options: &ParserOptions,
) -> Result<XmlDocument> {
    let xml = encoding::to_utf8(xml.as_ref());
    let xml = source::normalize_line_ends(xml)?;
    // Markup-dense XML averages a node per few dozen bytes (CityGML DEM: one
    // per ~31); reserving for one per 32 bytes saves most regrowth copies,
    // and capacity a text-heavy document leaves unused is never touched.
    let builder = DocumentBuilder::with_node_capacity(xml.len() / 32);
    let mut source = SliceSource::new(&xml);
    configure_reader(source.reader_mut(), options);
    parse_from_source(source, options, builder)
}

/// Parses XML from a BufRead source.
pub(crate) fn parse_from_bufread<R: BufRead>(
    reader: R,
    options: &ParserOptions,
) -> Result<XmlDocument> {
    let mut reader = source::reader_source(reader);
    configure_reader(&mut reader, options);
    parse_from_source(reader, options, DocumentBuilder::new())
}

fn configure_reader<R>(reader: &mut Reader<R>, options: &ParserOptions) {
    reader.config_mut().trim_text(options.trim_text);
    reader.config_mut().expand_empty_elements = options.expand_empty_elements;
    reader.config_mut().check_end_names = options.check_end_names;
    reader.config_mut().check_comments = options.check_comments;
}

fn parse_from_source<'i>(
    mut reader: impl EventSource<'i>,
    options: &ParserOptions,
    builder: DocumentBuilder,
) -> Result<XmlDocument> {
    let mut dom = DomBuilder {
        options,
        builder,
        memory_used: 0,
        ns_stack: NamespaceStack::new(),
        expander: EntityExpander::new(),
        checker: checks::WellformedChecker::new(),
    };
    let mut buf = Vec::with_capacity(options.buffer_size);

    loop {
        let event = reader.next_event(&mut buf);
        // Only elements record a position: start tags, and the elements an
        // entity reference in text expands to.
        let (line, column) = match &event {
            Ok(Event::Start(_) | Event::Empty(_)) => reader.line_column(),
            Ok(Event::Text(t)) if memchr::memchr(b'&', t).is_some() => reader.line_column(),
            _ => (0, 0),
        };
        match event {
            Ok(Event::Decl(ref e)) => {
                dom.checker.decl(std::str::from_utf8(e.as_ref())?)?;
            }
            Ok(Event::DocType(ref e)) => {
                // Collect internal-subset general entity declarations so
                // entity references in content/attributes resolve.
                if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                    dom.checker.doctype(text)?;
                    dom.expander.declare_from(&mut dom.checker);
                }
            }
            Ok(Event::Eof) => {
                dom.checker.eof()?;
                break;
            }
            Ok(ref e) => dom.event(e, line, column)?,
            Err(e) => {
                return Err(crate::parser::error::ParseError::AtPosition {
                    position: reader.error_position(),
                    message: e.to_string(),
                }
                .into());
            }
        }
        buf.clear();
    }

    Ok(dom.builder.build())
}

/// State of one DOM parse: the document under construction plus the
/// namespace, entity and well-formedness bookkeeping shared by the main
/// event loop and the content fragments of entities that contain markup.
struct DomBuilder<'o> {
    options: &'o ParserOptions,
    builder: DocumentBuilder,
    memory_used: usize,
    ns_stack: NamespaceStack,
    expander: EntityExpander,
    checker: checks::WellformedChecker,
}

impl DomBuilder<'_> {
    /// Handles one content event (anything but the XML declaration, the
    /// DOCTYPE and end of input). `line`/`column` locate new elements.
    fn event(&mut self, event: &Event<'_>, line: usize, column: usize) -> Result<()> {
        match event {
            Event::Start(e) => {
                check_memory(self.options, &mut self.memory_used, e.len())?;
                let tag = StartTag::decode(e)?;
                self.checker.start(&tag)?;
                self.start_element(&tag, line, column)?;
            }
            Event::Empty(e) => {
                check_memory(self.options, &mut self.memory_used, e.len())?;
                let tag = StartTag::decode(e)?;
                self.checker.start(&tag)?;
                self.start_element(&tag, line, column)?;
                // An empty-element tag opens and immediately closes.
                self.checker.end(tag.qname)?;
                self.ns_stack.pop_scope();
                self.builder.end_element();
            }
            Event::End(e) => {
                self.checker
                    .end(std::str::from_utf8(e.name().into_inner())?)?;
                self.ns_stack.pop_scope();
                self.builder.end_element();
            }
            Event::Text(e) => {
                // Check the raw (pre-expansion) text: literal characters must
                // satisfy the Char production, and each character reference
                // must name a legal XML 1.0 character.
                let raw = std::str::from_utf8(e.as_ref())?;
                let raw = self.checker.text(raw)?;
                self.expander.declare_from(&mut self.checker);
                match self.expander.expand_text(raw)? {
                    TextExpansion::Text(text) => self.text(&text)?,
                    TextExpansion::Segments(segments) => {
                        for segment in segments {
                            match segment {
                                Segment::Text(text) => self.text(&text)?,
                                Segment::Markup(name) => self.fragment(&name, line, column)?,
                            }
                        }
                    }
                }
            }
            Event::CData(e) => {
                let text = std::str::from_utf8(e.as_ref())?;
                self.checker.cdata(text)?;
                check_memory(self.options, &mut self.memory_used, text.len())?;
                self.builder.cdata(text);
            }
            Event::Comment(e) => {
                let text = std::str::from_utf8(e.as_ref())?;
                self.checker.comment(text)?;
                self.builder.comment(text);
            }
            Event::PI(e) => {
                let content = std::str::from_utf8(e.as_ref())?;
                self.checker.pi(content)?;
                // Parse PI: target followed by content
                let parts: Vec<&str> = content.splitn(2, char::is_whitespace).collect();
                let target = parts.first().unwrap_or(&"");
                let pi_content = parts.get(1).map(|s| s.trim());
                self.builder.processing_instruction(target, pi_content);
            }
            Event::Decl(_) | Event::DocType(_) | Event::Eof => {
                return Err(not_in_content(event).into());
            }
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<()> {
        if !text.is_empty() {
            check_memory(self.options, &mut self.memory_used, text.len())?;
            self.builder.text(text);
        }
        Ok(())
    }

    /// Parses the replacement text of entity `name` as content in place of
    /// its reference (XML 1.0 §4.4.2).
    fn fragment(&mut self, name: &str, line: usize, column: usize) -> Result<()> {
        let replacement = self.expander.enter(name)?;
        let depth = self.checker.depth();
        let mut reader = expand::fragment_reader(&replacement);
        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(ref e) => self.event(e, line, column)?,
                Err(e) => return Err(expand::fragment_error(name, e).into()),
            }
        }
        expand::check_balanced(name, depth, self.checker.depth())?;
        self.expander.leave();
        Ok(())
    }

    fn start_element(&mut self, tag: &StartTag<'_>, line: usize, column: usize) -> Result<()> {
        // Push a new scope for this element
        self.ns_stack.push_scope();

        let qname = tag.qname;
        let (prefix, _) = split_qname(qname);
        // Name and character well-formedness were validated by
        // `WellformedChecker::start` before this function ran.

        // First pass: collect namespace declarations and register them
        let mut namespace_decls = Vec::new();
        let mut raw_attributes: SmallVec<[(&str, String); 8]> = SmallVec::new();

        for (key, raw) in &tag.attrs {
            let key: &str = key;
            let value = self.expander.expand_attr(raw)?.into_owned();

            if key == "xmlns" {
                // Default namespace declaration
                self.ns_stack.register("", self.builder.intern(&value));
                namespace_decls.push(Namespace::default_ns(value));
            } else if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
                // Prefixed namespace declaration
                self.ns_stack
                    .register(ns_prefix, self.builder.intern(&value));
                namespace_decls.push(Namespace::new(ns_prefix, value));
            } else {
                raw_attributes.push((key, value));
            }
        }

        // Resolve namespace URI for the element; unprefixed elements take
        // the default namespace if one is declared.
        let namespace_uri = self.ns_stack.resolve(prefix.unwrap_or(""));

        // Second pass: split each attribute name and resolve its prefix.
        // Every attribute is kept with its prefix and namespace, including
        // ones that share a local name with another attribute.
        let attributes: SmallVec<[ParsedAttr<'_>; 8]> = raw_attributes
            .iter()
            .map(|(key, value)| {
                let (attr_prefix, local) = split_qname(key);
                ParsedAttr {
                    prefix: attr_prefix,
                    local,
                    ns_uri: attr_prefix.and_then(|p| self.ns_stack.resolve(p)),
                    value,
                }
            })
            .collect();

        let (name, prefix) = self.builder.intern_qname(qname);
        self.builder.start_element_resolved(
            name,
            prefix,
            namespace_uri,
            &attributes,
            namespace_decls,
            Some(line),
            Some(column),
        );

        Ok(())
    }
}

/// Error for a document-level event inside content (only possible in the
/// replacement text of an entity).
fn not_in_content(event: &Event<'_>) -> crate::parser::error::ParseError {
    let what = match event {
        Event::Decl(_) => "an XML declaration",
        Event::DocType(_) => "a DOCTYPE declaration",
        _ => "end of input",
    };
    crate::parser::error::ParseError::NotWellFormed {
        message: format!("{what} is not allowed in entity replacement text"),
    }
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

/// Parses xsi:schemaLocation from the root element of an XML stream without building a DOM.
///
/// Reads only until the first `Start` or `Empty` element event, extracts the
/// `schemaLocation` attribute in the XML Schema instance namespace (under
/// whichever prefix the root element binds to it), and returns
/// (namespace, location) pairs. This avoids allocating a full DOM tree and is
/// suitable for large XML files where only the schema locations are needed.
pub fn parse_schema_locations_from_reader<R: BufRead>(reader: R) -> Result<Vec<(String, String)>> {
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                // The attribute is `schemaLocation` in the XSI namespace,
                // under whatever prefix the root element binds to it.
                let attrs: Vec<_> = e.attributes().flatten().collect();
                let xsi_prefixes: Vec<&str> = attrs
                    .iter()
                    .filter(|a| a.value.as_ref() == XSI_NS.as_bytes())
                    .filter_map(|a| std::str::from_utf8(a.key.as_ref()).ok())
                    .filter_map(|k| k.strip_prefix("xmlns:"))
                    .collect();
                for attr in &attrs {
                    let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                    let is_xsi = key
                        .strip_suffix(":schemaLocation")
                        .is_some_and(|p| xsi_prefixes.contains(&p));
                    if is_xsi {
                        let value = attr.unescape_value().map_err(|e| {
                            crate::parser::error::ParseError::AttributeDecodeError {
                                message: e.to_string(),
                            }
                        })?;
                        return parse_schema_location_value(&value);
                    }
                }
                // Root element had no xsi:schemaLocation attribute
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

/// Parses the root element's `xsi:schemaLocation` attribute (matched by the
/// XML Schema instance namespace, not by prefix) and returns
/// (namespace, location) pairs.
///
/// The schemaLocation attribute value is a whitespace-separated list of
/// namespace/location pairs.
pub fn parse_schema_locations(doc: &XmlDocument) -> Result<Vec<(String, String)>> {
    let root = doc.get_root_element()?;
    match root.get_attribute_ns("schemaLocation", XSI_NS) {
        Some(value) => parse_schema_location_value(&value),
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

    /// Every node of `doc` (type, names, content, attributes, links and
    /// position) as text, for comparing two parses of one input.
    fn dump(doc: &XmlDocument) -> Vec<String> {
        doc.nodes.read().iter().map(|n| format!("{n:?}")).collect()
    }

    /// Parses `xml` from a byte slice and from readers with small buffers.
    /// The two input paths must build the same document, positions included,
    /// or fail with the same error.
    fn assert_paths_agree(xml: &[u8]) {
        let from_slice = parse(xml).map(|d| dump(&d)).map_err(|e| e.to_string());
        // From 3 bytes up: a reader whose first fill returns fewer bytes than
        // a UTF-8 BOM hides the BOM from quick-xml (a separate issue).
        for capacity in [3, 4, 7, 64, 8192] {
            let reader = std::io::BufReader::with_capacity(capacity, xml);
            let from_reader = parse_from_bufread(reader, &ParserOptions::default())
                .map(|d| dump(&d))
                .map_err(|e| e.to_string());
            assert_eq!(
                from_slice,
                from_reader,
                "input {:?}, reader capacity {capacity}",
                String::from_utf8_lossy(xml)
            );
        }
    }

    #[test]
    fn slice_and_reader_inputs_build_the_same_document() {
        let docs: &[&[u8]] = &[
            b"<root a='1'>text<child/>tail</root>",
            b"<?xml version='1.0'?>\n<root>\n  <a x=\"1\" y='2'>v</a>\n</root>\n",
            b"\xEF\xBB\xBF<root>\n<a/></root>",
            b"<root>\r\n  <a>x\ry</a>\r\n<b>\r</b></root>\r\n",
            "<r>日本語\n<名前 属性='値'>ü</名前>\n<e/></r>".as_bytes(),
            b"<!DOCTYPE r [<!ENTITY e '<i>in</i>'><!ENTITY t 'tt'>]>\n<r>&e;&t;\n<a v='&t;'/></r>",
            b"<r xmlns='urn:d' xmlns:p='urn:p'><p:a p:x='1'><b/></p:a></r>",
            b"<r><!-- c --><?pi data?><![CDATA[<x>]]>&amp;&#x41;</r>",
            b"<r>\n\t\t<a>1 2 3</a>\n\t\t<a>4 5 6</a>\n</r>",
            // Not well-formed: the errors must match too.
            b"<r><a></r>",
            b"<r>\n<a x='1' x='2'/></r>",
            b"<r>\n\n  <a>&undefined;</a></r>",
            b"<r>\x01</r>",
        ];
        for doc in docs {
            assert_paths_agree(doc);
            assert_event_paths_agree(doc);
        }
    }

    /// The streaming counterpart of [`assert_paths_agree`]: the same events,
    /// positions included, or the same error.
    fn assert_event_paths_agree(xml: &[u8]) {
        let dump = |events: Result<Vec<crate::event::XmlEvent>>| {
            events
                .map(|e| e.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>())
                .map_err(|e| e.to_string())
        };
        let from_slice = dump(crate::Parser::from(xml).events());
        for capacity in [3, 4, 7, 64, 8192] {
            let reader = std::io::BufReader::with_capacity(capacity, xml);
            let from_reader = dump(crate::Parser::from_reader(reader).events());
            assert_eq!(
                from_slice,
                from_reader,
                "events of {:?}, reader capacity {capacity}",
                String::from_utf8_lossy(xml)
            );
        }
    }

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
