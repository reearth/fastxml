//! Function-based API for streaming XML transformation.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::fallback;
use super::streaming;
use super::xpath_analyze::{self, XPathAnalysis};

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
#[doc(hidden)]
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
#[doc(hidden)]
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
#[doc(hidden)]
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

pub(crate) fn stream_transform_impl<W, F>(
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

pub(crate) fn stream_for_each_impl<F>(
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
