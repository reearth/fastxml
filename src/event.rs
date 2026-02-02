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

/// String interner for reducing memory allocations.
///
/// Caches frequently used strings (element names, prefixes) to avoid
/// repeated allocations for the same string values.
#[derive(Debug, Default)]
struct StringInterner {
    cache: HashMap<Box<str>, Arc<str>>,
}

impl StringInterner {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
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
        /// Line number (if available)
        line: Option<usize>,
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
pub trait XmlEventHandler: Send + Any {
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
pub struct StreamingParser<R: BufRead> {
    reader: Reader<R>,
    handlers: Vec<Box<dyn XmlEventHandler>>,
    interner: StringInterner,
}

impl<R: BufRead> StreamingParser<R> {
    /// Creates a new streaming parser from a BufRead source.
    pub fn new(reader: R) -> Self {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(false);
        xml_reader.config_mut().expand_empty_elements = true;

        Self {
            reader: xml_reader,
            handlers: Vec::new(),
            interner: StringInterner::new(),
        }
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
    pub fn parse(&mut self) -> Result<()> {
        let mut buffer = Vec::with_capacity(8 * 1024);

        loop {
            let event_result = self.reader.read_event_into(&mut buffer);
            let position = self.reader.buffer_position();

            match event_result {
                Ok(Event::Start(ref e)) => {
                    let event = convert_start_event(e, position, &mut self.interner)?;
                    self.dispatch_event(&event)?;
                }
                Ok(Event::Empty(ref e)) => {
                    let start_event = convert_start_event(e, position, &mut self.interner)?;
                    self.dispatch_event(&start_event)?;

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
                        self.dispatch_event(&end_event)?;
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name_bytes = e.name().as_ref().to_vec();
                    let full_name = std::str::from_utf8(&name_bytes)?;
                    let (prefix, name) = crate::namespace::split_qname(full_name);
                    let event = XmlEvent::EndElement {
                        name: self.interner.intern(name),
                        prefix: prefix.map(|p| self.interner.intern(p)),
                    };
                    self.dispatch_event(&event)?;
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().map_err(|e| {
                        crate::parse_error::ParseError::TextDecodeError {
                            message: e.to_string(),
                        }
                    })?;
                    if !text.is_empty() {
                        let event = XmlEvent::Text(text.into_owned());
                        self.dispatch_event(&event)?;
                    }
                }
                Ok(Event::CData(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    let event = XmlEvent::CData(text.to_string());
                    self.dispatch_event(&event)?;
                }
                Ok(Event::Comment(ref e)) => {
                    let text = std::str::from_utf8(e.as_ref())?;
                    let event = XmlEvent::Comment(text.to_string());
                    self.dispatch_event(&event)?;
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
                    self.dispatch_event(&event)?;
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
                    self.dispatch_event(&event)?;
                }
                Ok(Event::DocType(_)) => {
                    // Skip DOCTYPE
                }
                Ok(Event::Eof) => {
                    let event = XmlEvent::Eof;
                    self.dispatch_event(&event)?;
                    break;
                }
                Err(e) => {
                    return Err(crate::parse_error::ParseError::AtPosition {
                        position,
                        message: e.to_string(),
                    }
                    .into());
                }
            }
            buffer.clear();
        }

        // Call finish on all handlers
        for handler in &mut self.handlers {
            handler.finish()?;
        }

        Ok(())
    }

    fn dispatch_event(&mut self, event: &XmlEvent) -> Result<()> {
        for handler in &mut self.handlers {
            handler.handle(event)?;
        }
        Ok(())
    }
}

fn convert_start_event(
    e: &quick_xml::events::BytesStart<'_>,
    position: u64,
    interner: &mut StringInterner,
) -> Result<XmlEvent> {
    let name_bytes = e.name().as_ref().to_vec();
    let full_name = std::str::from_utf8(&name_bytes)?;
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls = Vec::new();
    let mut attributes = Vec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let value = attr.unescape_value().map_err(|e| {
            crate::parse_error::ParseError::AttributeDecodeError {
                message: e.to_string(),
            }
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

    let line = Some(position as usize);

    Ok(XmlEvent::StartElement {
        name: interner.intern(name),
        prefix: prefix.map(|p| interner.intern(p)),
        namespace: None, // Would need namespace resolution
        attributes,
        namespace_decls,
        line,
    })
}

/// A simple handler that collects all events.
pub struct EventCollector {
    events: Vec<XmlEvent>,
}

impl EventCollector {
    /// Creates a new event collector.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Returns the collected events.
    pub fn events(&self) -> &[XmlEvent] {
        &self.events
    }

    /// Takes ownership of the collected events.
    pub fn into_events(self) -> Vec<XmlEvent> {
        self.events
    }
}

impl Default for EventCollector {
    fn default() -> Self {
        Self::new()
    }
}

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
