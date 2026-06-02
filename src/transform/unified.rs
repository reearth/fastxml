//! The unified [`Transformer`] entry point.
//!
//! `Transformer` is the redesigned, consistent front door for streaming XML
//! transformation. It follows the crate-wide shape — `from(source)`, register
//! handlers, then a terminal — and unifies the two underlying engines behind
//! one type:
//!
//! - [`Transformer::from`] (`&str`) transforms in memory, reusing the input
//!   verbatim for unchanged regions (zero-copy).
//! - [`Transformer::from_reader`] streams from any [`BufRead`], for inputs too
//!   large to hold in memory.
//!
//! ```ignore
//! use fastxml::transform::Transformer;
//!
//! let out = Transformer::from(xml)
//!     .on("//item[@id='2']", |node| node.set_attribute("done", "1"))
//!     .to_string()?;
//!
//! Transformer::from_reader(file)
//!     .on("//item", |node| node.set_attribute("seen", "1"))
//!     .write_to(&mut std::io::stdout())?;
//! ```
//!
//! Advanced, in-memory-only features (`on_with_context`, `collect`,
//! `collect_multi`, fallback for non-streamable XPath) remain available on
//! [`StreamTransformer`] directly.

use std::io::{BufRead, Write};

use crate::transform::builder::StreamTransformer;
use crate::transform::editable::EditableNode;
use crate::transform::error::{TransformError, TransformResult};
use crate::transform::reader::StreamTransformerReader;

/// The transformation engine, selected by the input.
enum Inner<'a> {
    /// In-memory, zero-copy transform over a borrowed `&str`.
    InMemory(StreamTransformer<'a>),
    /// Streaming transform over any buffered reader.
    Reader(StreamTransformerReader<'a, Box<dyn BufRead + 'a>>),
}

/// A consistent front door for streaming XML transformation.
///
/// `from(source)` → `on(xpath, callback)…` → a terminal (`to_string`,
/// `into_bytes`, `write_to`, or `for_each`). The input type selects the engine
/// (in-memory zero-copy vs reader-based streaming) transparently.
pub struct Transformer<'a> {
    inner: Inner<'a>,
}

impl<'a> From<&'a str> for Transformer<'a> {
    fn from(xml: &'a str) -> Self {
        Transformer {
            inner: Inner::InMemory(StreamTransformer::new(xml)),
        }
    }
}

impl<'a> Transformer<'a> {
    /// Creates a transformer that streams its input from any [`BufRead`].
    ///
    /// `from` cannot be used for readers because of Rust coherence
    /// (`From<&str>` and a blanket `From<R: BufRead>` cannot coexist).
    pub fn from_reader<R: BufRead + 'a>(reader: R) -> Self {
        Transformer {
            inner: Inner::Reader(StreamTransformerReader::new(Box::new(reader))),
        }
    }

    /// Registers a callback to run on each element matching `xpath`.
    pub fn on<F>(self, xpath: &str, callback: F) -> Self
    where
        F: FnMut(&mut EditableNode) + 'a,
    {
        let inner = match self.inner {
            Inner::InMemory(t) => Inner::InMemory(t.on(xpath, callback)),
            Inner::Reader(t) => Inner::Reader(t.on(xpath, callback)),
        };
        Transformer { inner }
    }

    /// Binds a namespace prefix used in the handler XPath expressions.
    pub fn namespace(self, prefix: &str, uri: &str) -> Self {
        let inner = match self.inner {
            Inner::InMemory(t) => Inner::InMemory(t.namespace(prefix, uri)),
            Inner::Reader(t) => Inner::Reader(t.namespace(prefix, uri)),
        };
        Transformer { inner }
    }

    /// Binds multiple namespace prefixes at once.
    pub fn namespaces<I, S1, S2>(self, iter: I) -> Self
    where
        I: IntoIterator<Item = (S1, S2)>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        let inner = match self.inner {
            Inner::InMemory(t) => Inner::InMemory(t.namespaces(iter)),
            Inner::Reader(t) => Inner::Reader(t.namespaces(iter)),
        };
        Transformer { inner }
    }

    /// Runs the handlers for their side effects only, producing no output XML.
    pub fn for_each(self) -> TransformResult<()> {
        match self.inner {
            Inner::InMemory(t) => t.for_each(),
            Inner::Reader(t) => t.for_each(),
        }
    }

    /// Runs the transform, writing the result to `writer`. Returns the number
    /// of matched elements.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        match self.inner {
            Inner::InMemory(t) => {
                let output = t.run()?;
                let count = output.count();
                output.write_to(writer)?;
                Ok(count)
            }
            Inner::Reader(t) => t.run_to_writer(writer),
        }
    }

    /// Runs the transform and returns the resulting XML as bytes.
    pub fn into_bytes(self) -> TransformResult<Vec<u8>> {
        let mut buf = Vec::new();
        self.write_to(&mut buf)?;
        Ok(buf)
    }

    /// Runs the transform and returns the resulting XML as a `String`.
    pub fn to_string(self) -> TransformResult<String> {
        let bytes = self.into_bytes()?;
        String::from_utf8(bytes).map_err(|e| TransformError::Utf8(e.utf8_error()))
    }
}
