//! XPath expression support.
//!
//! This module provides XPath 1.0 expression parsing and evaluation
//! with support for a subset of the specification used by CityGML processing.
//!
//! # Supported Features
//!
//! ## Axes
//! - `child::` (default)
//! - `descendant::`
//! - `descendant-or-self::` (also via `//`)
//! - `parent::`
//! - `self::`
//! - `ancestor::`
//! - `following-sibling::`
//! - `preceding-sibling::`
//! - `@` (attribute axis shorthand)
//!
//! ## Node Tests
//! - `*` (any element)
//! - `name` (element by name)
//! - `prefix:name` (element by qualified name)
//! - `text()` (text nodes)
//!
//! ## Predicates
//! - `[expr]` (filter expression)
//! - `[position]` (positional predicate)
//! - Comparison operators: `=`, `!=`, `<`, `<=`, `>`, `>=`
//! - Logical operators: `and`, `or`, `not()`
//!
//! ## Functions
//! - `name()` - qualified name of context node
//! - `local-name()` - local name without prefix
//! - `namespace-uri()` - namespace URI
//! - `text()` - text content
//! - `contains(haystack, needle)` - string contains
//! - `starts-with(string, prefix)` - string starts with
//!
//! # Examples
//!
//! ```
//! use fastxml::{parse, xpath};
//!
//! let doc = parse(r#"<root><Building/><Room/></root>"#).unwrap();
//!
//! // Simple path
//! let result = xpath::evaluate(&doc, "/root/Building").unwrap();
//!
//! // Descendant search
//! let result = xpath::evaluate(&doc, "//Building").unwrap();
//!
//! // Name predicate
//! let result = xpath::evaluate(&doc, "//*[name()='Building']").unwrap();
//!
//! // Logical OR
//! let result = xpath::evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
//! ```

pub mod context;
pub mod evaluator;
pub mod lexer;
pub mod parser;

// Re-export main types and functions
pub use context::{
    XmlContext, XmlSafeContext,
    create_context, create_safe_context,
    find_nodes_by_xpath, find_readonly_nodes_by_xpath,
    find_safe_readonly_nodes_by_xpath, find_readonly_nodes_in_elements,
};
pub use evaluator::{
    XPathResult, XPathEvaluator,
    evaluate, collect_text_values, collect_text_value,
};
pub use parser::{
    Axis, NodeTest, Predicate, ComparisonOp, Expr, PathExpr, Step,
    parse_xpath,
};
