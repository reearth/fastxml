//! Error types for fastxml.

use std::io;

use crate::namespace_error::NamespaceError;
use crate::node_error::NodeError;
use crate::parse_error::ParseError;
use crate::schema::error::SchemaError;
use crate::schema::fetch_error::FetchError;
use crate::schema::xsd::error::XsdParseError;
use crate::xpath::error::{XPathEvalError, XPathSyntaxError};

/// Main error type for fastxml operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// XML parsing error
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    /// IO error
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// XPath syntax error
    #[error("xpath syntax error: {0}")]
    XPathSyntax(#[from] XPathSyntaxError),

    /// XPath evaluation error
    #[error("xpath evaluation error: {0}")]
    XPathEval(#[from] XPathEvalError),

    /// Schema error
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),

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
    Namespace(#[from] NamespaceError),

    /// Node-related error
    #[error("node error: {0}")]
    Node(#[from] NodeError),

    /// Invalid operation
    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// Network/fetch error
    #[error("fetch error: {0}")]
    Fetch(#[from] FetchError),

    /// UTF-8 encoding error
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// String UTF-8 error
    #[error("string utf8 error: {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    /// XSD parsing error
    #[error("xsd parse error: {0}")]
    XsdParse(#[from] XsdParseError),
}

impl From<quick_xml::Error> for Error {
    fn from(err: quick_xml::Error) -> Self {
        ParseError::Generic {
            message: err.to_string(),
        }
        .into()
    }
}

impl From<quick_xml::events::attributes::AttrError> for Error {
    fn from(err: quick_xml::events::attributes::AttrError) -> Self {
        ParseError::AttributeError {
            message: err.to_string(),
        }
        .into()
    }
}

/// Result type alias for fastxml operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ErrorLevel {
    /// Warning - validation can continue
    Warning,
    /// Error - validation issue but can continue
    #[default]
    Error,
    /// Fatal - validation cannot continue
    Fatal,
}

impl std::fmt::Display for ErrorLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorLevel::Warning => write!(f, "warning"),
            ErrorLevel::Error => write!(f, "error"),
            ErrorLevel::Fatal => write!(f, "fatal"),
        }
    }
}

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
    /// Error severity level
    pub level: ErrorLevel,
    /// XPath-like path to the element where error occurred
    pub element_path: Option<String>,
    /// Name of the element or attribute that caused the error
    pub node_name: Option<String>,
    /// Expected value or type (for type mismatch errors)
    pub expected: Option<String>,
    /// Actual value found (for type mismatch errors)
    pub found: Option<String>,
}

impl Default for StructuredError {
    fn default() -> Self {
        Self {
            message: String::new(),
            line: None,
            column: None,
            error_type: ValidationErrorType::Other,
            level: ErrorLevel::Error,
            element_path: None,
            node_name: None,
            expected: None,
            found: None,
        }
    }
}

impl StructuredError {
    /// Creates a new error with the given message and type.
    pub fn new(message: impl Into<String>, error_type: ValidationErrorType) -> Self {
        Self {
            message: message.into(),
            error_type,
            ..Default::default()
        }
    }

    /// Sets the line number.
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Sets the column number.
    pub fn with_column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    /// Sets the error level.
    pub fn with_level(mut self, level: ErrorLevel) -> Self {
        self.level = level;
        self
    }

    /// Sets the element path.
    pub fn with_element_path(mut self, path: impl Into<String>) -> Self {
        self.element_path = Some(path.into());
        self
    }

    /// Sets the node name.
    pub fn with_node_name(mut self, name: impl Into<String>) -> Self {
        self.node_name = Some(name.into());
        self
    }

    /// Sets the expected value.
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// Sets the found value.
    pub fn with_found(mut self, found: impl Into<String>) -> Self {
        self.found = Some(found.into());
        self
    }

    /// Returns true if this is a warning.
    pub fn is_warning(&self) -> bool {
        self.level == ErrorLevel::Warning
    }

    /// Returns true if this is an error or fatal.
    pub fn is_error(&self) -> bool {
        self.level >= ErrorLevel::Error
    }
}

impl std::fmt::Display for StructuredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format: [level] location: message
        write!(f, "[{}] ", self.level)?;

        if let Some(ref path) = self.element_path {
            write!(f, "{}", path)?;
            if let Some(line) = self.line {
                write!(f, " (line {})", line)?;
            }
            write!(f, ": ")?;
        } else if let (Some(line), Some(col)) = (self.line, self.column) {
            write!(f, "{}:{}: ", line, col)?;
        } else if let Some(line) = self.line {
            write!(f, "line {}: ", line)?;
        }

        write!(f, "{}", self.message)?;

        if let (Some(expected), Some(found)) = (&self.expected, &self.found) {
            write!(f, " (expected: {}, found: {})", expected, found)?;
        }

        Ok(())
    }
}

/// Classification of validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorType {
    /// Unknown or unrecognized element
    UnknownElement,
    /// Unknown or unrecognized attribute
    UnknownAttribute,
    /// Missing required element
    MissingRequiredElement,
    /// Missing required attribute
    MissingRequiredAttribute,
    /// Invalid attribute value
    InvalidAttributeValue,
    /// Invalid element content
    InvalidContent,
    /// Invalid text content (type mismatch)
    InvalidTextContent,
    /// Element appears too many times
    TooManyOccurrences,
    /// Element appears too few times
    TooFewOccurrences,
    /// Element out of order (sequence violation)
    ElementOutOfOrder,
    /// Unexpected element in choice/sequence
    UnexpectedElement,
    /// Namespace mismatch
    NamespaceMismatch,
    /// Schema not found
    SchemaNotFound,
    /// Identity constraint violation (unique, key, keyref)
    IdentityConstraint,
    /// Type definition not found
    TypeNotFound,
    /// Facet constraint violation
    FacetViolation,
    /// Content model violation
    ContentModelViolation,
    /// Unclosed element at end of document
    UnclosedElement,
    /// Generic validation error
    Other,
}
