//! StreamTransformer builder for XML transformations.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::XPathSource;

use super::context::TransformContext;
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::streaming;
use super::xpath_analyze::{self, NotStreamableReason, StreamableXPath, XPathAnalysis};
use super::{
    CollectMulti, FallbackMode, stream_for_each_impl, stream_for_each_with_callback,
    stream_transform_with_callback,
};

/// A handler that pairs an XPath expression with a callback function.
pub(crate) struct Handler<'a> {
    pub(crate) xpath: XPathSource,
    pub(crate) callback: HandlerCallback<'a>,
}

/// Type alias for simple transform callback.
pub(crate) type SimpleCallback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;

/// Type alias for context-aware transform callback.
pub(crate) type ContextCallback<'a> = Box<dyn FnMut(&mut EditableNode, &TransformContext) + 'a>;

/// Callback type for handlers.
pub(crate) enum HandlerCallback<'a> {
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
    pub(crate) input: &'a str,
    pub(crate) handlers: Vec<Handler<'a>>,
    pub(crate) namespaces: HashMap<String, String>,
    pub(crate) fallback_mode: FallbackMode,
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
    /// # Multiple Handler Behavior
    ///
    /// When multiple handlers are registered, they are processed in a single pass using
    /// **first-match-wins** strategy for nested elements:
    ///
    /// - Non-overlapping elements: All matching handlers are called
    /// - Nested elements: Only the **first** handler (in registration order) that matches
    ///   an outer element is called. Inner elements are NOT processed by other handlers
    ///   while the outer element's subtree is being transformed.
    ///
    /// ```rust
    /// # use fastxml::transform::StreamTransformer;
    /// let xml = r#"<root><outer><inner/></outer></root>"#;
    ///
    /// // handler1 matches //outer, handler2 matches //inner
    /// // Result: Only handler1 is called. handler2 is NOT called because
    /// // <inner> is inside the already-matched <outer> subtree.
    /// let result = StreamTransformer::new(xml)
    ///     .on("//outer", |node| node.set_attribute("matched", "outer"))
    ///     .on("//inner", |node| node.set_attribute("matched", "inner"))
    ///     .run()?
    ///     .to_string()?;
    ///
    /// assert!(result.contains(r#"matched="outer""#));
    /// assert!(!result.contains(r#"matched="inner""#)); // inner was NOT processed
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    ///
    /// If you need to process both container and child elements, use separate
    /// `StreamTransformer` instances or use `for_each()` which processes all matches.
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

    /// Collects values from multiple XPath expressions in a single pass.
    ///
    /// This is more efficient than calling `collect()` multiple times
    /// as it processes the XML document only once.
    ///
    /// # Example
    ///
    /// ```rust
    /// use fastxml::transform::{StreamTransformer, EditableNode};
    ///
    /// let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
    ///
    /// let (ids, contents): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
    ///     .collect_multi((
    ///         ("//item", |node: &mut EditableNode| node.get_attribute("id").unwrap_or_default()),
    ///         ("//item", |node: &mut EditableNode| node.get_content().unwrap_or_default()),
    ///     ))?;
    ///
    /// assert_eq!(ids, vec!["1", "2"]);
    /// assert_eq!(contents, vec!["A", "B"]);
    /// # Ok::<(), fastxml::transform::TransformError>(())
    /// ```
    pub fn collect_multi<C: CollectMulti<'a>>(self, collectors: C) -> TransformResult<C::Output> {
        collectors.collect(self.input, &self.namespaces, self.fallback_mode)
    }

    /// Internal: Execute transformation with all handlers
    ///
    /// Optimized to process multiple handlers in a single pass when all XPaths are streamable.
    ///
    /// # Single-Pass Strategy: First-Match-Wins
    ///
    /// When multiple handlers are registered and all XPaths are streamable, handlers are
    /// processed in a single XML parsing pass using first-match-wins strategy:
    ///
    /// - Only one handler can be "active" (processing a subtree) at a time
    /// - When a handler matches an element, other handlers cannot match elements
    ///   within that subtree until processing is complete
    /// - This prevents overlapping transformations and ensures predictable output
    ///
    /// **Example:** If handler1 matches `//outer` and handler2 matches `//inner`,
    /// and `<inner>` is nested inside `<outer>`, only handler1 will be called.
    /// handler2 will NOT process `<inner>` because it's inside handler1's active subtree.
    ///
    /// # Fallback Behavior
    ///
    /// - If any XPath is not streamable (e.g., uses `last()`), behavior depends on `fallback_mode`:
    ///   - `FallbackMode::Disabled` (default): Returns `NotStreamable` error
    ///   - `FallbackMode::Enabled`: Falls back to sequential multi-pass processing
    fn execute_transform<W: Write>(mut self, writer: &mut W) -> TransformResult<usize> {
        // Fast path: single handler
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            return stream_transform_with_callback(
                self.input,
                &handler.xpath,
                &self.namespaces,
                self.fallback_mode,
                handler.callback,
                writer,
            );
        }

        // Parse and analyze all XPaths
        let mut analyses: Vec<XPathAnalysis> = Vec::with_capacity(self.handlers.len());
        for handler in &self.handlers {
            let expr = handler.xpath.parse()?;
            analyses.push(xpath_analyze::analyze_xpath(&expr));
        }

        // Check if all are streamable
        let mut all_streamable = true;
        let mut first_not_streamable_reason: Option<(String, NotStreamableReason)> = None;

        for (i, analysis) in analyses.iter().enumerate() {
            if let XPathAnalysis::NotStreamable(reason) = analysis {
                all_streamable = false;
                if first_not_streamable_reason.is_none() {
                    let xpath_str = self.handlers[i]
                        .xpath
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    first_not_streamable_reason = Some((xpath_str, reason.clone()));
                }
                break;
            }
        }

        // If not all streamable and fallback is disabled, return error
        if !all_streamable {
            match self.fallback_mode {
                FallbackMode::Disabled => {
                    let (xpath, reason) = first_not_streamable_reason.unwrap();
                    return Err(TransformError::NotStreamable { xpath, reason });
                }
                FallbackMode::Enabled => {
                    // Fall back to sequential processing (multiple passes)
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
                        current_input = String::from_utf8(output)
                            .map_err(|e| TransformError::Utf8(e.utf8_error()))?;
                    }

                    writer
                        .write_all(current_input.as_bytes())
                        .map_err(TransformError::Io)?;
                    return Ok(total_count);
                }
            }
        }

        // All streamable - extract StreamableXPath from analyses
        let streamable_xpaths: Vec<StreamableXPath> = analyses
            .into_iter()
            .filter_map(|a| match a {
                XPathAnalysis::Streamable(s) => Some(s),
                _ => None,
            })
            .collect();

        // Check if any handler uses WithContext
        let has_context_handler = self
            .handlers
            .iter()
            .any(|h| matches!(h.callback, HandlerCallback::WithContext(_)));

        if has_context_handler {
            // Use context-aware multi processing
            type Callback<'a> = Box<dyn FnMut(&mut EditableNode, &TransformContext) + 'a>;
            let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

            for handler in self.handlers.iter_mut() {
                match &mut handler.callback {
                    HandlerCallback::Simple(f) => {
                        // Wrap simple callback to ignore context
                        callbacks.push(Box::new(move |node: &mut EditableNode, _ctx| f(node)));
                    }
                    HandlerCallback::WithContext(f) => {
                        callbacks.push(Box::new(move |node: &mut EditableNode, ctx| f(node, ctx)));
                    }
                }
            }

            // Build handlers array for the multi function
            let mut handler_pairs: Vec<streaming::MultiTransformHandlerWithContext<'_>> =
                streamable_xpaths
                    .iter()
                    .zip(callbacks.iter_mut())
                    .map(|(xpath, cb)| {
                        (
                            xpath,
                            cb.as_mut() as &mut dyn FnMut(&mut EditableNode, &TransformContext),
                        )
                    })
                    .collect();

            streaming::process_streaming_multi_with_context(
                self.input,
                &mut handler_pairs,
                &self.namespaces,
                writer,
            )
        } else {
            // Use simple multi processing (no context)
            type Callback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;
            let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

            for handler in self.handlers.iter_mut() {
                if let HandlerCallback::Simple(f) = &mut handler.callback {
                    callbacks.push(Box::new(move |node: &mut EditableNode| f(node)));
                }
            }

            // Build handlers array for the multi function
            let mut handler_pairs: Vec<streaming::MultiTransformHandler<'_>> = streamable_xpaths
                .iter()
                .zip(callbacks.iter_mut())
                .map(|(xpath, cb)| (xpath, cb.as_mut() as &mut dyn FnMut(&mut EditableNode)))
                .collect();

            streaming::process_streaming_multi(
                self.input,
                &mut handler_pairs,
                &self.namespaces,
                writer,
            )
        }
    }

    /// Internal: Execute for_each with all handlers
    ///
    /// Optimized to process multiple handlers in a single pass when all XPaths are streamable.
    fn execute_for_each(mut self) -> TransformResult<usize> {
        // Fast path: single handler
        if self.handlers.len() == 1 {
            let handler = self.handlers.remove(0);
            return stream_for_each_with_callback(
                self.input,
                &handler.xpath,
                &self.namespaces,
                self.fallback_mode,
                handler.callback,
            );
        }

        // Parse and analyze all XPaths
        let mut analyses: Vec<XPathAnalysis> = Vec::with_capacity(self.handlers.len());
        for handler in &self.handlers {
            let expr = handler.xpath.parse()?;
            analyses.push(xpath_analyze::analyze_xpath(&expr));
        }

        // Check if all are streamable
        let mut all_streamable = true;
        let mut first_not_streamable_reason: Option<(String, NotStreamableReason)> = None;

        for (i, analysis) in analyses.iter().enumerate() {
            if let XPathAnalysis::NotStreamable(reason) = analysis {
                all_streamable = false;
                if first_not_streamable_reason.is_none() {
                    let xpath_str = self.handlers[i]
                        .xpath
                        .as_string()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<ast>".to_string());
                    first_not_streamable_reason = Some((xpath_str, reason.clone()));
                }
                break;
            }
        }

        // If not all streamable and fallback is disabled, return error
        if !all_streamable {
            match self.fallback_mode {
                FallbackMode::Disabled => {
                    let (xpath, reason) = first_not_streamable_reason.unwrap();
                    return Err(TransformError::NotStreamable { xpath, reason });
                }
                FallbackMode::Enabled => {
                    // Fall back to sequential processing
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
                    return Ok(total_count);
                }
            }
        }

        // All streamable - extract StreamableXPath from analyses
        let streamable_xpaths: Vec<StreamableXPath> = analyses
            .into_iter()
            .filter_map(|a| match a {
                XPathAnalysis::Streamable(s) => Some(s),
                _ => None,
            })
            .collect();

        // Check if any handler uses WithContext
        let has_context_handler = self
            .handlers
            .iter()
            .any(|h| matches!(h.callback, HandlerCallback::WithContext(_)));

        if has_context_handler {
            // Use context-aware multi processing
            type Callback<'a> = Box<dyn FnMut(&mut EditableNode, &TransformContext) + 'a>;
            let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

            for handler in self.handlers.iter_mut() {
                match &mut handler.callback {
                    HandlerCallback::Simple(f) => {
                        // Wrap simple callback to ignore context
                        callbacks.push(Box::new(move |node: &mut EditableNode, _ctx| f(node)));
                    }
                    HandlerCallback::WithContext(f) => {
                        callbacks.push(Box::new(move |node: &mut EditableNode, ctx| f(node, ctx)));
                    }
                }
            }

            // Build handlers array for the multi function
            let mut handler_pairs: Vec<streaming::MultiHandlerWithContext<'_>> = streamable_xpaths
                .iter()
                .zip(callbacks.iter_mut())
                .map(|(xpath, cb)| {
                    (
                        xpath,
                        cb.as_mut() as &mut dyn FnMut(&mut EditableNode, &TransformContext),
                    )
                })
                .collect();

            streaming::process_for_each_multi_with_context(
                self.input,
                &mut handler_pairs,
                &self.namespaces,
            )
        } else {
            // Use simple multi processing (no context)
            type Callback<'a> = Box<dyn FnMut(&mut EditableNode) + 'a>;
            let mut callbacks: Vec<Callback<'_>> = Vec::with_capacity(self.handlers.len());

            for handler in self.handlers.iter_mut() {
                if let HandlerCallback::Simple(f) = &mut handler.callback {
                    callbacks.push(Box::new(move |node: &mut EditableNode| f(node)));
                }
            }

            // Build handlers array for the multi function
            let mut handler_pairs: Vec<streaming::MultiHandler<'_>> = streamable_xpaths
                .iter()
                .zip(callbacks.iter_mut())
                .map(|(xpath, cb)| (xpath, cb.as_mut() as &mut dyn FnMut(&mut EditableNode)))
                .collect();

            streaming::process_for_each_multi(self.input, &mut handler_pairs, &self.namespaces)
        }
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
