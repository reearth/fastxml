//! Streaming XML transformation with zero-copy output.
//!
//! This module provides APIs for transforming XML documents by selectively
//! modifying elements that match XPath expressions, while preserving
//! unchanged portions of the document with zero-copy efficiency.
//!
//! # Features
//!
//! - **Zero-copy output**: With in-memory input, the input outside matched
//!   elements is copied to the output verbatim (matched elements are
//!   re-serialized from their DOM)
//! - **Selective DOM**: Only matched elements are converted to a modifiable DOM
//! - **Streaming**: Single-pass processing for compatible XPath expressions
//! - **Fallback**: Opt-in two-pass processing for other XPath expressions
//!   (`allow_fallback()`; disabled by default)
//! - **Multiple handlers**: Register multiple XPath-callback pairs
//!
//! # Streamable XPath Patterns
//!
//! An XPath is streamable when the single-pass matcher can evaluate it
//! exactly, so it selects the same elements as the DOM XPath evaluator:
//!
//! - Child and descendant steps: `/root/items/item`, `//item`, `//a/b`,
//!   `//a//b`, `/root//item`, `/root/descendant::item`
//! - Element name tests: `item`, `*`, `ns:item`, `ns:*` (prefixes are compared
//!   by namespace URI, resolved from the document's `xmlns` declarations)
//! - Attribute predicates: `[@id='2']`, `[@id!='2']`, `[@id]`, `[@gml:id='b2']`
//! - A position as the first predicate of a child step: `//item[2]`,
//!   `/root/*[1]`, `//item[position() <= 3]`, `//item[1][@k='x']`
//! - `[namespace-uri()='…']`, and `[local-name()='…']` on `*`
//!
//! Anything else is not streamable, for example:
//!
//! - `last()`: `//item[last()]`, `//item[position()=last()]`
//! - Backward axes: `//item/parent::*`, `//item/ancestor::root`
//! - Sibling and self axes: `//a/following-sibling::b`, `self::*`
//! - Non-element node tests: `//text()`, `//node()`
//! - `and` / `or` / `not()`, numeric or relational comparisons (`[@id=3]`,
//!   `[@id>3]`), `position() != n`, a position after another predicate
//!   (`//item[@k='x'][1]`), and unions
//!
//! Non-streamable expressions return [`TransformError::NotStreamable`] unless
//! fallback is enabled.
//!
//! # Examples
//!
//! ## Transform with Multiple Handlers
//!
//! ```rust
//! use fastxml::transform::Transformer;
//!
//! let xml = r#"<root><item id="1">A</item><other>B</other></root>"#;
//!
//! let result = Transformer::from(xml)
//!     .on("//item", |node| {
//!         node.set_attribute("type", "item");
//!     })
//!     .on("//other", |node| {
//!         node.set_attribute("type", "other");
//!     })
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
//! use fastxml::transform::Transformer;
//!
//! let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
//!
//! let ids: Vec<String> = Transformer::from(xml)
//!     .collect("//item", |node| node.get_attribute("id").unwrap_or_default())?;
//!
//! assert_eq!(ids, vec!["1", "2"]);
//! # Ok::<(), fastxml::transform::TransformError>(())
//! ```
//!
//! ## For Each (Side Effects Only)
//!
//! ```rust
//! use fastxml::transform::Transformer;
//!
//! let xml = r#"<root><item>A</item><other>B</other></root>"#;
//!
//! let mut items = Vec::new();
//! let mut others = Vec::new();
//!
//! Transformer::from(xml)
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
pub mod editable;
pub mod error;
pub mod fallback;
mod functions;
mod multi;
mod reader;
pub mod span;
mod streamable;
pub mod streaming;
mod unified;
pub mod xpath_analyze;

// The public transform entry point is `Transformer`. The builders behind it
// (`builder` / `reader`) and the `stream_transform*` free functions are
// internal; the lower-level engine modules (`streaming`, `fallback`,
// `xpath_analyze`, `span`) are public.
pub use context::{AncestorInfo, TransformContext};
pub use editable::{EditableNode, EditableNodeBuilder, EditableNodeRef, Modification, NewNode};
pub use error::{ErrorLocation, TransformError, TransformResult};
pub use span::ByteSpan;
pub use streamable::{IntoStreamable, StreamableQuery};
pub use unified::Transformer;
pub use xpath_analyze::{
    AttributePredicate, NotStreamableReason, PositionPredicate, StreamableStep, StreamableXPath,
    XPathAnalysis,
};

// Re-export XPath types for convenience
pub use crate::xpath::{Expr, XPathResult, XPathSource};

// Re-export analysis functions
pub use analysis::{analyze_xpath_str, get_not_streamable_reason, is_streamable};

// Re-export multi collection trait
pub use multi::CollectMulti;

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
