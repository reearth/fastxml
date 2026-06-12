//! SAX-like event streaming for XML processing.
//!
//! This module provides an event-based interface for processing XML
//! that enables single-pass parsing with optional validation.

use std::any::Any;
use std::collections::HashMap;
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
struct StringInterner {
    cache: rustc_hash::FxHashMap<Box<str>, Arc<str>>,
}

impl StringInterner {
    fn new() -> Self {
        Self {
            cache: rustc_hash::FxHashMap::default(),
        }
    }

    /// Interns a string, returning a shared reference.
    ///
    /// If the string is already interned, returns the existing Arc.
    /// Otherwise, creates a new Arc and caches it.
    fn intern(&mut self, s: &str) -> Arc<str> {
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

/// Trait for handling XML events.
///
/// Implement this trait to process XML events during streaming parsing.
/// Multiple handlers can be attached to a single parser.
///
/// Internal engine API; the public streaming entry point is
/// [`Parser`](crate::Parser).
pub(crate) trait XmlEventHandler: Send + Any {
    /// Called for each XML event.
    ///
    /// Return `Ok(())` to continue processing, or an error to stop.
    fn handle(&mut self, event: &XmlEvent) -> Result<()>;

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
    interner: StringInterner,
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
            interner: StringInterner::new(),
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
        F: FnMut(&XmlEvent) -> Result<()>,
    {
        let mut buffer = Vec::with_capacity(8 * 1024);

        loop {
            let event_result = self.reader.read_event_into(&mut buffer);
            let line = self.current_line();
            let column = self.current_column();

            match event_result {
                Ok(Event::Start(ref e)) => {
                    let event =
                        convert_start_event(e, line, column, &mut self.interner, &self.entities)?;
                    on_event(&event)?;
                }
                Ok(Event::Empty(ref e)) => {
                    let start_event =
                        convert_start_event(e, line, column, &mut self.interner, &self.entities)?;
                    on_event(&start_event)?;

                    // For empty elements, also dispatch end event
                    if let XmlEvent::StartElement {
                        ref name,
                        ref prefix,
                        ..
                    } = start_event
                    {
                        let end_event = XmlEvent::EndElement {
                            name: name.clone(),
                            prefix: prefix.clone(),
                        };
                        on_event(&end_event)?;
                    }
                }
                Ok(Event::End(ref e)) => {
                    let qname = e.name();
                    let full_name = std::str::from_utf8(qname.as_ref())?;
                    let (prefix, name) = crate::namespace::split_qname(full_name);
                    let event = XmlEvent::EndElement {
                        name: self.interner.intern(name),
                        prefix: prefix.map(|p| self.interner.intern(p)),
                    };
                    on_event(&event)?;
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
                        let event = XmlEvent::Text(text.into_owned());
                        on_event(&event)?;
                    }
                }
                Ok(Event::CData(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    let event = XmlEvent::CData(text.to_string());
                    on_event(&event)?;
                }
                Ok(Event::Comment(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    let event = XmlEvent::Comment(text.to_string());
                    on_event(&event)?;
                }
                Ok(Event::PI(ref e)) => {
                    let content = std::str::from_utf8(e.as_ref())?;
                    let parts: Vec<&str> = content.splitn(2, char::is_whitespace).collect();
                    let target = parts.first().unwrap_or(&"").to_string();
                    let pi_content = parts.get(1).map(|s| s.trim().to_string());
                    let event = XmlEvent::ProcessingInstruction {
                        target,
                        content: pi_content,
                    };
                    on_event(&event)?;
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
                    let event = XmlEvent::Declaration {
                        version,
                        encoding,
                        standalone,
                    };
                    on_event(&event)?;
                }
                Ok(Event::DocType(ref e)) => {
                    // Collect internal-subset general entity declarations
                    if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                        self.entities = crate::parser::entities::parse_internal_entities(text);
                    }
                }
                Ok(Event::Eof) => {
                    let event = XmlEvent::Eof;
                    on_event(&event)?;
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
    pub fn for_each_event<F>(&mut self, on_event: F) -> Result<()>
    where
        F: FnMut(&XmlEvent) -> Result<()>,
    {
        self.drive_loop(on_event)
    }
}

fn convert_start_event(
    e: &quick_xml::events::BytesStart<'_>,
    line: usize,
    column: usize,
    interner: &mut StringInterner,
    entities: &std::collections::HashMap<String, String>,
) -> Result<XmlEvent> {
    let qname = e.name();
    let full_name = std::str::from_utf8(qname.as_ref())?;
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls = Vec::new();
    let mut attributes = Vec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
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
            attributes.push((
                CompactString::from(key),
                CompactString::from(value.as_ref()),
            ));
        }
    }

    Ok(XmlEvent::StartElement {
        name: interner.intern(name),
        prefix: prefix.map(|p| interner.intern(p)),
        namespace: None, // Would need namespace resolution
        attributes,
        namespace_decls,
        line: Some(line),
        column: Some(column),
    })
}

/// A simple handler that collects all events (used by the in-crate tests).
#[cfg(test)]
struct EventCollector {
    events: Vec<XmlEvent>,
}

#[cfg(test)]
impl EventCollector {
    /// Creates a new event collector.
    fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Takes ownership of the collected events.
    fn into_events(self) -> Vec<XmlEvent> {
        self.events
    }
}

#[cfg(test)]
impl XmlEventHandler for EventCollector {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        self.events.push(event.clone());
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
            .handle(&XmlEvent::StartElement {
                name: Arc::from("root"),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
                column: Some(1),
            })
            .unwrap();

        collector
            .handle(&XmlEvent::StartElement {
                name: Arc::from("child"),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
                column: Some(1),
            })
            .unwrap();

        collector
            .handle(&XmlEvent::EndElement {
                name: Arc::from("child"),
                prefix: None,
            })
            .unwrap();

        collector
            .handle(&XmlEvent::EndElement {
                name: Arc::from("root"),
                prefix: None,
            })
            .unwrap();

        collector.handle(&XmlEvent::Eof).unwrap();

        let events = collector.into_events();
        assert_eq!(events.len(), 5);
    }
}
