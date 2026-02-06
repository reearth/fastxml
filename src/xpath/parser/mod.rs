//! XPath expression parser.
//!
//! Parses tokenized XPath expressions into an abstract syntax tree (AST).

mod ast;
mod impl_;

#[cfg(test)]
mod tests;

// Re-export AST types
pub use ast::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step};

// Re-export parser
pub use impl_::{parse_xpath, Parser};
