//! Error types for fastxml.

use std::io;

/// Main error type for fastxml operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// XML parsing error
    #[error("parse error: {0}")]
    Parse(String),

    /// IO error
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// XPath syntax error
    #[error("xpath syntax error: {0}")]
    XPathSyntax(String),

    /// XPath evaluation error
    #[error("xpath evaluation error: {0}")]
    XPathEval(String),

    /// Schema error
    #[error("schema error: {0}")]
    Schema(String),

    /// Validation error
    #[error("validation error: {message}")]
    Validation {
        /// Error message
        message: String,
        /// Line number where the error occurred
        line: Option<usize>,
        /// Column number where the error occurred
        column: Option<usize>,
    },

    /// Namespace error
    #[error("namespace error: {0}")]
    Namespace(String),

    /// Node not found
    #[error("node not found: {0}")]
    NodeNotFound(String),

    /// Invalid operation
    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// Network/fetch error
    #[error("fetch error: {0}")]
    Fetch(String),

    /// UTF-8 encoding error
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// String UTF-8 error
    #[error("string utf8 error: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),
}

impl From<quick_xml::Error> for Error {
    fn from(err: quick_xml::Error) -> Self {
        Error::Parse(err.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(err: quick_xml::events::attributes::AttrError) -> Self {
        Error::Parse(format!("attribute error: {}", err))
    }
}

/// Result type alias for fastxml operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Structured error for schema validation, compatible with libxml's StructuredError.
#[derive(Debug, Clone)]
pub struct StructuredError {
    /// Error message
    pub message: String,
    /// Line number (1-based, if available)
    pub line: Option<usize>,
    /// Column number (1-based, if available)
    pub column: Option<usize>,
    /// Error type classification
    pub error_type: ValidationErrorType,
}

impl std::fmt::Display for StructuredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(line), Some(col)) = (self.line, self.column) {
            write!(f, "{}:{}: {}", line, col, self.message)
        } else if let Some(line) = self.line {
            write!(f, "line {}: {}", line, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

/// Classification of validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorType {
    /// Unknown or unrecognized element
    UnknownElement,
    /// Missing required attribute
    MissingRequiredAttribute,
    /// Invalid attribute value
    InvalidAttributeValue,
    /// Invalid element content
    InvalidContent,
    /// Namespace mismatch
    NamespaceMismatch,
    /// Schema not found
    SchemaNotFound,
    /// Generic validation error
    Other,
}
