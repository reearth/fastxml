//! Function-based API for streaming XML transformation.

use std::collections::HashMap;

use crate::xpath::XPathSource;

use super::FallbackMode;
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::fallback;
use super::streaming;
use super::xpath_analyze::{self, XPathAnalysis};

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
