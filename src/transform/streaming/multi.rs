//! Multi-handler streaming processing functions.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;

use super::super::error::TransformResult;
use super::rewrite::{RewriteCallback, RewriteHandler, rewrite_str};
use super::{DispatchCallback, DispatchHandler, InputSource, dispatch};
use super::{
    MultiHandler, MultiHandlerWithContext, MultiTransformHandler, MultiTransformHandlerWithContext,
};

/// Processes XML with streaming iteration for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_for_each` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
pub fn process_for_each_multi<'a>(
    input: &str,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut handlers: Vec<DispatchHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| DispatchHandler {
            xpath,
            callback: DispatchCallback::Node(&mut **callback),
            limit: None,
        })
        .collect();
    dispatch(
        Reader::from_str(input),
        InputSource::Str(input),
        &mut handlers,
        namespaces,
    )
}

/// Processes XML with streaming iteration and context for multiple XPath handlers in a single pass.
pub fn process_for_each_multi_with_context<'a>(
    input: &str,
    handlers: &mut [MultiHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut handlers: Vec<DispatchHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| DispatchHandler {
            xpath,
            callback: DispatchCallback::NodeWithContext(&mut **callback),
            limit: None,
        })
        .collect();
    dispatch(
        Reader::from_str(input),
        InputSource::Str(input),
        &mut handlers,
        namespaces,
    )
}

/// Processes XML with streaming transformation for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
///
/// # Strategy: First-Match-Wins
///
/// When multiple handlers could match nested elements, only the **first matching handler**
/// (in registration order) is active at a time. While a handler is processing an element's
/// subtree, other handlers cannot match elements within that subtree.
pub fn process_streaming_multi<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut handlers: Vec<RewriteHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| RewriteHandler {
            xpath,
            callback: RewriteCallback::Node(&mut **callback),
        })
        .collect();
    rewrite_str(input, &mut handlers, namespaces, writer)
}

/// Processes XML with streaming transformation and context for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming_with_context` multiple times.
///
/// # Strategy: First-Match-Wins
///
/// Uses the same first-match-wins strategy as [`process_streaming_multi`].
pub fn process_streaming_multi_with_context<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut handlers: Vec<RewriteHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| RewriteHandler {
            xpath,
            callback: RewriteCallback::NodeWithContext(&mut **callback),
        })
        .collect();
    rewrite_str(input, &mut handlers, namespaces, writer)
}
