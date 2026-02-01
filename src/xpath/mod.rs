//! XPath expression support.
//!
//! This module provides XPath 1.0 expression parsing and evaluation
//! with support for a comprehensive subset of the specification.
//!
//! # Supported Features
//!
//! ## Axes
//! - `child::` (default axis)
//! - `descendant::`
//! - `descendant-or-self::` (also via `//`)
//! - `parent::`
//! - `self::`
//! - `ancestor::`
//! - `ancestor-or-self::`
//! - `following-sibling::`
//! - `preceding-sibling::`
//! - `following::`
//! - `preceding::`
//! - `attribute::` (also via `@`)
//! - `namespace::`
//!
//! ## Node Tests
//! - `*` (any element)
//! - `name` (element by name)
//! - `prefix:name` (element by qualified name)
//! - `text()` (text nodes)
//! - `node()` (any node)
//!
//! ## Predicates
//! - `[expr]` (filter expression)
//! - `[position]` (positional predicate)
//! - Comparison operators: `=`, `!=`, `<`, `<=`, `>`, `>=`
//! - Logical operators: `and`, `or`, `not()`
//!
//! ## Operators
//! - Arithmetic: `+`, `-`, `*`, `div`, `mod`
//! - Comparison: `=`, `!=`, `<`, `<=`, `>`, `>=`
//! - Union: `|`
//!
//! ## Functions
//!
//! ### Node Set Functions
//! - `name([node-set])` - qualified name
//! - `local-name([node-set])` - local name without prefix
//! - `namespace-uri([node-set])` - namespace URI
//! - `position()` - context position
//! - `last()` - context size
//! - `count(node-set)` - number of nodes
//! - `id(object)` - select by ID
//!
//! ### String Functions
//! - `string([object])` - convert to string
//! - `concat(string, string, ...)` - concatenate strings
//! - `contains(haystack, needle)` - string contains
//! - `starts-with(string, prefix)` - string starts with
//! - `substring(string, start, [length])` - extract substring
//! - `substring-before(string, string)` - substring before match
//! - `substring-after(string, string)` - substring after match
//! - `string-length([string])` - string length
//! - `normalize-space([string])` - normalize whitespace
//! - `translate(string, from, to)` - character translation
//!
//! ### Boolean Functions
//! - `boolean(object)` - convert to boolean
//! - `not(boolean)` - logical negation
//! - `true()` - returns true
//! - `false()` - returns false
//! - `lang(string)` - check language
//!
//! ### Number Functions
//! - `number([object])` - convert to number
//! - `sum(node-set)` - sum of numeric values
//! - `floor(number)` - round down
//! - `ceiling(number)` - round up
//! - `round(number)` - round to nearest
//!
//! # Module Structure
//!
//! - `types` - Core XPath value types and evaluation context
//! - `lexer` - XPath expression tokenizer
//! - `parser` - XPath expression parser (AST construction)
//! - `evaluator` - XPath expression evaluation engine
//! - `functions` - XPath function library
//! - `axes` - Axis navigation implementations
//! - `operators` - Comparison, arithmetic, and union operators
//! - `context` - XML document context for evaluation
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
//!
//! // Position functions
//! let result = xpath::evaluate(&doc, "/root/*[position()=1]").unwrap();
//!
//! // String functions
//! let result = xpath::evaluate(&doc, "count(/root/*)").unwrap();
//! ```

// Core modules
pub mod context;
pub mod error;
pub mod evaluator;
pub mod lexer;
pub mod parser;
pub mod types;

// New modular implementation
pub mod axes;
pub mod functions;
pub mod operators;

// Re-export main types and functions
pub use context::{
    XmlContext, XmlSafeContext, create_context, create_safe_context, find_nodes_by_xpath,
    find_readonly_nodes_by_xpath, find_readonly_nodes_in_elements,
    find_safe_readonly_nodes_by_xpath,
};
pub use evaluator::{
    XPathEvaluator, XPathResult, collect_text_value, collect_text_values, evaluate,
};
pub use operators::ArithmeticOp;
pub use parser::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step, parse_xpath};
pub use types::{EvaluationContext, XPathValue};

use crate::error::Result;

/// Source of XPath expression - either a string or a pre-parsed AST.
///
/// This allows APIs to accept either a string (which will be parsed on use)
/// or a pre-parsed AST for reuse or programmatic construction.
#[derive(Debug, Clone)]
pub enum XPathSource {
    /// XPath as a string (will be parsed on use)
    String(String),
    /// Pre-parsed XPath AST
    Ast(Expr),
}

impl XPathSource {
    /// Parses the XPath source into an AST.
    pub fn parse(&self) -> Result<Expr> {
        match self {
            XPathSource::String(s) => parse_xpath(s),
            XPathSource::Ast(expr) => Ok(expr.clone()),
        }
    }

    /// Returns the original string if this is a string source.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            XPathSource::String(s) => Some(s),
            XPathSource::Ast(_) => None,
        }
    }
}

impl From<&str> for XPathSource {
    fn from(s: &str) -> Self {
        XPathSource::String(s.to_string())
    }
}

impl From<String> for XPathSource {
    fn from(s: String) -> Self {
        XPathSource::String(s)
    }
}

impl From<Expr> for XPathSource {
    fn from(expr: Expr) -> Self {
        XPathSource::Ast(expr)
    }
}
