//! Streaming XML transformation with zero-copy output.
//!
//! This module provides APIs for transforming XML documents by selectively
//! modifying elements that match an XPath expression, while preserving
//! unchanged portions of the document with zero-copy efficiency.
//!
//! # Features
//!
//! - **Zero-copy output**: Unchanged portions of the input are written directly
//!   without copying or re-serialization
//! - **Selective DOM**: Only matched elements are converted to a modifiable DOM
//! - **Streaming**: Single-pass processing for compatible XPath expressions
//! - **Fallback**: Automatic two-pass processing for complex XPath patterns
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
//! ## Builder Pattern
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
//!
//! let result = StreamTransformer::new(xml)
//!     .xpath("//item[@id='2']")
//!     .transform(|node| {
//!         node.set_attribute("modified", "true");
//!     })
//!     .to_string()
//!     .unwrap();
//!
//! assert!(result.contains(r#"modified="true""#));
//! ```
//!
//! ## Function API
//!
//! ```rust
//! use fastxml::transform::stream_transform;
//!
//! let xml = r#"<root><item>Hello</item></root>"#;
//! let mut output = Vec::new();
//!
//! stream_transform(xml, "//item", |node| {
//!     node.set_attribute("processed", "true");
//! }, &mut output).unwrap();
//! ```
//!
//! ## Removing Elements
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;
//!
//! let result = StreamTransformer::new(xml)
//!     .xpath("//remove")
//!     .transform(|node| {
//!         node.remove();
//!     })
//!     .to_string()
//!     .unwrap();
//!
//! assert!(!result.contains("<remove>"));
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

use crate::xpath::parser::parse_xpath;

/// Builder for streaming XML transformations.
///
/// Provides a fluent API for configuring and executing XML transformations.
pub struct StreamTransformer<'a> {
    input: &'a str,
    xpath_expr: Option<String>,
    namespaces: HashMap<String, String>,
}

impl<'a> StreamTransformer<'a> {
    /// Creates a new transformer for the given XML input.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            xpath_expr: None,
            namespaces: HashMap::new(),
        }
    }

    /// Sets the XPath expression for matching elements to transform.
    pub fn xpath(mut self, xpath: &str) -> Self {
        self.xpath_expr = Some(xpath.to_string());
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
    /// ```
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root xmlns:gml="http://example.com/gml"><gml:point/></root>"#;
    /// let result = StreamTransformer::new(xml)
    ///     .namespaces([
    ///         ("gml", "http://example.com/gml"),
    ///         ("bldg", "http://example.com/bldg"),
    ///     ])
    ///     .xpath("//gml:point")
    ///     .transform(|node| {
    ///         node.set_attribute("found", "true");
    ///     })
    ///     .to_string()
    ///     .unwrap();
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
    ///
    /// # Example
    /// ```
    /// use fastxml::{parse, transform::StreamTransformer};
    ///
    /// let xml = r#"<root xmlns:gml="http://example.com/gml"><gml:point/></root>"#;
    /// let doc = parse(xml).unwrap();
    ///
    /// let result = StreamTransformer::new(xml)
    ///     .with_document_namespaces(&doc)
    ///     .xpath("//gml:point")
    ///     .transform(|node| {
    ///         node.set_attribute("found", "true");
    ///     })
    ///     .to_string()
    ///     .unwrap();
    /// ```
    pub fn with_document_namespaces(mut self, doc: &crate::document::XmlDocument) -> Self {
        self.namespaces.extend(doc.namespaces());
        self
    }

    /// Sets the transform function and returns a builder for final operations.
    pub fn transform<F>(self, transform_fn: F) -> StreamTransformBuilder<'a, F>
    where
        F: FnMut(&mut EditableNode),
    {
        StreamTransformBuilder {
            transformer: self,
            transform_fn,
        }
    }

    /// Iterates over matched elements without modifying the document.
    ///
    /// This is useful when you want to extract data from specific elements
    /// without building a full DOM tree.
    ///
    /// # Example
    /// ```
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
    ///
    /// let mut ids = Vec::new();
    /// StreamTransformer::new(xml)
    ///     .xpath("//item")
    ///     .for_each(|node| {
    ///         if let Some(id) = node.get_attribute("id") {
    ///             ids.push(id);
    ///         }
    ///     })
    ///     .unwrap();
    ///
    /// assert_eq!(ids, vec!["1", "2"]);
    /// ```
    pub fn for_each<F>(self, mut f: F) -> TransformResult<usize>
    where
        F: FnMut(&EditableNode),
    {
        let xpath_str = self.xpath_expr.as_ref().ok_or_else(|| {
            TransformError::InvalidXPath("No XPath expression provided".to_string())
        })?;

        stream_for_each_impl(self.input, xpath_str, &self.namespaces, |node| {
            f(node);
        })
    }

    /// Collects values from matched elements.
    ///
    /// This is useful when you want to extract and collect data from
    /// specific elements without building a full DOM tree.
    ///
    /// # Example
    /// ```
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
    ///
    /// let contents: Vec<String> = StreamTransformer::new(xml)
    ///     .xpath("//item")
    ///     .collect(|node| node.get_content().unwrap_or_default())
    ///     .unwrap();
    ///
    /// assert_eq!(contents, vec!["A", "B"]);
    /// ```
    pub fn collect<F, T>(self, mut f: F) -> TransformResult<Vec<T>>
    where
        F: FnMut(&EditableNode) -> T,
    {
        let mut results = Vec::new();
        self.for_each(|node| {
            results.push(f(node));
        })?;
        Ok(results)
    }
}

/// A consuming builder that captures the transform function.
pub struct StreamTransformBuilder<'a, F> {
    transformer: StreamTransformer<'a>,
    transform_fn: F,
}

impl<'a, F> StreamTransformBuilder<'a, F>
where
    F: FnMut(&mut EditableNode),
{
    /// Writes the transformation result to a writer.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        let xpath_str = self.transformer.xpath_expr.as_ref().ok_or_else(|| {
            TransformError::InvalidXPath("No XPath expression provided".to_string())
        })?;

        stream_transform_impl(
            self.transformer.input,
            xpath_str,
            &self.transformer.namespaces,
            self.transform_fn,
            writer,
        )
    }

    /// Returns the transformation result as a String.
    pub fn to_string(self) -> TransformResult<String> {
        let mut output = Vec::new();
        self.write_to(&mut output)?;
        String::from_utf8(output).map_err(|e| TransformError::Utf8(e.utf8_error()))
    }
}

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
    stream_transform_impl(input, xpath, &HashMap::new(), transform_fn, writer)
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
    stream_transform_impl(input, xpath, namespaces, transform_fn, writer)
}

fn stream_transform_impl<W, F>(
    input: &str,
    xpath_str: &str,
    namespaces: &HashMap<String, String>,
    transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    // Parse XPath expression
    let expr = parse_xpath(xpath_str).map_err(|e| TransformError::InvalidXPath(e.to_string()))?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => {
            // Use single-pass streaming
            streaming::process_streaming(input, &streamable, namespaces, transform_fn, writer)
        }
        XPathAnalysis::NotStreamable(_reason) => {
            // Fall back to two-pass
            fallback::process_fallback(input, xpath_str, transform_fn, writer)
        }
    }
}

fn stream_for_each_impl<F>(
    input: &str,
    xpath_str: &str,
    namespaces: &HashMap<String, String>,
    callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&EditableNode),
{
    // Parse XPath expression
    let expr = parse_xpath(xpath_str).map_err(|e| TransformError::InvalidXPath(e.to_string()))?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => {
            // Use single-pass streaming
            streaming::process_for_each(input, &streamable, namespaces, callback)
        }
        XPathAnalysis::NotStreamable(_reason) => {
            // Fall back to two-pass
            fallback::process_for_each(input, xpath_str, callback)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item[@id='2']")
            .transform(|node| {
                node.set_attribute("modified", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"modified="true""#));
        assert!(result.contains("<item id=\"1\">A</item>"));
    }

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

    #[test]
    fn test_remove_element() {
        let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//remove")
            .transform(|node| {
                node.remove();
            })
            .to_string()
            .unwrap();

        assert!(!result.contains("<remove>"));
        assert!(result.contains("<keep>A</keep>"));
        assert!(result.contains("<keep>C</keep>"));
    }

    #[test]
    fn test_multiple_matches() {
        let xml = "<root><item>1</item><item>2</item><item>3</item></root>";

        let mut count_total = 0;
        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node| {
                count_total += 1;
                node.set_attribute("n", &count_total.to_string());
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"n="1""#));
        assert!(result.contains(r#"n="2""#));
        assert!(result.contains(r#"n="3""#));
    }

    #[test]
    fn test_fallback_for_last() {
        let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

        let result = StreamTransformer::new(xml)
            .xpath("//item[last()]")
            .transform(|node| {
                node.set_attribute("last", "true");
            })
            .to_string()
            .unwrap();

        // Only the last item should have the attribute
        assert!(result.contains(r#"last="true""#));
        // Count occurrences - should be exactly one
        assert_eq!(result.matches(r#"last="true""#).count(), 1);
    }

    #[test]
    fn test_no_xpath_error() {
        let xml = "<root/>";

        let result = StreamTransformer::new(xml).transform(|_| {}).to_string();

        assert!(result.is_err());
    }

    #[test]
    fn test_preserve_structure() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <item id="1">A</item>
  <item id="2">B</item>
</root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item[@id='2']")
            .transform(|node| {
                node.set_attribute("modified", "true");
            })
            .to_string()
            .unwrap();

        // Should preserve XML declaration and whitespace around non-matched elements
        assert!(result.starts_with("<?xml"));
        assert!(result.contains("<item id=\"1\">A</item>"));
    }

    #[test]
    fn test_for_each() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let mut ids = Vec::new();
        let count = StreamTransformer::new(xml)
            .xpath("//item")
            .for_each(|node| {
                if let Some(id) = node.get_attribute("id") {
                    ids.push(id);
                }
            })
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[test]
    fn test_collect() {
        let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let contents: Vec<String> = StreamTransformer::new(xml)
            .xpath("//item")
            .collect(|node| node.get_content().unwrap_or_default())
            .unwrap();

        assert_eq!(contents, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_for_each_with_last_fallback() {
        let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let mut contents = Vec::new();
        let count = StreamTransformer::new(xml)
            .xpath("//item[last()]")
            .for_each(|node| {
                if let Some(content) = node.get_content() {
                    contents.push(content);
                }
            })
            .unwrap();

        assert_eq!(count, 1);
        assert_eq!(contents, vec!["C"]);
    }
}
