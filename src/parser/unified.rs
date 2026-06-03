//! The unified [`Parser`] entry point.
//!
//! `Parser` is the redesigned, consistent front door for parsing. It follows
//! the crate-wide shape — `from(source)`, optional configuration, then a
//! terminal — and folds the `parse` / `parse_with_options` / `parse_from_bufread`
//! functions into one surface:
//!
//! ```ignore
//! use fastxml::Parser;
//!
//! let doc = Parser::from(xml).parse()?;                 // DOM from &str / &[u8]
//! let doc = Parser::from_reader(file).parse()?;         // DOM from any reader
//! let doc = Parser::from(xml).options(opts).parse()?;   // with parser options
//!
//! for event in Parser::from(xml).events()? { /* … */ }  // buffered event list
//! ```
//!
//! For true push-based streaming (handlers invoked as events arrive without
//! buffering), use [`StreamingParser`](crate::event::StreamingParser) directly.

use std::io::BufRead;
use std::sync::{Arc, Mutex};

use crate::document::XmlDocument;
use crate::error::Result;
use crate::event::{StreamingParser, XmlEvent, XmlEventHandler};

use super::{ParserOptions, parse_from_bufread, parse_with_options};

/// The input to parse.
enum Source<'a> {
    /// In-memory XML bytes.
    Bytes(&'a [u8]),
    /// Any buffered reader.
    Reader(Box<dyn BufRead + 'a>),
}

/// A consistent front door for parsing XML.
///
/// `from(source)` → optional [`options`](Parser::options) → a terminal
/// ([`parse`](Parser::parse) for a DOM, or [`events`](Parser::events) for a
/// buffered event list).
pub struct Parser<'a> {
    source: Source<'a>,
    options: ParserOptions,
}

impl<'a> From<&'a str> for Parser<'a> {
    fn from(xml: &'a str) -> Self {
        Self::from_bytes(xml.as_bytes())
    }
}

impl<'a> From<&'a [u8]> for Parser<'a> {
    fn from(xml: &'a [u8]) -> Self {
        Self::from_bytes(xml)
    }
}

impl<'a> Parser<'a> {
    fn from_bytes(bytes: &'a [u8]) -> Self {
        Parser {
            source: Source::Bytes(bytes),
            options: ParserOptions::default(),
        }
    }

    /// Creates a parser that reads its input from any [`BufRead`].
    ///
    /// `from` cannot be used for readers because of Rust coherence
    /// (`From<&str>` and a blanket `From<R: BufRead>` cannot coexist).
    pub fn from_reader<R: BufRead + 'a>(reader: R) -> Self {
        Parser {
            source: Source::Reader(Box::new(reader)),
            options: ParserOptions::default(),
        }
    }

    /// Sets the parser options (buffer size, memory limits, libxml-compat, …).
    pub fn options(mut self, options: ParserOptions) -> Self {
        self.options = options;
        self
    }

    /// Parses the input into a DOM [`XmlDocument`].
    pub fn parse(self) -> Result<XmlDocument> {
        match self.source {
            Source::Bytes(bytes) => parse_with_options(bytes, &self.options),
            Source::Reader(reader) => parse_from_bufread(reader, &self.options),
        }
    }

    /// Parses the input and returns all XML events as a buffered `Vec`.
    ///
    /// This collects the full event stream into memory. For push-based
    /// streaming without buffering, use [`for_each_event`](Self::for_each_event).
    pub fn events(self) -> Result<Vec<XmlEvent>> {
        match self.source {
            Source::Bytes(bytes) => collect_events(bytes),
            Source::Reader(reader) => collect_events(reader),
        }
    }

    /// Streams the input, invoking `on_event` for every event as it is read,
    /// with **constant memory** (nothing is buffered).
    ///
    /// The callback is borrowed for the duration of the call, so it may capture
    /// and mutate local state. Return `Err(..)` to stop early.
    ///
    /// ```ignore
    /// let mut elements = 0;
    /// Parser::from_reader(file).for_each_event(|event| {
    ///     if matches!(event, XmlEvent::StartElement { .. }) {
    ///         elements += 1;
    ///     }
    ///     Ok(())
    /// })?;
    /// ```
    pub fn for_each_event<F>(self, on_event: F) -> Result<()>
    where
        F: FnMut(&XmlEvent) -> Result<()>,
    {
        match self.source {
            Source::Bytes(bytes) => {
                let mut parser = StreamingParser::new(bytes);
                parser.for_each_event(on_event)
            }
            Source::Reader(reader) => {
                let mut parser = StreamingParser::new(reader);
                parser.for_each_event(on_event)
            }
        }
    }
}

fn collect_events<R: BufRead>(reader: R) -> Result<Vec<XmlEvent>> {
    let collected = Arc::new(Mutex::new(Vec::new()));
    let mut parser = StreamingParser::new(reader);
    parser.add_handler(Box::new(CollectHandler(Arc::clone(&collected))));
    parser.parse()?;
    let events = std::mem::take(&mut *collected.lock().unwrap());
    Ok(events)
}

/// Collects every event into a shared buffer.
struct CollectHandler(Arc<Mutex<Vec<XmlEvent>>>);

impl XmlEventHandler for CollectHandler {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        self.0.lock().unwrap().push(event.clone());
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
