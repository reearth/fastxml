//! Single-pass streaming processor for XML transformation.

mod dispatch;
mod helpers;
mod multi;
mod reader;
mod rewrite;
mod single;
mod tracker;

use super::context::TransformContext;
use super::editable::EditableNode;
use super::xpath_analyze::StreamableXPath;

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
pub use tracker::{ElementInfo, PathTracker};

pub(crate) use dispatch::{DispatchCallback, DispatchHandler, InputSource, dispatch};
pub(crate) use helpers::{add_pi_to_builder, extract_element_info, split_bom};

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

#[cfg(test)]
mod tests;
