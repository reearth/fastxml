//! Callback dispatch functions for handler execution.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::builder::HandlerCallback;
use super::error::{TransformError, TransformResult};
use super::fallback;
use super::streaming;
use super::xpath_analyze::{self, XPathAnalysis};

pub(crate) fn stream_transform_with_callback<'a, W: Write>(
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
            HandlerCallback::Match(_) => Err(not_for_transform()),
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
                        fallback::process_fallback(input, xpath_str, |node| f(node), writer)
                    }
                    HandlerCallback::WithContext(mut f) => fallback::process_fallback_with_context(
                        input,
                        xpath_str,
                        |node, ctx| f(node, ctx),
                        writer,
                    ),
                    HandlerCallback::Match(_) => Err(not_for_transform()),
                }
            }
        },
    }
}

/// The error for a read-only match handler reaching a transform.
fn not_for_transform() -> TransformError {
    super::builder::invalid_operation(
        "on_match handlers only run with for_each, not when writing output",
    )
}

/// Runs one handler over the input for iteration, streaming when its XPath
/// is streamable and against a DOM of the input otherwise (fallback mode).
/// Stops after `limit` calls. Returns the number of callback calls.
pub(crate) fn stream_for_each_with_callback<'a>(
    input: &str,
    xpath_source: &XPathSource,
    namespaces: &HashMap<String, String>,
    fallback_mode: FallbackMode,
    mut callback: HandlerCallback<'a>,
    limit: Option<usize>,
) -> TransformResult<usize> {
    let expr = xpath_source.parse()?;
    match xpath_analyze::analyze_xpath(&expr) {
        XPathAnalysis::Streamable(streamable) => {
            let callback = match &mut callback {
                HandlerCallback::Simple(f) => streaming::DispatchCallback::Node(f.as_mut()),
                HandlerCallback::WithContext(f) => {
                    streaming::DispatchCallback::NodeWithContext(f.as_mut())
                }
                HandlerCallback::Match(f) => streaming::DispatchCallback::Match(f.as_mut()),
            };
            let mut handlers = [streaming::DispatchHandler {
                xpath: &streamable,
                callback,
                limit,
            }];
            streaming::dispatch(
                quick_xml::Reader::from_str(input),
                streaming::InputSource::Str(input),
                &mut handlers,
                namespaces,
            )
        }
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
                // The DOM fallback evaluates the expression as a string.
                let xpath_str = xpath_source.as_string().ok_or_else(|| {
                    TransformError::InvalidXPath(
                        "XPath AST without string representation cannot use fallback processor. \
                         Use a streamable XPath pattern or provide the expression as a string."
                            .to_string(),
                    )
                })?;
                match &mut callback {
                    HandlerCallback::Simple(f) => {
                        fallback::for_each_limited(input, xpath_str, &mut |node, _| f(node), limit)
                    }
                    HandlerCallback::WithContext(f) => {
                        fallback::for_each_limited(input, xpath_str, f.as_mut(), limit)
                    }
                    HandlerCallback::Match(f) => {
                        fallback::process_matches(input, xpath_str, f.as_mut(), limit)
                    }
                }
            }
        },
    }
}
