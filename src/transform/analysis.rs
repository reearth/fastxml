//! XPath analysis API for determining streamability.

use super::error::{TransformError, TransformResult};
use super::xpath_analyze::{self, NotStreamableReason, XPathAnalysis};

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
