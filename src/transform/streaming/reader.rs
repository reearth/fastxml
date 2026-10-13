//! Reader-based streaming processing functions.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use quick_xml::Reader;

use super::super::editable::EditableNode;
use super::super::error::TransformResult;
use super::super::xpath_analyze::StreamableXPath;
use super::rewrite::{RewriteCallback, RewriteHandler, rewrite_reader};
use super::{DispatchCallback, DispatchHandler, InputSource, dispatch};
use super::{MultiHandler, MultiTransformHandler};

/// Processes XML from a reader with streaming iteration (no transformation output).
///
/// Unlike `process_for_each`, this function reads from a `BufRead` source instead of
/// requiring the entire XML string in memory. This is important for processing large
/// XML files without excessive memory usage.
pub fn process_for_each_from_reader<R, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    R: BufRead,
    F: FnMut(&mut EditableNode),
{
    let mut handlers = [DispatchHandler {
        xpath,
        callback: DispatchCallback::Node(&mut callback),
        limit: None,
    }];
    dispatch(
        Reader::from_reader(reader),
        InputSource::Reader,
        &mut handlers,
        namespaces,
    )
}

/// Processes XML from a reader with streaming transformation.
///
/// Unlike `process_streaming`, this function reads from a `BufRead` source and uses
/// quick_xml's Writer to echo non-matched events, avoiding the need to keep the entire
/// XML input in memory. Matched elements are still built into a DOM subtree for editing.
///
/// Note: The output may have minor formatting differences from the original XML
/// (e.g., attribute quoting style) for non-matched content, since events are
/// reconstructed through the XML writer rather than copied verbatim.
pub fn process_streaming_from_reader<R, W, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut handlers = [RewriteHandler {
        xpath,
        callback: RewriteCallback::Node(&mut transform_fn),
    }];
    rewrite_reader(reader, &mut handlers, namespaces, writer)
}

/// Processes XML from a reader with streaming iteration for multiple XPath handlers.
///
/// Reader-based version of `process_for_each_multi`. Reads from a `BufRead` source
/// instead of requiring the entire XML string in memory.
pub fn process_for_each_multi_from_reader<'a, R>(
    reader: R,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize>
where
    R: BufRead,
{
    let mut handlers: Vec<DispatchHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| DispatchHandler {
            xpath,
            callback: DispatchCallback::Node(&mut **callback),
            limit: None,
        })
        .collect();
    dispatch(
        Reader::from_reader(reader),
        InputSource::Reader,
        &mut handlers,
        namespaces,
    )
}

/// Processes XML from a reader with streaming transformation for multiple XPath handlers.
///
/// Reader-based version of `process_streaming_multi`. Uses quick_xml's Writer to echo
/// non-matched events instead of zero-copy byte slicing, avoiding the need to keep
/// the entire XML input in memory.
pub fn process_streaming_multi_from_reader<'a, R, W>(
    reader: R,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
{
    let mut handlers: Vec<RewriteHandler<'_>> = handlers
        .iter_mut()
        .map(|(xpath, callback)| RewriteHandler {
            xpath,
            callback: RewriteCallback::Node(&mut **callback),
        })
        .collect();
    rewrite_reader(reader, &mut handlers, namespaces, writer)
}
