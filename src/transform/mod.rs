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

pub mod context;
pub mod editable;
pub mod error;
pub mod fallback;
pub mod span;
pub mod streaming;
pub mod xpath_analyze;

use std::collections::HashMap;
use std::io::Write;

pub use context::{AncestorInfo, TransformContext};
pub use editable::{EditableNode, EditableNodeBuilder, EditableNodeRef, Modification, NewNode};
pub use error::{TransformError, TransformResult};
pub use span::ByteSpan;
pub use xpath_analyze::{
    AttributePredicate, NotStreamableReason, PositionPredicate, StreamableStep, StreamableXPath,
    XPathAnalysis,
};

// Re-export XPath types for convenience
pub use crate::xpath::{Expr, XPathSource};

/// Controls how non-streamable XPath expressions are handled.
///
/// By default, non-streamable XPath expressions will return an error.
/// This prevents unexpected memory usage from automatic fallback to
/// two-pass processing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FallbackMode {
    /// Return an error for non-streamable XPath expressions (default).
    ///
    /// Use this mode when you want to ensure streaming processing
    /// and avoid unexpected memory usage.
    #[default]
    Disabled,
    /// Automatically use two-pass processing for non-streamable XPath.
    ///
    /// **Warning**: This may load the entire document into memory.
    /// Only use this if you understand the memory implications.
    Enabled,
}

/// A handler that pairs an XPath expression with a callback function.
struct Handler<'a> {
    xpath: XPathSource,
    callback: HandlerCallback<'a>,
}

/// Type alias for simple transform callback.
type SimpleCallback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;

/// Type alias for context-aware transform callback.
type ContextCallback<'a> = Box<dyn FnMut(&mut EditableNode, &TransformContext) + 'a>;

/// Callback type for handlers.
enum HandlerCallback<'a> {
    /// Simple callback without context
    Simple(SimpleCallback<'a>),
    /// Callback with transform context
    WithContext(ContextCallback<'a>),
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
    fallback_mode: FallbackMode,
}

impl<'a> StreamTransformer<'a> {
    /// Creates a new transformer for the given XML input.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            handlers: Vec::new(),
            namespaces: HashMap::new(),
            fallback_mode: FallbackMode::default(),
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
            callback: HandlerCallback::Simple(Box::new(callback)),
        });
        self
    }

    /// Registers an XPath expression with a context-aware callback function.
    ///
    /// The callback receives both the matched node and a `TransformContext` that
    /// provides access to ancestor elements, position information, and depth.
    ///
    /// This is useful when you need to generate IDs based on the element's position
    /// in the document hierarchy (e.g., xmlParentId).
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><items><item id="1"/><item id="2"/></items></root>"#;
    ///
    /// let result = StreamTransformer::new(xml)
    ///     .on_with_context("//item", |node, ctx| {
    ///         // Generate path-based ID: "root/items/item[1]" or "root/items/item[2]"
    ///         let path_id = ctx.path_id();
    ///         node.set_attribute("path", &format!("{}/item[{}]", path_id, ctx.position()));
    ///
    ///         // Access parent attributes
    ///         if let Some(parent) = ctx.parent() {
    ///             node.set_attribute("parent_name", &parent.name);
    ///         }
    ///     })
    ///     .run()?
    ///     .to_string()?;
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn on_with_context<F>(mut self, xpath: &str, callback: F) -> Self
    where
        F: FnMut(&mut EditableNode, &TransformContext) + 'a,
    {
        self.handlers.push(Handler {
            xpath: XPathSource::String(xpath.to_string()),
            callback: HandlerCallback::WithContext(Box::new(callback)),
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

    /// Automatically extracts and registers namespaces from the root element.
    ///
    /// This is a lightweight operation that doesn't require full DOM parsing.
    /// It reads only the first element's xmlns attributes to extract namespaces.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
    ///     <gml:point id="1"/>
    /// </root>"#;
    ///
    /// // Without with_root_namespaces, you'd need to manually register:
    /// // .namespace("gml", "http://www.opengis.net/gml")
    ///
    /// let result = StreamTransformer::new(xml)
    ///     .with_root_namespaces()?
    ///     .on("//gml:point", |node| {
    ///         node.set_attribute("found", "true");
    ///     })
    ///     .run()?
    ///     .to_string()?;
    ///
    /// assert!(result.contains(r#"found="true""#));
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn with_root_namespaces(mut self) -> TransformResult<Self> {
        let ns_map = crate::namespace::extract_root_namespaces(self.input)
            .map_err(|e| TransformError::XmlParse(e.to_string()))?;
        for (prefix, uri) in ns_map {
            // Register all namespaces including default (empty prefix)
            self.namespaces.insert(prefix, uri);
        }
        Ok(self)
    }

    /// Enables automatic fallback to two-pass processing for non-streamable XPath.
    ///
    /// By default, non-streamable XPath expressions (e.g., those using `last()`,
    /// backward axes, or complex predicates) will return an error. This method
    /// enables automatic fallback to two-pass processing for such expressions.
    ///
    /// **Warning**: Two-pass processing loads the entire document into memory.
    /// Only use this if you understand the memory implications.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::StreamTransformer;
    ///
    /// let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;
    ///
    /// // Without allow_fallback(), this would return an error
    /// let result = StreamTransformer::new(xml)
    ///     .allow_fallback()
    ///     .on("//item[last()]", |node| {
    ///         node.set_attribute("is_last", "true");
    ///     })
    ///     .run()?
    ///     .to_string()?;
    ///
    /// assert!(result.contains(r#"is_last="true""#));
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn allow_fallback(mut self) -> Self {
        self.fallback_mode = FallbackMode::Enabled;
        self
    }

    /// Sets the fallback mode explicitly.
    ///
    /// See [`FallbackMode`] for available options.
    pub fn fallback_mode(mut self, mode: FallbackMode) -> Self {
        self.fallback_mode = mode;
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

        stream_for_each_impl(
            self.input,
            &xpath_source,
            &self.namespaces,
            self.fallback_mode,
            |node| {
                results.push(f(node));
            },
        )?;

        Ok(results)
    }

    /// Internal: Execute transformation with all handlers
    fn execute_transform<W: Write>(mut self, writer: &mut W) -> TransformResult<usize> {
        // For now, we process handlers sequentially
        // TODO: optimize for multiple handlers in a single pass
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            stream_transform_with_callback(
                self.input,
                &handler.xpath,
                &self.namespaces,
                self.fallback_mode,
                handler.callback,
                writer,
            )
        } else {
            // Multiple handlers: process sequentially, passing output to next
            let mut current_input = self.input.to_string();
            let mut total_count = 0;

            for handler in self.handlers {
                let mut output = Vec::new();
                let count = stream_transform_with_callback(
                    &current_input,
                    &handler.xpath,
                    &self.namespaces,
                    self.fallback_mode,
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
    fn execute_for_each(self) -> TransformResult<usize> {
        let mut total_count = 0;

        for handler in self.handlers {
            let count = stream_for_each_with_callback(
                self.input,
                &handler.xpath,
                &self.namespaces,
                self.fallback_mode,
                handler.callback,
            )?;
            total_count += count;
        }

        Ok(total_count)
    }
}

/// Output from a transformation operation.
///
/// Contains the transformed XML data and metadata about the transformation.
#[derive(Debug)]
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
            callback: HandlerCallback::Simple(Box::new(|_| {})),
        });
        self
    }

    /// Sets a pre-parsed XPath AST for matching elements to transform.
    #[deprecated(since = "0.4.0", note = "use .on() with string XPath instead")]
    pub fn xpath_ast(mut self, expr: Expr) -> Self {
        self.handlers.push(Handler {
            xpath: XPathSource::Ast(expr),
            callback: HandlerCallback::Simple(Box::new(|_| {})),
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

        // Deprecated API maintains backward compatibility with fallback enabled
        stream_transform_impl(
            self.input,
            &self.xpath_source,
            &self.namespaces,
            FallbackMode::Enabled,
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
// XPath Analysis API
// =============================================================================

/// Analyzes an XPath expression to determine if it can be processed
/// in a single streaming pass.
///
/// This is useful for checking whether an XPath expression will use
/// efficient streaming or require fallback to two-pass processing.
///
/// # Example
///
/// ```rust
/// use fastxml::transform::{analyze_xpath_str, XPathAnalysis};
///
/// match analyze_xpath_str("//item[@id='1']") {
///     Ok(XPathAnalysis::Streamable(s)) => {
///         println!("Streamable with {} steps", s.steps.len());
///     }
///     Ok(XPathAnalysis::NotStreamable(reason)) => {
///         println!("Not streamable: {}", reason);
///     }
///     Err(e) => println!("Parse error: {}", e),
/// }
/// ```
pub fn analyze_xpath_str(xpath: &str) -> TransformResult<XPathAnalysis> {
    let expr = crate::xpath::parser::parse_xpath(xpath)
        .map_err(|e| TransformError::InvalidXPath(e.to_string()))?;
    Ok(xpath_analyze::analyze_xpath(&expr))
}

/// Returns true if the XPath can be processed in streaming mode.
///
/// This is a convenience function for quickly checking streamability.
///
/// # Example
///
/// ```rust
/// use fastxml::transform::is_streamable;
///
/// assert!(is_streamable("//item[@id='1']"));
/// assert!(!is_streamable("//item[last()]"));
/// ```
pub fn is_streamable(xpath: &str) -> bool {
    analyze_xpath_str(xpath)
        .map(|a| matches!(a, XPathAnalysis::Streamable(_)))
        .unwrap_or(false)
}

/// Returns the reason why an XPath is not streamable, if any.
///
/// Returns `None` if the XPath is streamable or if parsing fails.
///
/// # Example
///
/// ```rust
/// use fastxml::transform::get_not_streamable_reason;
///
/// if let Some(reason) = get_not_streamable_reason("//item[last()]") {
///     println!("Not streamable: {}", reason);
/// }
/// ```
pub fn get_not_streamable_reason(xpath: &str) -> Option<NotStreamableReason> {
    analyze_xpath_str(xpath).ok().and_then(|a| match a {
        XPathAnalysis::NotStreamable(r) => Some(r),
        _ => None,
    })
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
    stream_transform_impl(
        input,
        &source,
        &HashMap::new(),
        FallbackMode::Disabled,
        transform_fn,
        writer,
    )
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
    stream_transform_impl(
        input,
        &source,
        namespaces,
        FallbackMode::Disabled,
        transform_fn,
        writer,
    )
}

/// Streaming transform with fallback enabled for non-streamable XPath.
///
/// **Warning**: This may load the entire document into memory for
/// non-streamable XPath expressions.
pub fn stream_transform_with_fallback<W, F>(
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
    stream_transform_impl(
        input,
        &source,
        &HashMap::new(),
        FallbackMode::Enabled,
        transform_fn,
        writer,
    )
}

fn stream_transform_impl<W, F>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
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
        XPathAnalysis::NotStreamable(reason) => {
            match fallback_mode {
                FallbackMode::Disabled => {
                    let xpath_str = xpath_source
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    Err(TransformError::NotStreamable {
                        xpath: xpath_str,
                        reason,
                    })
                }
                FallbackMode::Enabled => {
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
    }
}

fn stream_for_each_impl<F>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
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
        XPathAnalysis::NotStreamable(reason) => {
            match fallback_mode {
                FallbackMode::Disabled => {
                    let xpath_str = xpath_source
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    Err(TransformError::NotStreamable {
                        xpath: xpath_str,
                        reason,
                    })
                }
                FallbackMode::Enabled => {
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
    }
}

// =============================================================================
// HandlerCallback dispatch functions
// =============================================================================

fn stream_transform_with_callback<'a, W: Write>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
    callback: HandlerCallback<'a>,
    writer: &mut W,
) -> TransformResult<usize> {
    // Parse XPath expression
    let expr = xpath_source.parse()?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => match callback {
            HandlerCallback::Simple(mut f) => {
                streaming::process_streaming(input, &streamable, namespaces, |node| f(node), writer)
            }
            HandlerCallback::WithContext(mut f) => streaming::process_streaming_with_context(
                input,
                &streamable,
                namespaces,
                |node, ctx| f(node, ctx),
                writer,
            ),
        },
        XPathAnalysis::NotStreamable(reason) => match fallback_mode {
            FallbackMode::Disabled => {
                let xpath_str = xpath_source
                    .as_string()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<ast>".to_string());
                Err(TransformError::NotStreamable {
                    xpath: xpath_str,
                    reason,
                })
            }
            FallbackMode::Enabled => {
                // Fall back to two-pass - requires string representation
                // Note: WithContext callbacks are not supported in fallback mode
                // because the fallback processor uses libxml which doesn't track context
                let xpath_str = xpath_source.as_string().ok_or_else(|| {
                    TransformError::InvalidXPath(
                        "XPath AST without string representation cannot use fallback processor. \
                         Use a streamable XPath pattern or provide the expression as a string."
                            .to_string(),
                    )
                })?;

                match callback {
                    HandlerCallback::Simple(mut f) => {
                        fallback::process_fallback(input, xpath_str, |node| f(node), writer)
                    }
                    HandlerCallback::WithContext(mut f) => {
                        // Fallback mode doesn't support context, create an empty context
                        let empty_ctx = TransformContext::new(vec![], 0, 0);
                        fallback::process_fallback(
                            input,
                            xpath_str,
                            |node| f(node, &empty_ctx),
                            writer,
                        )
                    }
                }
            }
        },
    }
}

fn stream_for_each_with_callback<'a>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
    callback: HandlerCallback<'a>,
) -> TransformResult<usize> {
    // Parse XPath expression
    let expr = xpath_source.parse()?;

    // Analyze for streamability
    let analysis = xpath_analyze::analyze_xpath(&expr);

    match analysis {
        XPathAnalysis::Streamable(streamable) => match callback {
            HandlerCallback::Simple(mut f) => {
                streaming::process_for_each(input, &streamable, namespaces, |node| f(node))
            }
            HandlerCallback::WithContext(mut f) => streaming::process_for_each_with_context(
                input,
                &streamable,
                namespaces,
                |node, ctx| f(node, ctx),
            ),
        },
        XPathAnalysis::NotStreamable(reason) => match fallback_mode {
            FallbackMode::Disabled => {
                let xpath_str = xpath_source
                    .as_string()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<ast>".to_string());
                Err(TransformError::NotStreamable {
                    xpath: xpath_str,
                    reason,
                })
            }
            FallbackMode::Enabled => {
                // Fall back to two-pass - requires string representation
                let xpath_str = xpath_source.as_string().ok_or_else(|| {
                    TransformError::InvalidXPath(
                        "XPath AST without string representation cannot use fallback processor. \
                         Use a streamable XPath pattern or provide the expression as a string."
                            .to_string(),
                    )
                })?;

                match callback {
                    HandlerCallback::Simple(mut f) => {
                        fallback::process_for_each(input, xpath_str, |node| f(node))
                    }
                    HandlerCallback::WithContext(mut f) => {
                        // Fallback mode doesn't support context, create an empty context
                        let empty_ctx = TransformContext::new(vec![], 0, 0);
                        fallback::process_for_each(input, xpath_str, |node| f(node, &empty_ctx))
                    }
                }
            }
        },
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
    fn test_fallback_for_last_with_allow_fallback() {
        let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

        let result = StreamTransformer::new(xml)
            .allow_fallback()
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

    #[test]
    fn test_not_streamable_error_without_fallback() {
        let xml = "<root><item>A</item><item>B</item></root>";

        let result = StreamTransformer::new(xml)
            .on("//item[last()]", |node| {
                node.set_attribute("last", "true");
            })
            .run();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, TransformError::NotStreamable { .. }));
        let err_msg = format!("{}", err);
        assert!(err_msg.contains("last()"));
        assert!(err_msg.contains("allow_fallback()"));
    }

    #[test]
    fn test_fallback_mode_enabled() {
        let xml = "<root><item>A</item><item>B</item></root>";

        let result = StreamTransformer::new(xml)
            .fallback_mode(FallbackMode::Enabled)
            .on("//item[last()]", |node| {
                node.set_attribute("last", "true");
            })
            .run();

        assert!(result.is_ok());
    }

    #[test]
    fn test_with_root_namespaces() {
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <gml:point id="1"/>
        </root>"#;

        let result = StreamTransformer::new(xml)
            .with_root_namespaces()
            .unwrap()
            .on("//gml:point", |node| {
                node.set_attribute("found", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"found="true""#));
    }

    #[test]
    fn test_with_root_namespaces_multiple() {
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns:uro="http://example.com/uro">
            <gml:point/><uro:item/>
        </root>"#;

        let mut found_gml = false;
        let mut found_uro = false;

        StreamTransformer::new(xml)
            .with_root_namespaces()
            .unwrap()
            .on("//gml:point", |_| found_gml = true)
            .on("//uro:item", |_| found_uro = true)
            .for_each()
            .unwrap();

        assert!(found_gml);
        assert!(found_uro);
    }

    // =============================================================================
    // Context API Tests
    // =============================================================================

    #[test]
    fn test_on_with_context_parent() {
        let xml = r#"<root><items id="list1"><item>A</item><item>B</item></items></root>"#;

        let mut parent_names = Vec::new();
        let mut parent_ids = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//item", |_node, ctx| {
                if let Some(parent) = ctx.parent() {
                    parent_names.push(parent.name.clone());
                    if let Some(id) = parent.attributes.get("id") {
                        parent_ids.push(id.clone());
                    }
                }
            })
            .for_each()
            .unwrap();

        assert_eq!(parent_names, vec!["items", "items"]);
        assert_eq!(parent_ids, vec!["list1", "list1"]);
    }

    #[test]
    fn test_on_with_context_position() {
        let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let mut positions = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//item", |_node, ctx| {
                positions.push(ctx.position());
            })
            .for_each()
            .unwrap();

        assert_eq!(positions, vec![1, 2, 3]);
    }

    #[test]
    fn test_on_with_context_depth() {
        let xml = r#"<root><level1><level2><target/></level2></level1></root>"#;

        let mut depths = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//target", |_node, ctx| {
                depths.push(ctx.depth());
            })
            .for_each()
            .unwrap();

        // root=1, level1=2, level2=3, target=4
        assert_eq!(depths, vec![4]);
    }

    #[test]
    fn test_on_with_context_ancestors() {
        let xml = r#"<root><a><b><target/></b></a></root>"#;

        let mut ancestor_names = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//target", |_node, ctx| {
                ancestor_names = ctx.ancestors().iter().map(|a| a.name.clone()).collect();
            })
            .for_each()
            .unwrap();

        assert_eq!(ancestor_names, vec!["root", "a", "b"]);
    }

    #[test]
    fn test_on_with_context_path_id() {
        let xml = r#"<root><items><item/><item/></items><items><item/></items></root>"#;

        let mut paths = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//item", |_node, ctx| {
                paths.push(ctx.path_id());
            })
            .for_each()
            .unwrap();

        // First items group: position 1
        // Second items group: position 2
        assert_eq!(paths, vec!["root/items", "root/items", "root/items[2]"]);
    }

    #[test]
    fn test_on_with_context_transform() {
        let xml = r#"<root><items id="list1"><item/><item/></items></root>"#;

        let result = StreamTransformer::new(xml)
            .on_with_context("//item", |node, ctx| {
                let path = ctx.path_id();
                let pos = ctx.position();
                node.set_attribute("path", &format!("{}/item[{}]", path, pos));

                if let Some(parent_id) = ctx.parent_attribute("id") {
                    node.set_attribute("parent_id", parent_id);
                }
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"path="root/items/item[1]""#));
        assert!(result.contains(r#"path="root/items/item[2]""#));
        assert!(result.contains(r#"parent_id="list1""#));
    }

    #[test]
    fn test_on_with_context_empty_element() {
        let xml = r#"<root><item/></root>"#;

        let mut depths = Vec::new();
        let mut positions = Vec::new();

        StreamTransformer::new(xml)
            .on_with_context("//item", |_node, ctx| {
                depths.push(ctx.depth());
                positions.push(ctx.position());
            })
            .for_each()
            .unwrap();

        assert_eq!(depths, vec![2]);
        assert_eq!(positions, vec![1]);
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
    // XPath Analysis API Tests
    // =============================================================================

    #[test]
    fn test_analyze_xpath_str_streamable() {
        let result = analyze_xpath_str("//item[@id='1']").unwrap();
        assert!(matches!(result, XPathAnalysis::Streamable(_)));
    }

    #[test]
    fn test_analyze_xpath_str_not_streamable() {
        let result = analyze_xpath_str("//item[last()]").unwrap();
        assert!(matches!(result, XPathAnalysis::NotStreamable(_)));
    }

    #[test]
    fn test_analyze_xpath_str_parse_error() {
        let result = analyze_xpath_str("//[invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_streamable_true() {
        assert!(is_streamable("//item"));
        assert!(is_streamable("//item[@id='1']"));
        assert!(is_streamable("/root/items/item"));
    }

    #[test]
    fn test_is_streamable_false() {
        assert!(!is_streamable("//item[last()]"));
        assert!(!is_streamable("//item/parent::*"));
        assert!(!is_streamable("//a | //b"));
    }

    #[test]
    fn test_get_not_streamable_reason_some() {
        let reason = get_not_streamable_reason("//item[last()]");
        assert!(reason.is_some());
        assert!(matches!(reason.unwrap(), NotStreamableReason::UsesLast));
    }

    #[test]
    fn test_get_not_streamable_reason_none() {
        let reason = get_not_streamable_reason("//item[@id='1']");
        assert!(reason.is_none());
    }

    #[test]
    fn test_not_streamable_reason_display() {
        let reason = NotStreamableReason::UsesLast;
        let display = format!("{}", reason);
        assert!(display.contains("last()"));
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

    // =============================================================================
    // Namespace URI Matching Tests
    // =============================================================================

    #[test]
    fn test_namespace_uri_matching() {
        // Test that namespace-uri() and local-name() predicates work for matching
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <gml:feature id="1">Test</gml:feature>
        </root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on(
                "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
                |node| {
                    node.set_attribute("matched", "true");
                },
            )
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"matched="true""#));
    }

    #[test]
    fn test_namespace_uri_matching_different_prefix() {
        // Test that namespace-uri() matches elements with different prefixes but same URI
        let xml = r#"<root xmlns:g="http://www.opengis.net/gml">
            <g:feature id="1">Test</g:feature>
        </root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("g", "http://www.opengis.net/gml")
            .on(
                "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
                |node| {
                    node.set_attribute("matched", "true");
                },
            )
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Should match even though the prefix is 'g' instead of 'gml'
        assert!(result.contains(r#"matched="true""#));
    }

    #[test]
    fn test_namespace_uri_no_match_wrong_uri() {
        // Test that namespace-uri() doesn't match when URI is different
        let xml = r#"<root xmlns:gml="http://different.uri.com">
            <gml:feature id="1">Test</gml:feature>
        </root>"#;

        let mut matched = false;

        StreamTransformer::new(xml)
            .namespace("gml", "http://different.uri.com")
            .on(
                "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
                |_| {
                    matched = true;
                },
            )
            .for_each()
            .unwrap();

        // Should NOT match because the URI is different
        assert!(!matched);
    }

    #[test]
    fn test_local_name_only_matching() {
        // Test that local-name() alone matches elements regardless of prefix
        let xml = r#"<root><item id="1">A</item><ns:item xmlns:ns="http://example.com" id="2">B</ns:item></root>"#;

        let mut matched_ids = Vec::new();

        StreamTransformer::new(xml)
            .namespace("ns", "http://example.com")
            .on("//*[local-name()='item']", |node| {
                if let Some(id) = node.get_attribute("id") {
                    matched_ids.push(id);
                }
            })
            .for_each()
            .unwrap();

        // Should match both items regardless of namespace
        assert_eq!(matched_ids, vec!["1", "2"]);
    }
}
