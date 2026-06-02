//! Streaming XML transformation with zero-copy output.
//!
//! This module provides APIs for transforming XML documents by selectively
//! modifying elements that match XPath expressions, while preserving
//! unchanged portions of the document with zero-copy efficiency.
//!
//! # Features
//!
//! - **Zero-copy output**: Unchanged portions of the input are written directly
//!   without copying or re-serialization
//! - **Selective DOM**: Only matched elements are converted to a modifiable DOM
//! - **Streaming**: Single-pass processing for compatible XPath expressions
//! - **Fallback**: Automatic two-pass processing for complex XPath patterns
//! - **Multiple handlers**: Register multiple XPath-callback pairs
//!
//! # Streamable XPath Patterns
//!
//! The following patterns can be processed in a single streaming pass:
//!
//! - Absolute paths: `/root/items/item`
//! - Descendant search: `//item`
//! - Attribute predicates: `//item[@id='2']`
//! - Namespaced elements: `//ns:item`
//! - Position predicates with upper bound: `//item[position() <= 3]`
//!
//! The following patterns require two-pass processing:
//!
//! - `last()` function: `//item[last()]`, `//item[position()=last()]`
//! - Backward axes: `//item/parent::*`, `//item/ancestor::root`
//! - Complex predicates requiring full tree evaluation
//!
//! # Examples
//!
//! ## Transform with Multiple Handlers
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item id="1">A</item><other>B</other></root>"#;
//!
//! let result = StreamTransformer::new(xml)
//!     .on("//item", |node| {
//!         node.set_attribute("type", "item");
//!     })
//!     .on("//other", |node| {
//!         node.set_attribute("type", "other");
//!     })
//!     .run()?
//!     .to_string()?;
//!
//! assert!(result.contains(r#"type="item""#));
//! assert!(result.contains(r#"type="other""#));
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```
//!
//! ## Collect Data
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
//!
//! let ids: Vec<String> = StreamTransformer::new(xml)
//!     .collect("//item", |node| node.get_attribute("id").unwrap_or_default())?;
//!
//! assert_eq!(ids, vec!["1", "2"]);
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```
//!
//! ## For Each (Side Effects Only)
//!
//! ```rust
//! use fastxml::transform::StreamTransformer;
//!
//! let xml = r#"<root><item>A</item><other>B</other></root>"#;
//!
//! let mut items = Vec::new();
//! let mut others = Vec::new();
//!
//! StreamTransformer::new(xml)
//!     .on("//item", |node| {
//!         items.push(node.get_content().unwrap_or_default());
//!     })
//!     .on("//other", |node| {
//!         others.push(node.get_content().unwrap_or_default());
//!     })
//!     .for_each()?;
//!
//! assert_eq!(items, vec!["A"]);
//! assert_eq!(others, vec!["B"]);
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```

// Core submodules
mod analysis;
mod builder;
mod callbacks;
pub mod context;
mod deprecated;
pub mod editable;
pub mod error;
pub mod fallback;
mod functions;
mod multi;
mod reader;
pub mod span;
pub mod streaming;
mod unified;
pub mod xpath_analyze;

// Re-export main types
pub use builder::{StreamTransformer, TransformOutput};
pub use context::{AncestorInfo, TransformContext};
pub use editable::{EditableNode, EditableNodeBuilder, EditableNodeRef, Modification, NewNode};
pub use error::{ErrorLocation, TransformError, TransformResult};
pub use reader::StreamTransformerReader;
pub use span::ByteSpan;
pub use unified::Transformer;
pub use xpath_analyze::{
    AttributePredicate, NotStreamableReason, PositionPredicate, StreamableStep, StreamableXPath,
    XPathAnalysis,
};

// Re-export XPath types for convenience
pub use crate::xpath::{Expr, XPathResult, XPathSource};

// Re-export analysis functions
pub use analysis::{analyze_xpath_str, get_not_streamable_reason, is_streamable};

// Re-export function API
pub use functions::{
    stream_transform, stream_transform_with_fallback, stream_transform_with_namespaces,
};

// Re-export multi collection trait
pub use multi::CollectMulti;

// Re-export deprecated API
#[allow(deprecated)]
pub use deprecated::StreamTransformBuilder;

// Internal re-exports for submodules
pub(crate) use callbacks::{stream_for_each_with_callback, stream_transform_with_callback};
pub(crate) use functions::stream_for_each_impl;

/// Controls how non-streamable XPath expressions are handled.
///
/// By default, non-streamable XPath expressions will return an error.
/// This prevents unexpected memory usage from automatic fallback to
/// two-pass processing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FallbackMode {
    /// Return an error for non-streamable XPath expressions (default).
    ///
    /// Use this mode when you want to ensure streaming processing
    /// and avoid unexpected memory usage.
    #[default]
    Disabled,
    /// Automatically use two-pass processing for non-streamable XPath.
    ///
    /// **Warning**: This may load the entire document into memory.
    /// Only use this if you understand the memory implications.
    Enabled,
}

#[cfg(test)]
mod tests;
