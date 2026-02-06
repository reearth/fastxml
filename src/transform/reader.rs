//! Reader-based StreamTransformer for memory-efficient processing.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::xpath::XPathSource;

use super::builder::{Handler, HandlerCallback};
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::streaming;
use super::xpath_analyze::{self, StreamableXPath, XPathAnalysis};

/// Builder for streaming XML transformations from a reader source.
///
/// Unlike [`StreamTransformer`](super::StreamTransformer) which requires the entire XML input as a string,
/// this reads from a `BufRead` source, enabling memory-efficient processing of
/// large XML files. Non-matched content is echoed through quick_xml's Writer
/// rather than using zero-copy byte slicing.
///
/// # Example
///
/// ```rust
/// use std::io::{BufReader, Cursor};
/// use fastxml::transform::StreamTransformerReader;
///
/// let xml = r#"<root><item>A</item><other>B</other></root>"#;
/// let reader = BufReader::new(Cursor::new(xml));
///
/// let mut output = Vec::new();
/// let count = StreamTransformerReader::new(reader)
///     .on("//item", |node| node.set_attribute("processed", "true"))
///     .run_to_writer(&mut output)?;
///
/// let result = String::from_utf8(output).unwrap();
/// assert!(result.contains(r#"processed="true""#));
/// # Ok::<(), fastxml::transform::TransformError>(())
/// ```
pub struct StreamTransformerReader<'a, R: BufRead> {
    reader: R,
    handlers: Vec<Handler<'a>>,
    namespaces: HashMap<String, String>,
}

impl<'a, R: BufRead> StreamTransformerReader<'a, R> {
    /// Creates a new reader-based transformer.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            handlers: Vec::new(),
            namespaces: HashMap::new(),
        }
    }

    /// Registers an XPath expression with its callback function.
    ///
    /// Only streamable XPath expressions are supported. Non-streamable expressions
    /// (e.g., those using `last()`, backward axes) will return an error at execution time.
    pub fn on<F>(mut self, xpath: &str, callback: F) -> Self
    where
        F: FnMut(&mut EditableNode) + 'a,
    {
        self.handlers.push(Handler {
            xpath: XPathSource::String(xpath.to_string()),
            callback: HandlerCallback::Simple(Box::new(callback)),
        });
        self
    }

    /// Registers a namespace prefix for use in XPath expressions.
    pub fn namespace(mut self, prefix: &str, uri: &str) -> Self {
        self.namespaces.insert(prefix.to_string(), uri.to_string());
        self
    }

    /// Registers multiple namespace prefixes at once.
    pub fn namespaces<I, S1, S2>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = (S1, S2)>,
        S1: AsRef<str>,
        S2: AsRef<str>,
    {
        for (prefix, uri) in iter {
            self.namespaces
                .insert(prefix.as_ref().to_string(), uri.as_ref().to_string());
        }
        self
    }

    /// Executes all registered handlers and writes the result to a writer.
    ///
    /// This is the memory-efficient way to transform XML: the input is read
    /// incrementally and the output is written incrementally, without needing
    /// the entire file in memory.
    ///
    /// Returns the number of elements that were matched and transformed.
    pub fn run_to_writer<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        self.execute_transform_reader(writer)
    }

    /// Executes all registered handlers for their side effects only.
    ///
    /// Unlike `run_to_writer()`, this method does not produce output XML.
    /// Use this when you only need to extract data from a large file.
    pub fn for_each(self) -> TransformResult<()> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        self.execute_for_each_reader()?;
        Ok(())
    }

    /// Internal: Execute transformation with reader source
    fn execute_transform_reader<W: Write>(mut self, writer: &mut W) -> TransformResult<usize> {
        // Parse and analyze all XPaths
        let mut analyses: Vec<XPathAnalysis> = Vec::with_capacity(self.handlers.len());
        for handler in &self.handlers {
            let expr = handler.xpath.parse()?;
            analyses.push(xpath_analyze::analyze_xpath(&expr));
        }

        // All must be streamable (no fallback for reader mode)
        let mut streamable_xpaths: Vec<StreamableXPath> = Vec::with_capacity(analyses.len());
        for (i, analysis) in analyses.into_iter().enumerate() {
            match analysis {
                XPathAnalysis::Streamable(s) => streamable_xpaths.push(s),
                XPathAnalysis::NotStreamable(reason) => {
                    let xpath_str = self.handlers[i]
                        .xpath
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    return Err(TransformError::NotStreamable {
                        xpath: xpath_str,
                        reason,
                    });
                }
            }
        }

        // Fast path: single handler
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            match handler.callback {
                HandlerCallback::Simple(mut f) => {
                    return streaming::process_streaming_from_reader(
                        self.reader,
                        &streamable_xpaths[0],
                        &self.namespaces,
                        |node| f(node),
                        writer,
                    );
                }
                HandlerCallback::WithContext(_) => {
                    return Err(TransformError::InvalidXPath(
                        "WithContext callbacks are not supported in reader mode".to_string(),
                    ));
                }
            }
        }

        // Multi-handler path
        type Callback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;
        let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

        for handler in self.handlers.iter_mut() {
            if let HandlerCallback::Simple(f) = &mut handler.callback {
                callbacks.push(Box::new(move |node: &mut EditableNode| f(node)));
            } else {
                return Err(TransformError::InvalidXPath(
                    "WithContext callbacks are not supported in reader mode".to_string(),
                ));
            }
        }

        let mut handler_pairs: Vec<streaming::MultiTransformHandler<'_>> = streamable_xpaths
            .iter()
            .zip(callbacks.iter_mut())
            .map(|(xpath, cb)| (xpath, cb.as_mut() as &mut dyn FnMut(&mut EditableNode)))
            .collect();

        streaming::process_streaming_multi_from_reader(
            self.reader,
            &mut handler_pairs,
            &self.namespaces,
            writer,
        )
    }

    /// Internal: Execute for_each with reader source
    fn execute_for_each_reader(mut self) -> TransformResult<usize> {
        // Parse and analyze all XPaths
        let mut analyses: Vec<XPathAnalysis> = Vec::with_capacity(self.handlers.len());
        for handler in &self.handlers {
            let expr = handler.xpath.parse()?;
            analyses.push(xpath_analyze::analyze_xpath(&expr));
        }

        // All must be streamable (no fallback for reader mode)
        let mut streamable_xpaths: Vec<StreamableXPath> = Vec::with_capacity(analyses.len());
        for (i, analysis) in analyses.into_iter().enumerate() {
            match analysis {
                XPathAnalysis::Streamable(s) => streamable_xpaths.push(s),
                XPathAnalysis::NotStreamable(reason) => {
                    let xpath_str = self.handlers[i]
                        .xpath
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    return Err(TransformError::NotStreamable {
                        xpath: xpath_str,
                        reason,
                    });
                }
            }
        }

        // Fast path: single handler
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            match handler.callback {
                HandlerCallback::Simple(mut f) => {
                    return streaming::process_for_each_from_reader(
                        self.reader,
                        &streamable_xpaths[0],
                        &self.namespaces,
                        |node| f(node),
                    );
                }
                HandlerCallback::WithContext(_) => {
                    return Err(TransformError::InvalidXPath(
                        "WithContext callbacks are not supported in reader mode".to_string(),
                    ));
                }
            }
        }

        // Multi-handler path
        type Callback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;
        let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

        for handler in self.handlers.iter_mut() {
            if let HandlerCallback::Simple(f) = &mut handler.callback {
                callbacks.push(Box::new(move |node: &mut EditableNode| f(node)));
            } else {
                return Err(TransformError::InvalidXPath(
                    "WithContext callbacks are not supported in reader mode".to_string(),
                ));
            }
        }

        let mut handler_pairs: Vec<streaming::MultiHandler<'_>> = streamable_xpaths
            .iter()
            .zip(callbacks.iter_mut())
            .map(|(xpath, cb)| (xpath, cb.as_mut() as &mut dyn FnMut(&mut EditableNode)))
            .collect();

        streaming::process_for_each_multi_from_reader(
            self.reader,
            &mut handler_pairs,
            &self.namespaces,
        )
    }
}
