//! Streaming XML transformation with zero-copy output.
//!
//! This module provides APIs for transforming XML documents by selectively
//! modifying elements that match XPath expressions, while preserving
//! unchanged portions of the document with zero-copy efficiency.
//!
//! # Features
//!
//! - **Zero-copy output**: Unchanged portions of the input are written directly
//!   without copying or re-serialization
//! - **Selective DOM**: Only matched elements are converted to a modifiable DOM
//! - **Streaming**: Single-pass processing for compatible XPath expressions
//! - **Fallback**: Automatic two-pass processing for complex XPath patterns
//! - **Multiple handlers**: Register multiple XPath-callback pairs
//!
//! # Streamable XPath Patterns
//!
//! The following patterns can be processed in a single streaming pass:
//!
//! - Absolute paths: `/root/items/item`
//! - Descendant search: `//item`
//! - Attribute predicates: `//item[@id='2']`
//! - Namespaced elements: `//ns:item`
//! - Position predicates with upper bound: `//item[position() <= 3]`
//!
//! The following patterns require two-pass processing:
//!
//! - `last()` function: `//item[last()]`, `//item[position()=last()]`
//! - Backward axes: `//item/parent::*`, `//item/ancestor::root`
//! - Complex predicates requiring full tree evaluation
//!
//! # Examples
//!
//! ## Transform with Multiple Handlers
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item id="1">A</item><other>B</other></root>"#;
//!
//! let result = StreamTransformer::new(xml)
//!     .on("//item", |node| {
//!         node.set_attribute("type", "item");
//!     })
//!     .on("//other", |node| {
//!         node.set_attribute("type", "other");
//!     })
//!     .run()?
//!     .to_string()?;
//!
//! assert!(result.contains(r#"type="item""#));
//! assert!(result.contains(r#"type="other""#));
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```
//!
//! ## Collect Data
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
//!
//! let ids: Vec<String> = StreamTransformer::new(xml)
//!     .collect("//item", |node| node.get_attribute("id").unwrap_or_default())?;
//!
//! assert_eq!(ids, vec!["1", "2"]);
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```
//!
//! ## For Each (Side Effects Only)
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item>A</item><other>B</other></root>"#;
//!
//! let mut items = Vec::new();
//! let mut others = Vec::new();
//!
//! StreamTransformer::new(xml)
//!     .on("//item", |node| {
//!         items.push(node.get_content().unwrap_or_default());
//!     })
//!     .on("//other", |node| {
//!         others.push(node.get_content().unwrap_or_default());
//!     })
//!     .for_each()?;
//!
//! assert_eq!(items, vec!["A"]);
//! assert_eq!(others, vec!["B"]);
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```

pub mod editable;
pub mod error;
pub mod fallback;
pub mod span;
pub mod streaming;
pub mod xpath_analyze;

use std::collections::HashMap;
use std::io::Write;

pub use editable::{EditableNode, EditableNodeBuilder, EditableNodeRef, Modification, NewNode};
pub use error::{TransformError, TransformResult};
pub use span::ByteSpan;
pub use xpath_analyze::{NotStreamableReason, StreamableXPath, XPathAnalysis};

// Re-export XPath types for convenience
pub use crate::xpath::{Expr, XPathSource};

/// A handler that pairs an XPath expression with a callback function.
struct Handler<'a> {
    xpath: XPathSource,
    callback: Box<dyn FnMut(&mut EditableNode) + 'a>,
}

/// Builder for streaming XML transformations.
///
/// Provides a fluent API for configuring and executing XML transformations
/// with support for multiple XPath-callback pairs.
///
/// # Example
///
/// ```rust
/// use fastxml::transform::StreamTransformer;
///
/// let xml = r#"<root><item>A</item><other>B</other></root>"#;
///
/// let result = StreamTransformer::new(xml)
///     .on("//item", |node| node.set_attribute("processed", "true"))
///     .on("//other", |node| node.remove())
///     .run()?
///     .to_string()?;
/// # Ok::<(), fastxml::transform::TransformError>(())
/// ```
pub struct StreamTransformer<'a> {
    input: &'a str,
    handlers: Vec<Handler<'a>>,
    namespaces: HashMap<String, String>,
}

impl<'a> StreamTransformer<'a> {
    /// Creates a new transformer for the given XML input.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            handlers: Vec::new(),
            namespaces: HashMap::new(),
        }
    }

    /// Registers an XPath expression with its callback function.
    ///
    /// Multiple handlers can be registered, and they will all be applied
    /// during transformation. Each handler is called when its XPath matches.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><a/><b/></root>"#;
    ///
    /// let result = StreamTransformer::new(xml)
    ///     .on("//a", |node| node.set_attribute("found", "a"))
    ///     .on("//b", |node| node.set_attribute("found", "b"))
    ///     .run()?
    ///     .to_string()?;
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn on<F>(mut self, xpath: &str, callback: F) -> Self
    where
        F: FnMut(&mut EditableNode) + 'a,
    {
        self.handlers.push(Handler {
            xpath: XPathSource::String(xpath.to_string()),
            callback: Box::new(callback),
        });
        self
    }

    /// Registers a namespace prefix for use in XPath expressions.
    pub fn namespace(mut self, prefix: &str, uri: &str) -> Self {
        self.namespaces.insert(prefix.to_string(), uri.to_string());
        self
    }

    /// Registers multiple namespace prefixes at once.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root xmlns:gml="http://example.com/gml"><gml:point/></root>"#;
    ///
    /// let result = StreamTransformer::new(xml)
    ///     .namespaces([
    ///         ("gml", "http://example.com/gml"),
    ///     ])
    ///     .on("//gml:point", |node| node.set_attribute("found", "true"))
    ///     .run()?
    ///     .to_string()?;
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
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

    /// Imports namespace bindings from an XmlDocument.
    ///
    /// This is useful when you want to use the same namespace prefixes
    /// as declared in the document.
    pub fn with_document_namespaces(mut self, doc: &crate::document::XmlDocument) -> Self {
        self.namespaces.extend(doc.namespaces());
        self
    }

    /// Executes all registered handlers and returns the transformation output.
    ///
    /// This method processes the XML and applies all handlers registered via `on()`.
    /// Returns a `TransformOutput` that can be converted to a String or written to a writer.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item/></root>"#;
    ///
    /// let output = StreamTransformer::new(xml)
    ///     .on("//item", |node| node.set_attribute("done", "true"))
    ///     .run()?;
    ///
    /// let result = output.to_string()?;
    /// assert!(result.contains(r#"done="true""#));
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn run(self) -> TransformResult<TransformOutput> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        let mut output = Vec::new();
        let count = self.execute_transform(&mut output)?;

        Ok(TransformOutput {
            data: output,
            count,
        })
    }

    /// Executes all registered handlers for their side effects only.
    ///
    /// Unlike `run()`, this method does not produce output XML.
    /// Use this when you only need to extract data or perform side effects.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item id="1"/><item id="2"/></root>"#;
    ///
    /// let mut ids = Vec::new();
    /// StreamTransformer::new(xml)
    ///     .on("//item", |node| {
    ///         if let Some(id) = node.get_attribute("id") {
    ///             ids.push(id);
    ///         }
    ///     })
    ///     .for_each()?;
    ///
    /// assert_eq!(ids, vec!["1", "2"]);
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn for_each(self) -> TransformResult<()> {
        if self.handlers.is_empty() {
            return Err(TransformError::InvalidXPath(
                "No handlers registered. Use .on() to add handlers.".to_string(),
            ));
        }

        self.execute_for_each()?;
        Ok(())
    }

    /// Collects values from matched elements using a single XPath expression.
    ///
    /// This is a convenience method for extracting data from elements.
    /// For multiple XPath expressions, use `on()` with `for_each()` instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item>A</item><item>B</item></root>"#;
    ///
    /// let contents: Vec<String> = StreamTransformer::new(xml)
    ///     .collect("//item", |node| node.get_content().unwrap_or_default())?;
    ///
    /// assert_eq!(contents, vec!["A", "B"]);
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn collect<F, T>(self, xpath: &str, mut f: F) -> TransformResult<Vec<T>>
    where
        F: FnMut(&mut EditableNode) -> T,
    {
        let mut results = Vec::new();
        let xpath_source = XPathSource::String(xpath.to_string());

        stream_for_each_impl(self.input, &xpath_source, &self.namespaces, |node| {
            results.push(f(node));
        })?;

        Ok(results)
    }

    /// Internal: Execute transformation with all handlers
    fn execute_transform<W: Write>(mut self, writer: &mut W) -> TransformResult<usize> {
        // For now, we process handlers sequentially
        // TODO: optimize for multiple handlers in a single pass
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            stream_transform_impl(
                self.input,
                &handler.xpath,
                &self.namespaces,
                handler.callback,
                writer,
            )
        } else {
            // Multiple handlers: process sequentially, passing output to next
            let mut current_input = self.input.to_string();
            let mut total_count = 0;

            for handler in self.handlers {
                let mut output = Vec::new();
                let count = stream_transform_impl(
                    &current_input,
                    &handler.xpath,
                    &self.namespaces,
                    handler.callback,
                    &mut output,
                )?;
                total_count += count;
                current_input =
                    String::from_utf8(output).map_err(|e| TransformError::Utf8(e.utf8_error()))?;
            }

            writer
                .write_all(current_input.as_bytes())
                .map_err(TransformError::Io)?;
            Ok(total_count)
        }
    }

    /// Internal: Execute for_each with all handlers
    fn execute_for_each(mut self) -> TransformResult<usize> {
        let mut total_count = 0;

        for handler in &mut self.handlers {
            let count =
                stream_for_each_impl(self.input, &handler.xpath, &self.namespaces, |node| {
                    (handler.callback)(node);
                })?;
            total_count += count;
        }

        Ok(total_count)
    }
}

/// Output from a transformation operation.
///
/// Contains the transformed XML data and metadata about the transformation.
pub struct TransformOutput {
    data: Vec<u8>,
    count: usize,
}

impl TransformOutput {
    /// Returns the number of nodes that were transformed.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Converts the output to a String.
    pub fn to_string(self) -> TransformResult<String> {
        String::from_utf8(self.data).map_err(|e| TransformError::Utf8(e.utf8_error()))
    }

    /// Writes the output to a writer.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<()> {
        writer.write_all(&self.data).map_err(TransformError::Io)
    }

    /// Returns the raw bytes of the output.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

// =============================================================================
// Deprecated API
// =============================================================================

impl<'a> StreamTransformer<'a> {
    /// Sets the XPath expression for matching elements to transform.
    #[deprecated(since = "0.4.0", note = "use .on(xpath, callback).run() instead")]
    pub fn xpath(mut self, xpath: &str) -> Self {
        // Store as a placeholder handler with no-op callback
        // The actual callback will be set by transform()
        self.handlers.push(Handler {
            xpath: XPathSource::String(xpath.to_string()),
            callback: Box::new(|_| {}),
        });
        self
    }

    /// Sets a pre-parsed XPath AST for matching elements to transform.
    #[deprecated(since = "0.4.0", note = "use .on() with string XPath instead")]
    pub fn xpath_ast(mut self, expr: Expr) -> Self {
        self.handlers.push(Handler {
            xpath: XPathSource::Ast(expr),
            callback: Box::new(|_| {}),
        });
        self
    }

    /// Sets the transform function and returns a builder for final operations.
    #[deprecated(since = "0.4.0", note = "use .on(xpath, callback).run() instead")]
    #[allow(clippy::should_implement_trait, deprecated)]
    pub fn transform<F>(mut self, transform_fn: F) -> StreamTransformBuilder<'a, F>
    where
        F: FnMut(&mut EditableNode),
    {
        // Get the xpath from the last handler (set by xpath())
        let xpath_source = if let Some(handler) = self.handlers.pop() {
            handler.xpath
        } else {
            XPathSource::String(String::new())
        };

        StreamTransformBuilder {
            input: self.input,
            xpath_source,
            namespaces: self.namespaces,
            transform_fn,
        }
    }
}

/// A consuming builder that captures the transform function.
#[deprecated(since = "0.4.0", note = "use StreamTransformer::on().run() instead")]
pub struct StreamTransformBuilder<'a, F> {
    input: &'a str,
    xpath_source: XPathSource,
    namespaces: HashMap<String, String>,
    transform_fn: F,
}

#[allow(deprecated)]
impl<'a, F> StreamTransformBuilder<'a, F>
where
    F: FnMut(&mut EditableNode),
{
    /// Writes the transformation result to a writer.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        if self
            .xpath_source
            .as_string()
            .map(|s| s.is_empty())
            .unwrap_or(true)
            && matches!(self.xpath_source, XPathSource::String(_))
        {
            return Err(TransformError::InvalidXPath(
                "No XPath expression provided".to_string(),
            ));
        }

        stream_transform_impl(
            self.input,
            &self.xpath_source,
            &self.namespaces,
            self.transform_fn,
            writer,
        )
    }

    /// Returns the transformation result as a String.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(self) -> TransformResult<String> {
        let mut output = Vec::new();
        self.write_to(&mut output)?;
        String::from_utf8(output).map_err(|e| TransformError::Utf8(e.utf8_error()))
    }
}

// =============================================================================
// Function API
// =============================================================================

/// Simple function API for streaming XML transformation.
///
/// This is a convenience function that handles both streamable and
/// non-streamable XPath expressions automatically.
///
/// # Arguments
///
/// * `input` - The input XML string
/// * `xpath` - XPath expression to match elements
/// * `transform_fn` - Function to transform matched elements
/// * `writer` - Output writer
///
/// # Returns
///
/// The number of nodes that were transformed.
///
/// # Example
///
/// ```rust
/// use fastxml::transform::stream_transform;
///
/// let xml = "<root><item>test</item></root>";
/// let mut output = Vec::new();
///
/// let count = stream_transform(xml, "//item", |node| {
///     node.set_attribute("processed", "true");
/// }, &mut output).unwrap();
///
/// assert_eq!(count, 1);
/// ```
pub fn stream_transform<W, F>(
    input: &str,
    xpath: &str,
    transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let source = XPathSource::String(xpath.to_string());
    stream_transform_impl(input, &source, &HashMap::new(), transform_fn, writer)
}

/// Streaming transform with namespace support.
pub fn stream_transform_with_namespaces<W, F>(
    input: &str,
    xpath: &str,
    namespaces: &HashMap<String, String>,
    transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let source = XPathSource::String(xpath.to_string());
    stream_transform_impl(input, &source, namespaces, transform_fn, writer)
}

fn stream_transform_impl<W, F>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    // Parse XPath expression
    let expr = xpath_source.parse()?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => {
            // Use single-pass streaming
            streaming::process_streaming(input, &streamable, namespaces, transform_fn, writer)
        }
        XPathAnalysis::NotStreamable(_reason) => {
            // Fall back to two-pass - requires string representation
            let xpath_str = xpath_source.as_string().ok_or_else(|| {
                TransformError::InvalidXPath(
                    "XPath AST without string representation cannot use fallback processor. \
                     Use a streamable XPath pattern or provide the expression as a string."
                        .to_string(),
                )
            })?;
            fallback::process_fallback(input, xpath_str, transform_fn, writer)
        }
    }
}

fn stream_for_each_impl<F>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    // Parse XPath expression
    let expr = xpath_source.parse()?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => {
            // Use single-pass streaming
            streaming::process_for_each(input, &streamable, namespaces, callback)
        }
        XPathAnalysis::NotStreamable(_reason) => {
            // Fall back to two-pass - requires string representation
            let xpath_str = xpath_source.as_string().ok_or_else(|| {
                TransformError::InvalidXPath(
                    "XPath AST without string representation cannot use fallback processor. \
                     Use a streamable XPath pattern or provide the expression as a string."
                        .to_string(),
                )
            })?;
            fallback::process_for_each(input, xpath_str, callback)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =============================================================================
    // New API Tests
    // =============================================================================

    #[test]
    fn test_on_single_handler() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item[@id='2']", |node| {
                node.set_attribute("modified", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"modified="true""#));
        assert!(result.contains("<item id=\"1\">A</item>"));
    }

    #[test]
    fn test_on_multiple_handlers() {
        let xml = r#"<root><item>A</item><other>B</other></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node| {
                node.set_attribute("type", "item");
            })
            .on("//other", |node| {
                node.set_attribute("type", "other");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"type="item""#));
        assert!(result.contains(r#"type="other""#));
    }

    #[test]
    fn test_for_each_single_handler() {
        let xml = r#"<root><item id="1"/><item id="2"/></root>"#;

        let mut ids = Vec::new();
        StreamTransformer::new(xml)
            .on("//item", |node| {
                if let Some(id) = node.get_attribute("id") {
                    ids.push(id);
                }
            })
            .for_each()
            .unwrap();

        assert_eq!(ids, vec!["1", "2"]);
    }

    #[test]
    fn test_for_each_multiple_handlers() {
        let xml = r#"<root><item>A</item><other>B</other></root>"#;

        let mut items = Vec::new();
        let mut others = Vec::new();

        StreamTransformer::new(xml)
            .on("//item", |node| {
                items.push(node.get_content().unwrap_or_default());
            })
            .on("//other", |node| {
                others.push(node.get_content().unwrap_or_default());
            })
            .for_each()
            .unwrap();

        assert_eq!(items, vec!["A"]);
        assert_eq!(others, vec!["B"]);
    }

    #[test]
    fn test_collect() {
        let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let contents: Vec<String> = StreamTransformer::new(xml)
            .collect("//item", |node| node.get_content().unwrap_or_default())
            .unwrap();

        assert_eq!(contents, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_collect_attributes() {
        let xml = r#"<root><item id="1"/><item id="2"/><item id="3"/></root>"#;

        let ids: Vec<String> = StreamTransformer::new(xml)
            .collect("//item", |node| {
                node.get_attribute("id").unwrap_or_default()
            })
            .unwrap();

        assert_eq!(ids, vec!["1", "2", "3"]);
    }

    #[test]
    fn test_run_no_handlers_error() {
        let xml = "<root/>";
        let result = StreamTransformer::new(xml).run();
        assert!(result.is_err());
    }

    #[test]
    fn test_for_each_no_handlers_error() {
        let xml = "<root/>";
        let result = StreamTransformer::new(xml).for_each();
        assert!(result.is_err());
    }

    #[test]
    fn test_transform_output_count() {
        let xml = r#"<root><item/><item/><item/></root>"#;

        let output = StreamTransformer::new(xml)
            .on("//item", |node| {
                node.set_attribute("found", "true");
            })
            .run()
            .unwrap();

        assert_eq!(output.count(), 3);
    }

    #[test]
    fn test_with_namespaces() {
        let xml = r#"<root xmlns:ns="http://example.com"><ns:item/></root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("ns", "http://example.com")
            .on("//ns:item", |node| {
                node.set_attribute("found", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"found="true""#));
    }

    #[test]
    fn test_remove_element() {
        let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//remove", |node| {
                node.remove();
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(!result.contains("<remove>"));
        assert!(result.contains("<keep>A</keep>"));
        assert!(result.contains("<keep>C</keep>"));
    }

    #[test]
    fn test_fallback_for_last() {
        let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

        let result = StreamTransformer::new(xml)
            .on("//item[last()]", |node| {
                node.set_attribute("last", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"last="true""#));
        assert_eq!(result.matches(r#"last="true""#).count(), 1);
    }

    // =============================================================================
    // Deprecated API Tests (ensure backward compatibility)
    // =============================================================================

    #[test]
    #[allow(deprecated)]
    fn test_deprecated_xpath_transform() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item[@id='2']")
            .transform(|node| {
                node.set_attribute("modified", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"modified="true""#));
    }

    // =============================================================================
    // Function API Tests
    // =============================================================================

    #[test]
    fn test_function_api() {
        let xml = "<root><item>test</item></root>";
        let mut output = Vec::new();

        let count = stream_transform(
            xml,
            "//item",
            |node| {
                node.set_attribute("processed", "true");
            },
            &mut output,
        )
        .unwrap();

        assert_eq!(count, 1);
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains(r#"processed="true""#));
    }
}
