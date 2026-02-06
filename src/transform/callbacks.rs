//! Callback dispatch functions for handler execution.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::builder::HandlerCallback;
use super::context::TransformContext;
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

pub(crate) fn stream_for_each_with_callback<'a>(
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
