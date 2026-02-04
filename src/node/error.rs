//! Node-related error types.

use std::fmt;

/// Errors related to XML node operations.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeError {
    /// No root element found in the document
    NoRootElement,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeError::NoRootElement => write!(f, "no root element"),
        }
    }
}

impl std::error::Error for NodeError {}
