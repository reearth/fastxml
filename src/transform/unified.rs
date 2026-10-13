//! The unified [`Transformer`] entry point.
//!
//! `Transformer` is the redesigned, consistent front door for streaming XML
//! transformation. It follows the crate-wide shape — `from(source)`, register
//! handlers, then a terminal — and unifies both transform engines behind one
//! type:
//!
//! - [`Transformer::from`] (`&str`) transforms in memory, reusing the input
//!   verbatim for unchanged regions (zero-copy). All features are available.
//! - [`Transformer::from_reader`] streams from any [`BufRead`], for inputs too
//!   large to hold in memory. Only the streamable subset is available; the
//!   in-memory-only operations below return a clear error.
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
//! In-memory-only operations: [`on_with_context`](Transformer::on_with_context),
//! [`with_root_namespaces`](Transformer::with_root_namespaces),
//! [`allow_fallback`](Transformer::allow_fallback) /
//! [`fallback_mode`](Transformer::fallback_mode),
//! [`collect`](Transformer::collect), and
//! [`collect_multi`](Transformer::collect_multi).

use std::io::{BufRead, Write};
use std::ops::ControlFlow;

use crate::transform::FallbackMode;
use crate::transform::builder::StreamTransformer;
use crate::transform::context::TransformContext;
use crate::transform::editable::EditableNode;
use crate::transform::error::{TransformError, TransformResult};
use crate::transform::matched::ElementMatch;
use crate::transform::multi::CollectMulti;
use crate::transform::reader::StreamTransformerReader;
use crate::transform::streamable::IntoStreamable;

/// The transformation engine, selected by the input.
enum Inner<'a> {
    /// In-memory, zero-copy transform over a borrowed `&str` (full feature set).
    InMemory(StreamTransformer<'a>),
    /// Streaming transform over any buffered reader (streamable subset only).
    Reader(StreamTransformerReader<'a, Box<dyn BufRead + 'a>>),
}

/// A consistent front door for streaming XML transformation.
///
/// `from(source)` → `on(xpath, callback)…` → a terminal (`to_string`,
/// `into_bytes`, `write_to`, `for_each`, `collect`, `collect_multi`).
pub struct Transformer<'a> {
    inner: Inner<'a>,
    /// An in-memory-only operation was requested on reader-based input; the
    /// error is surfaced at the terminal so the builder chain stays fluent.
    deferred: Option<&'static str>,
}

/// Builds the error for an in-memory-only operation used on reader input.
fn unsupported(msg: &'static str) -> TransformError {
    TransformError::Other(crate::error::Error::InvalidOperation(msg.to_string()))
}

impl<'a> From<&'a str> for Transformer<'a> {
    fn from(xml: &'a str) -> Self {
        Transformer {
            inner: Inner::InMemory(StreamTransformer::new(xml)),
            deferred: None,
        }
    }
}

impl<'a> Transformer<'a> {
    /// Creates a transformer that streams its input from any [`BufRead`].
    ///
    /// `from` cannot be used for readers because of Rust coherence
    /// (`From<&str>` and a blanket `From<R: BufRead>` cannot coexist).
    ///
    /// Reader-based transforms support only the streamable subset of the API;
    /// the in-memory-only operations return an error at the terminal.
    pub fn from_reader<R: BufRead + 'a>(reader: R) -> Self {
        Transformer {
            inner: Inner::Reader(StreamTransformerReader::new(Box::new(reader))),
            deferred: None,
        }
    }

    /// Registers a callback to run on each element matching `xpath`.
    ///
    /// `xpath` is anything that is [`IntoStreamable`]: a string (analyzed when
    /// the transform runs) or a pre-validated
    /// [`StreamableQuery`](crate::transform::StreamableQuery).
    pub fn on<X, F>(self, xpath: X, callback: F) -> Self
    where
        X: IntoStreamable,
        F: FnMut(&mut EditableNode) + 'a,
    {
        let Transformer { inner, deferred } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.on(xpath, callback)),
            Inner::Reader(t) => Inner::Reader(t.on(xpath, callback)),
        };
        Transformer { inner, deferred }
    }

    /// Registers a callback that also receives ancestor/position context.
    ///
    /// In-memory input only; on reader input this is reported at the terminal.
    pub fn on_with_context<X, F>(self, xpath: X, callback: F) -> Self
    where
        X: IntoStreamable,
        F: FnMut(&mut EditableNode, &TransformContext) + 'a,
    {
        let Transformer {
            inner,
            mut deferred,
        } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.on_with_context(xpath, callback)),
            Inner::Reader(t) => {
                deferred.get_or_insert(
                    "on_with_context is only available for in-memory input (Transformer::from)",
                );
                Inner::Reader(t)
            }
        };
        Transformer { inner, deferred }
    }

    /// Registers a read-only handler for `xpath`, called at the start tag of
    /// each matching element with its name, namespace and attributes.
    ///
    /// Unlike [`on`](Self::on), no subtree is buffered, so memory use does not
    /// grow with the size of the matched elements. Every match is reported,
    /// including elements nested inside another match (and inside an `on`
    /// handler's subtree). Return [`ControlFlow::Break`] to stop this
    /// handler; once every handler has stopped, the rest of the input is not
    /// read (and so not checked for well-formedness).
    ///
    /// Runs with [`for_each`](Self::for_each) only: the output terminals
    /// return an error. The XPath must be streamable, except for in-memory
    /// input with [`allow_fallback`](Self::allow_fallback), where a
    /// non-streamable XPath is evaluated against a DOM of the input.
    ///
    /// ```
    /// use std::ops::ControlFlow;
    /// use fastxml::transform::Transformer;
    ///
    /// let xml = r#"<root><item id="1"><item id="2"/></item></root>"#;
    /// let mut ids = Vec::new();
    /// Transformer::from_reader(xml.as_bytes())
    ///     .on_match("//item", |m| {
    ///         ids.push(m.get_attribute("id").unwrap_or_default().to_string());
    ///         ControlFlow::Continue(())
    ///     })
    ///     .for_each()?;
    /// assert_eq!(ids, ["1", "2"]);
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn on_match<X, F>(self, xpath: X, callback: F) -> Self
    where
        X: IntoStreamable,
        F: FnMut(&ElementMatch) -> ControlFlow<()> + 'a,
    {
        let Transformer { inner, deferred } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.on_match(xpath, callback)),
            Inner::Reader(t) => Inner::Reader(t.on_match(xpath, callback)),
        };
        Transformer { inner, deferred }
    }

    /// Limits the handler registered just before this call to `n` calls.
    /// Once a handler has reached its limit, no further subtrees are built
    /// for it; once every handler has stopped, the rest of the input is not
    /// read (and so not checked for well-formedness).
    ///
    /// Applies to [`for_each`](Self::for_each) only: the output terminals
    /// must copy the whole document and return an error instead. Calling it
    /// before any handler is registered is also an error, reported by the
    /// terminal.
    ///
    /// ```
    /// use fastxml::transform::Transformer;
    ///
    /// let xml = "<r><i n='1'/><i n='2'/><i n='3'/></r>";
    /// let mut seen = Vec::new();
    /// Transformer::from(xml)
    ///     .on("//i", |node| seen.push(node.get_attribute("n").unwrap()))
    ///     .limit(2)
    ///     .for_each()?;
    /// assert_eq!(seen, ["1", "2"]);
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn limit(mut self, n: usize) -> Self {
        let applied = match &mut self.inner {
            Inner::InMemory(t) => t.limit_last(n),
            Inner::Reader(t) => t.limit_last(n),
        };
        if !applied {
            self.deferred
                .get_or_insert("limit must follow a handler (on, on_with_context or on_match)");
        }
        self
    }

    /// Binds a namespace prefix used in the handler XPath expressions.
    pub fn namespace(self, prefix: &str, uri: &str) -> Self {
        let Transformer { inner, deferred } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.namespace(prefix, uri)),
            Inner::Reader(t) => Inner::Reader(t.namespace(prefix, uri)),
        };
        Transformer { inner, deferred }
    }

    /// Binds multiple namespace prefixes at once.
    pub fn namespaces<I, S1, S2>(self, iter: I) -> Self
    where
        I: IntoIterator<Item = (S1, S2)>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        let Transformer { inner, deferred } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.namespaces(iter)),
            Inner::Reader(t) => Inner::Reader(t.namespaces(iter)),
        };
        Transformer { inner, deferred }
    }

    /// Auto-registers namespace prefixes declared on the root element.
    ///
    /// In-memory input only.
    pub fn with_root_namespaces(self) -> TransformResult<Self> {
        let Transformer { inner, deferred } = self;
        match inner {
            Inner::InMemory(t) => Ok(Transformer {
                inner: Inner::InMemory(t.with_root_namespaces()?),
                deferred,
            }),
            Inner::Reader(_) => Err(unsupported(
                "with_root_namespaces is only available for in-memory input (Transformer::from)",
            )),
        }
    }

    /// Enables two-pass fallback for non-streamable XPath (loads the document
    /// into memory). In-memory input only.
    pub fn allow_fallback(self) -> Self {
        let Transformer {
            inner,
            mut deferred,
        } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.allow_fallback()),
            Inner::Reader(t) => {
                deferred.get_or_insert(
                    "allow_fallback is only available for in-memory input (Transformer::from)",
                );
                Inner::Reader(t)
            }
        };
        Transformer { inner, deferred }
    }

    /// Sets the fallback mode explicitly. In-memory input only.
    pub fn fallback_mode(self, mode: FallbackMode) -> Self {
        let Transformer {
            inner,
            mut deferred,
        } = self;
        let inner = match inner {
            Inner::InMemory(t) => Inner::InMemory(t.fallback_mode(mode)),
            Inner::Reader(t) => {
                if mode != FallbackMode::Disabled {
                    deferred.get_or_insert(
                        "fallback_mode is only available for in-memory input (Transformer::from)",
                    );
                }
                Inner::Reader(t)
            }
        };
        Transformer { inner, deferred }
    }

    /// Runs the handlers for their side effects only, producing no output XML.
    pub fn for_each(self) -> TransformResult<()> {
        if let Some(msg) = self.deferred {
            return Err(unsupported(msg));
        }
        match self.inner {
            Inner::InMemory(t) => t.for_each(),
            Inner::Reader(t) => t.for_each(),
        }
    }

    /// Runs the transform, writing the result to `writer`. Returns the number
    /// of matched elements.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        if let Some(msg) = self.deferred {
            return Err(unsupported(msg));
        }
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

    /// Extracts a value from each element matching `xpath`, in a single pass.
    ///
    /// In-memory input only.
    pub fn collect<X, F, T>(self, xpath: X, f: F) -> TransformResult<Vec<T>>
    where
        X: IntoStreamable,
        F: FnMut(&mut EditableNode) -> T,
    {
        if let Some(msg) = self.deferred {
            return Err(unsupported(msg));
        }
        match self.inner {
            Inner::InMemory(t) => t.collect(xpath, f),
            Inner::Reader(_) => Err(unsupported(
                "collect is only available for in-memory input (Transformer::from)",
            )),
        }
    }

    /// Extracts values from multiple XPath expressions in a single pass.
    ///
    /// In-memory input only.
    pub fn collect_multi<C: CollectMulti<'a>>(self, collectors: C) -> TransformResult<C::Output> {
        if let Some(msg) = self.deferred {
            return Err(unsupported(msg));
        }
        match self.inner {
            Inner::InMemory(t) => t.collect_multi(collectors),
            Inner::Reader(_) => Err(unsupported(
                "collect_multi is only available for in-memory input (Transformer::from)",
            )),
        }
    }
}
