//! SAX-like event streaming for XML processing.
//!
//! This module provides an event-based interface for processing XML
//! that enables single-pass parsing with optional validation.

use std::any::Any;
use std::io::BufRead;
use std::sync::Arc;

use compact_str::CompactString;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::Result;
use crate::namespace::Namespace;
use crate::position::PositionTrackingReader;

/// String interner for reducing memory allocations.
///
/// Caches frequently used strings (element names, prefixes) to avoid
/// repeated allocations for the same string values.
#[derive(Debug, Default)]
pub(crate) struct StringInterner {
    cache: rustc_hash::FxHashMap<Box<str>, Arc<str>>,
}

impl StringInterner {
    pub(crate) fn new() -> Self {
        Self {
            cache: rustc_hash::FxHashMap::default(),
        }
    }

    /// Interns a string, returning a shared reference.
    ///
    /// If the string is already interned, returns the existing Arc.
    /// Otherwise, creates a new Arc and caches it.
    pub(crate) fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(interned) = self.cache.get(s) {
            Arc::clone(interned)
        } else {
            let arc: Arc<str> = Arc::from(s);
            self.cache.insert(s.into(), Arc::clone(&arc));
            arc
        }
    }

    /// Returns the number of interned strings.
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.cache.len()
    }
}

/// An XML event for streaming processing.
#[derive(Debug, Clone)]
pub enum XmlEvent {
    /// Start of an element
    StartElement {
        /// Local name of the element (interned)
        name: Arc<str>,
        /// Namespace prefix (interned, if any)
        prefix: Option<Arc<str>>,
        /// Namespace URI (if known)
        namespace: Option<String>,
        /// Attributes as (name, value) pairs (using CompactString to avoid heap allocation for short strings)
        attributes: Vec<(CompactString, CompactString)>,
        /// Namespace declarations on this element
        namespace_decls: Vec<Namespace>,
        /// Line number (1-indexed, if available)
        line: Option<usize>,
        /// Column number (1-indexed, in UTF-8 characters, if available)
        column: Option<usize>,
    },
    /// End of an element
    EndElement {
        /// Local name of the element (interned)
        name: Arc<str>,
        /// Namespace prefix (interned, if any)
        prefix: Option<Arc<str>>,
    },
    /// Text content
    Text(String),
    /// CDATA content
    CData(String),
    /// Comment
    Comment(String),
    /// Processing instruction
    ProcessingInstruction {
        /// Target name
        target: String,
        /// Instruction content
        content: Option<String>,
    },
    /// XML declaration
    Declaration {
        /// XML version
        version: Option<String>,
        /// Document encoding
        encoding: Option<String>,
        /// Standalone declaration
        standalone: Option<bool>,
    },
    /// End of document
    Eof,
}

/// A borrowed XML event, valid only for the duration of the handler call.
///
/// This is what the streaming engine produces internally: names, text, and
/// attribute values borrow straight from the parser buffer (or its
/// unescaped copy), so dispatching an event allocates nothing for the
/// common cases. Handlers that need owned events materialize an
/// [`XmlEvent`] via [`RawEvent::to_xml_event`].
#[derive(Debug)]
pub(crate) enum RawEvent<'a> {
    /// Start of an element
    StartElement {
        /// Local name of the element
        name: &'a str,
        /// Namespace prefix (if any)
        prefix: Option<&'a str>,
        /// Attributes (namespace declarations excluded)
        attributes: &'a [(&'a str, std::borrow::Cow<'a, str>)],
        /// Namespace declarations on this element
        namespace_decls: &'a [Namespace],
        /// Line number (1-indexed)
        line: Option<usize>,
        /// Column number (1-indexed)
        column: Option<usize>,
    },
    /// End of an element
    EndElement {
        /// Local name of the element
        name: &'a str,
        /// Namespace prefix (if any)
        prefix: Option<&'a str>,
    },
    /// Text content (unescaped)
    Text(&'a str),
    /// CDATA content
    CData(&'a str),
    /// Comment
    Comment(&'a str),
    /// Processing instruction
    ProcessingInstruction {
        /// Target name
        target: &'a str,
        /// Instruction content
        content: Option<&'a str>,
    },
    /// XML declaration
    Declaration {
        /// XML version
        version: Option<String>,
        /// Document encoding
        encoding: Option<String>,
        /// Standalone declaration
        standalone: Option<bool>,
    },
    /// End of document
    Eof,
}

impl RawEvent<'_> {
    /// Materializes an owned [`XmlEvent`], interning names through
    /// `interner`.
    pub(crate) fn to_xml_event(&self, interner: &mut StringInterner) -> XmlEvent {
        match self {
            RawEvent::StartElement {
                name,
                prefix,
                attributes,
                namespace_decls,
                line,
                column,
            } => XmlEvent::StartElement {
                name: interner.intern(name),
                prefix: prefix.map(|p| interner.intern(p)),
                namespace: None,
                attributes: attributes
                    .iter()
                    .map(|(k, v)| (CompactString::from(*k), CompactString::from(v.as_ref())))
                    .collect(),
                namespace_decls: namespace_decls.to_vec(),
                line: *line,
                column: *column,
            },
            RawEvent::EndElement { name, prefix } => XmlEvent::EndElement {
                name: interner.intern(name),
                prefix: prefix.map(|p| interner.intern(p)),
            },
            RawEvent::Text(t) => XmlEvent::Text(t.to_string()),
            RawEvent::CData(t) => XmlEvent::CData(t.to_string()),
            RawEvent::Comment(t) => XmlEvent::Comment(t.to_string()),
            RawEvent::ProcessingInstruction { target, content } => {
                XmlEvent::ProcessingInstruction {
                    target: target.to_string(),
                    content: content.map(|c| c.to_string()),
                }
            }
            RawEvent::Declaration {
                version,
                encoding,
                standalone,
            } => XmlEvent::Declaration {
                version: version.clone(),
                encoding: encoding.clone(),
                standalone: *standalone,
            },
            RawEvent::Eof => XmlEvent::Eof,
        }
    }
}

/// Trait for handling XML events.
///
/// Implement this trait to process XML events during streaming parsing.
/// Multiple handlers can be attached to a single parser.
///
/// Internal engine API; the public streaming entry point is
/// [`Parser`](crate::Parser).
pub(crate) trait XmlEventHandler: Send + Any {
    /// Called for each XML event. The event borrows from the parser
    /// buffer and is only valid for the duration of the call.
    ///
    /// Return `Ok(())` to continue processing, or an error to stop.
    fn handle(&mut self, event: &RawEvent<'_>) -> Result<()>;

    /// Called when parsing is complete.
    ///
    /// This is called after the final Eof event, allowing handlers
    /// to perform final validation or cleanup.
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    /// Returns self as Any for downcasting.
    fn as_any(self: Box<Self>) -> Box<dyn Any>;
}

/// A streaming XML parser that dispatches events to handlers.
///
/// Internal engine API; the public streaming entry point is
/// [`Parser`](crate::Parser) (`Parser::from(..).events()` /
/// `.for_each_event(..)`).
pub(crate) struct StreamingParser<R: BufRead> {
    reader: Reader<PositionTrackingReader<R>>,
    handlers: Vec<Box<dyn XmlEventHandler>>,
    /// General entities declared in the internal DTD subset
    entities: std::collections::HashMap<String, String>,
}

impl<R: BufRead> StreamingParser<R> {
    /// Creates a new streaming parser from a BufRead source.
    pub fn new(reader: R) -> Self {
        let position_reader = PositionTrackingReader::new(reader);
        let mut xml_reader = Reader::from_reader(position_reader);
        xml_reader.config_mut().trim_text(false);
        xml_reader.config_mut().expand_empty_elements = true;

        Self {
            reader: xml_reader,
            handlers: Vec::new(),
            entities: std::collections::HashMap::new(),
        }
    }

    /// Returns the current line number (1-indexed).
    fn current_line(&self) -> usize {
        self.reader.get_ref().line()
    }

    /// Returns the current column number (1-indexed, in UTF-8 characters).
    fn current_column(&self) -> usize {
        self.reader.get_ref().column()
    }

    /// Adds an event handler.
    pub fn add_handler(&mut self, handler: Box<dyn XmlEventHandler>) {
        self.handlers.push(handler);
    }

    /// Takes ownership of all handlers.
    pub fn into_handlers(self) -> Vec<Box<dyn XmlEventHandler>> {
        self.handlers
    }

    /// Parses the document, dispatching events to all handlers.
    fn drive_loop<F>(&mut self, mut on_event: F) -> Result<()>
    where
        F: FnMut(&RawEvent<'_>) -> Result<()>,
    {
        let mut buffer = Vec::with_capacity(8 * 1024);

        loop {
            let event_result = self.reader.read_event_into(&mut buffer);
            let line = self.current_line();
            let column = self.current_column();

            match event_result {
                Ok(Event::Start(ref e)) => {
                    let (name, prefix, attributes, namespace_decls) =
                        split_start_event(e, &self.entities)?;
                    on_event(&RawEvent::StartElement {
                        name,
                        prefix,
                        attributes: &attributes,
                        namespace_decls: &namespace_decls,
                        line: Some(line),
                        column: Some(column),
                    })?;
                }
                Ok(Event::Empty(ref e)) => {
                    let (name, prefix, attributes, namespace_decls) =
                        split_start_event(e, &self.entities)?;
                    on_event(&RawEvent::StartElement {
                        name,
                        prefix,
                        attributes: &attributes,
                        namespace_decls: &namespace_decls,
                        line: Some(line),
                        column: Some(column),
                    })?;
                    on_event(&RawEvent::EndElement { name, prefix })?;
                }
                Ok(Event::End(ref e)) => {
                    let qname = e.name();
                    let full_name = std::str::from_utf8(qname.as_ref())?;
                    let (prefix, name) = crate::namespace::split_qname(full_name);
                    on_event(&RawEvent::EndElement { name, prefix })?;
                }
                Ok(Event::Text(ref e)) => {
                    let text = e
                        .unescape_with(|name| {
                            self.entities
                                .get(name)
                                .map(String::as_str)
                                .or_else(|| quick_xml::escape::resolve_predefined_entity(name))
                        })
                        .map_err(|e| crate::parser::error::ParseError::TextDecodeError {
                            message: e.to_string(),
                        })?;
                    if !text.is_empty() {
                        on_event(&RawEvent::Text(&text))?;
                    }
                }
                Ok(Event::CData(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    on_event(&RawEvent::CData(text))?;
                }
                Ok(Event::Comment(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    on_event(&RawEvent::Comment(text))?;
                }
                Ok(Event::PI(ref e)) => {
                    let content = std::str::from_utf8(e.as_ref())?;
                    let mut parts = content.splitn(2, char::is_whitespace);
                    let target = parts.next().unwrap_or("");
                    let pi_content = parts.next().map(str::trim);
                    on_event(&RawEvent::ProcessingInstruction {
                        target,
                        content: pi_content,
                    })?;
                }
                Ok(Event::Decl(ref e)) => {
                    let version = e
                        .version()
                        .ok()
                        .map(|v| String::from_utf8_lossy(v.as_ref()).into_owned());
                    let encoding = e
                        .encoding()
                        .and_then(|r| r.ok())
                        .map(|v| String::from_utf8_lossy(v.as_ref()).into_owned());
                    let standalone = e
                        .standalone()
                        .and_then(|r| r.ok())
                        .map(|v| v.as_ref() == b"yes");
                    on_event(&RawEvent::Declaration {
                        version,
                        encoding,
                        standalone,
                    })?;
                }
                Ok(Event::DocType(ref e)) => {
                    // Collect internal-subset general entity declarations
                    if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                        self.entities = crate::parser::entities::parse_internal_entities(text);
                    }
                }
                Ok(Event::Eof) => {
                    on_event(&RawEvent::Eof)?;
                    break;
                }
                Err(e) => {
                    return Err(crate::parser::error::ParseError::AtPosition {
                        position: self.reader.get_ref().byte_offset() as u64,
                        message: e.to_string(),
                    }
                    .into());
                }
            }
            buffer.clear();
        }

        Ok(())
    }

    /// Parses the document, dispatching every event to all registered handlers,
    /// then calling `finish` on each handler.
    pub fn parse(&mut self) -> Result<()> {
        // Move the handlers out so the drive loop's callback can borrow them
        // without conflicting with the `&mut self` the loop needs.
        let mut handlers = std::mem::take(&mut self.handlers);
        let result = self
            .drive_loop(|event| {
                for handler in handlers.iter_mut() {
                    handler.handle(event)?;
                }
                Ok(())
            })
            .and_then(|()| {
                for handler in handlers.iter_mut() {
                    handler.finish()?;
                }
                Ok(())
            });
        self.handlers = handlers;
        result
    }

    /// Drives the parser, invoking `on_event` for every event as it is read.
    ///
    /// Unlike [`parse`](Self::parse), the callback is borrowed only for the
    /// duration of the call, so it may capture and mutate local state (e.g.
    /// accumulate into a `Vec` or counter). Registered handlers are not invoked
    /// by this method.
    pub fn for_each_event<F>(&mut self, mut on_event: F) -> Result<()>
    where
        F: FnMut(&XmlEvent) -> Result<()>,
    {
        let mut interner = StringInterner::new();
        self.drive_loop(|raw| on_event(&raw.to_xml_event(&mut interner)))
    }
}

/// Splits a start tag into name parts, attributes, and namespace
/// declarations, borrowing from the parser buffer wherever possible.
#[allow(clippy::type_complexity)]
fn split_start_event<'a>(
    e: &'a quick_xml::events::BytesStart<'a>,
    entities: &'a std::collections::HashMap<String, String>,
) -> Result<(
    &'a str,
    Option<&'a str>,
    smallvec::SmallVec<[(&'a str, std::borrow::Cow<'a, str>); 8]>,
    smallvec::SmallVec<[Namespace; 2]>,
)> {
    let full_name = std::str::from_utf8(e.name().into_inner())?;
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls: smallvec::SmallVec<[Namespace; 2]> = smallvec::SmallVec::new();
    let mut attributes: smallvec::SmallVec<[(&str, std::borrow::Cow<str>); 8]> =
        smallvec::SmallVec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.into_inner())?;
        let value = attr
            .unescape_value_with(|name| {
                entities
                    .get(name)
                    .map(String::as_str)
                    .or_else(|| quick_xml::escape::resolve_predefined_entity(name))
            })
            .map_err(|e| crate::parser::error::ParseError::AttributeDecodeError {
                message: e.to_string(),
            })?;

        if key == "xmlns" {
            namespace_decls.push(Namespace::default_ns(value.as_ref()));
        } else if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            namespace_decls.push(Namespace::new(ns_prefix, value.as_ref()));
        } else {
            attributes.push((key, value));
        }
    }

    Ok((name, prefix, attributes, namespace_decls))
}

/// A simple handler that collects all events (used by the in-crate tests).
#[cfg(test)]
struct EventCollector {
    events: Vec<XmlEvent>,
    interner: StringInterner,
}

#[cfg(test)]
impl EventCollector {
    /// Creates a new event collector.
    fn new() -> Self {
        Self {
            events: Vec::new(),
            interner: StringInterner::new(),
        }
    }

    /// Takes ownership of the collected events.
    fn into_events(self) -> Vec<XmlEvent> {
        self.events
    }
}

#[cfg(test)]
impl XmlEventHandler for EventCollector {
    fn handle(&mut self, event: &RawEvent<'_>) -> Result<()> {
        self.events.push(event.to_xml_event(&mut self.interner));
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_parser() {
        let xml = r#"<root attr="value"><child>text</child></root>"#;
        let mut parser = StreamingParser::new(xml.as_bytes());

        let collector = EventCollector::new();
        parser.add_handler(Box::new(collector));

        parser.parse().unwrap();

        // Note: we can't access collector after it's been moved into the parser
        // This is a limitation of the current design
    }

    #[test]
    fn test_event_collector() {
        let mut collector = EventCollector::new();

        // Simulate events
        collector
            .handle(&RawEvent::StartElement {
                name: "root",
                prefix: None,
                attributes: &[],
                namespace_decls: &[],
                line: Some(1),
                column: Some(1),
            })
            .unwrap();

        collector
            .handle(&RawEvent::StartElement {
                name: "child",
                prefix: None,
                attributes: &[],
                namespace_decls: &[],
                line: Some(1),
                column: Some(1),
            })
            .unwrap();

        collector
            .handle(&RawEvent::EndElement {
                name: "child",
                prefix: None,
            })
            .unwrap();

        collector
            .handle(&RawEvent::EndElement {
                name: "root",
                prefix: None,
            })
            .unwrap();

        collector.handle(&RawEvent::Eof).unwrap();

        let events = collector.into_events();
        assert_eq!(events.len(), 5);
    }
}
