//! Single-pass streaming processor for XML transformation.

mod helpers;
mod multi;
mod reader;
mod single;

use super::context::TransformContext;
use super::editable::EditableNode;
use super::xpath_analyze::StreamableXPath;

pub use helpers::{ElementInfo, PathTracker};
pub use multi::{
    process_for_each_multi, process_for_each_multi_with_context, process_streaming_multi,
    process_streaming_multi_with_context,
};
pub use reader::{
    process_for_each_from_reader, process_for_each_multi_from_reader,
    process_streaming_from_reader, process_streaming_multi_from_reader,
};
pub use single::{
    process_for_each, process_for_each_with_context, process_streaming,
    process_streaming_with_context,
};

/// Handler pair for multi-xpath processing: (xpath, callback).
pub type MultiHandler<'a> = (&'a StreamableXPath, &'a mut dyn FnMut(&mut EditableNode));

/// Handler pair with context for multi-xpath processing: (xpath, callback).
pub type MultiHandlerWithContext<'a> = (
    &'a StreamableXPath,
    &'a mut dyn FnMut(&mut EditableNode, &TransformContext),
);

/// Handler pair for multi-xpath transform processing: (xpath, callback).
pub type MultiTransformHandler<'a> = (&'a StreamableXPath, &'a mut dyn FnMut(&mut EditableNode));

/// Handler pair with context for multi-xpath transform processing: (xpath, callback).
pub type MultiTransformHandlerWithContext<'a> = (
    &'a StreamableXPath,
    &'a mut dyn FnMut(&mut EditableNode, &TransformContext),
);

/// State for tracking a single XPath handler during multi-xpath processing.
pub(crate) struct HandlerState<'a> {
    pub(crate) xpath: &'a StreamableXPath,
    pub(crate) builder: Option<super::editable::EditableNodeBuilder>,
    pub(crate) match_context: Option<TransformContext>,
}

/// State for tracking a single XPath handler during multi-xpath transform processing.
/// Includes match_start_offset for zero-copy output.
pub(crate) struct TransformHandlerState<'a> {
    pub(crate) xpath: &'a StreamableXPath,
    pub(crate) builder: Option<super::editable::EditableNodeBuilder>,
    pub(crate) match_context: Option<TransformContext>,
    /// Byte offset where the match started (for zero-copy output).
    pub(crate) match_start_offset: usize,
}

#[cfg(test)]
mod tests;
