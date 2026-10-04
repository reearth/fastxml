//! SAX-like event streaming for XML processing.
//!
//! [`XmlEvent`] is the event type delivered by the public streaming entry
//! points, [`Parser::events`](crate::Parser::events) and
//! [`Parser::for_each_event`](crate::Parser::for_each_event). The streaming
//! engine behind them (also used by the single-pass validator) is internal
//! to the crate.

use std::any::Any;
use std::io::BufRead;
use std::sync::Arc;

use compact_str::CompactString;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::Result;
use crate::namespace::Namespace;
use crate::parser::eol::EolNormalizer;
use crate::parser::expand::{self, EntityExpander, Segment, TextExpansion};
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

/// Receiver of borrowed events inside the streaming engine; several can be
/// attached to one [`StreamingParser`] (e.g. a document builder and the
/// single-pass validator sharing one parse).
///
/// Internal engine API, not implementable outside the crate; consumers use
/// [`Parser::for_each_event`](crate::Parser::for_each_event).
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
    reader: Reader<PositionTrackingReader<EolNormalizer<R>>>,
    handlers: Vec<Box<dyn XmlEventHandler>>,
    /// General entities declared in the internal DTD subset
    expander: EntityExpander,
}

impl<R: BufRead> StreamingParser<R> {
    /// Creates a new streaming parser from a BufRead source.
    pub fn new(reader: R) -> Self {
        let position_reader = PositionTrackingReader::new(EolNormalizer::new(reader));
        let mut xml_reader = Reader::from_reader(position_reader);
        xml_reader.config_mut().trim_text(false);
        xml_reader.config_mut().expand_empty_elements = true;

        Self {
            reader: xml_reader,
            handlers: Vec::new(),
            expander: EntityExpander::new(),
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
    fn drive_loop<F>(&mut self, mut on_event: F) -> Result<()>
    where
        F: FnMut(&RawEvent<'_>) -> Result<()>,
    {
        let mut buffer = Vec::with_capacity(8 * 1024);
        let Self {
            reader, expander, ..
        } = self;
        let mut sink = StreamSink {
            checker: crate::parser::checks::WellformedChecker::new(),
            expander,
            on_event: &mut on_event,
        };

        loop {
            let event_result = reader.read_event_into(&mut buffer);
            let position = reader.get_ref();
            let (line, column) = (position.line(), position.column());

            match event_result {
                Ok(Event::Decl(ref e)) => {
                    sink.checker.decl(std::str::from_utf8(e.as_ref())?)?;
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
                    (sink.on_event)(&RawEvent::Declaration {
                        version,
                        encoding,
                        standalone,
                    })?;
                }
                Ok(Event::DocType(ref e)) => {
                    // Collect internal-subset general entity declarations
                    if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                        sink.checker.doctype(text)?;
                        sink.expander.declare_from(&mut sink.checker);
                    }
                }
                Ok(Event::Eof) => {
                    sink.checker.eof()?;
                    (sink.on_event)(&RawEvent::Eof)?;
                    break;
                }
                Ok(ref e) => sink.event(e, line, column)?,
                Err(e) => {
                    return Err(crate::parser::error::ParseError::AtPosition {
                        position: reader.get_ref().byte_offset() as u64,
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

/// Per-parse state of the streaming loop: well-formedness and entity
/// bookkeeping plus the event callback, shared by the main loop and the
/// content fragments of entities that contain markup.
struct StreamSink<'s, F> {
    checker: crate::parser::checks::WellformedChecker,
    expander: &'s mut EntityExpander,
    on_event: &'s mut F,
}

impl<F> StreamSink<'_, F>
where
    F: FnMut(&RawEvent<'_>) -> Result<()>,
{
    /// Handles one content event (anything but the XML declaration, the
    /// DOCTYPE and end of input).
    fn event(&mut self, event: &Event<'_>, line: usize, column: usize) -> Result<()> {
        match event {
            Event::Start(e) | Event::Empty(e) => {
                self.checker.start(e)?;
                let (name, prefix, attributes, namespace_decls) =
                    split_start_event(e, self.expander)?;
                (self.on_event)(&RawEvent::StartElement {
                    name,
                    prefix,
                    attributes: &attributes,
                    namespace_decls: &namespace_decls,
                    line: Some(line),
                    column: Some(column),
                })?;
                if matches!(event, Event::Empty(_)) {
                    // An empty-element tag opens and immediately closes.
                    self.checker
                        .end(std::str::from_utf8(e.name().into_inner())?)?;
                    (self.on_event)(&RawEvent::EndElement { name, prefix })?;
                }
            }
            Event::End(e) => {
                let qname = e.name();
                let full_name = std::str::from_utf8(qname.as_ref())?;
                self.checker.end(full_name)?;
                let (prefix, name) = crate::namespace::split_qname(full_name);
                (self.on_event)(&RawEvent::EndElement { name, prefix })?;
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
                (self.on_event)(&RawEvent::CData(text))?;
            }
            Event::Comment(e) => {
                let text = std::str::from_utf8(e.as_ref())?;
                self.checker.comment(text)?;
                (self.on_event)(&RawEvent::Comment(text))?;
            }
            Event::PI(e) => {
                let content = std::str::from_utf8(e.as_ref())?;
                self.checker.pi(content)?;
                let mut parts = content.splitn(2, char::is_whitespace);
                let target = parts.next().unwrap_or("");
                let pi_content = parts.next().map(str::trim);
                (self.on_event)(&RawEvent::ProcessingInstruction {
                    target,
                    content: pi_content,
                })?;
            }
            Event::Decl(_) | Event::DocType(_) | Event::Eof => {
                return Err(crate::parser::error::ParseError::NotWellFormed {
                    message: "a declaration is not allowed in entity replacement text".to_string(),
                }
                .into());
            }
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<()> {
        if !text.is_empty() {
            (self.on_event)(&RawEvent::Text(text))?;
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
}

/// Splits a start tag into name parts, attributes, and namespace
/// declarations, borrowing from the parser buffer wherever possible.
/// Attribute values are expanded and normalized (XML 1.0 §3.3.3).
#[allow(clippy::type_complexity)]
fn split_start_event<'a>(
    e: &'a quick_xml::events::BytesStart<'a>,
    expander: &mut EntityExpander,
) -> Result<(
    &'a str,
    Option<&'a str>,
    smallvec::SmallVec<[(&'a str, std::borrow::Cow<'a, str>); 8]>,
    smallvec::SmallVec<[Namespace; 2]>,
)> {
    let full_name = std::str::from_utf8(e.name().into_inner())?;
    // Name and character well-formedness were validated by
    // `WellformedChecker::start` before this function ran.
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls: smallvec::SmallVec<[Namespace; 2]> = smallvec::SmallVec::new();
    let mut attributes: smallvec::SmallVec<[(&str, std::borrow::Cow<str>); 8]> =
        smallvec::SmallVec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.into_inner())?;
        let value = match attr.value {
            std::borrow::Cow::Borrowed(raw) => expander.expand_attr(std::str::from_utf8(raw)?)?,
            std::borrow::Cow::Owned(raw) => std::borrow::Cow::Owned(
                expander
                    .expand_attr(std::str::from_utf8(&raw)?)?
                    .into_owned(),
            ),
        };

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
