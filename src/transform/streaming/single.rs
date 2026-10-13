//! Single-handler streaming processing functions.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;

use super::super::context::TransformContext;
use super::super::editable::EditableNode;
use super::super::error::TransformResult;
use super::super::xpath_analyze::StreamableXPath;
use super::rewrite::{RewriteCallback, RewriteHandler, rewrite_str};
use super::{DispatchCallback, DispatchHandler, InputSource, dispatch};

/// Processes XML with streaming transformation.
pub fn process_streaming<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut handlers = [RewriteHandler {
        xpath,
        callback: RewriteCallback::Node(&mut transform_fn),
    }];
    rewrite_str(input, &mut handlers, namespaces, writer)
}

/// Processes XML with streaming transformation and context.
pub fn process_streaming_with_context<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode, &TransformContext),
{
    let mut handlers = [RewriteHandler {
        xpath,
        callback: RewriteCallback::NodeWithContext(&mut transform_fn),
    }];
    rewrite_str(input, &mut handlers, namespaces, writer)
}

/// Processes XML with streaming iteration (no transformation output).
pub fn process_for_each<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    let mut handlers = [DispatchHandler {
        xpath,
        callback: DispatchCallback::Node(&mut callback),
        limit: None,
    }];
    dispatch(
        Reader::from_str(input),
        InputSource::Str(input),
        &mut handlers,
        namespaces,
    )
}

/// Processes XML with streaming iteration and context (no transformation output).
pub fn process_for_each_with_context<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode, &TransformContext),
{
    let mut handlers = [DispatchHandler {
        xpath,
        callback: DispatchCallback::NodeWithContext(&mut callback),
        limit: None,
    }];
    dispatch(
        Reader::from_str(input),
        InputSource::Str(input),
        &mut handlers,
        namespaces,
    )
}
