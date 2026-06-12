//! Error types for fastxml.

use std::fmt;
use std::io;

use crate::namespace::error::NamespaceError;
use crate::node::error::NodeError;
use crate::parser::error::ParseError;
use crate::schema::error::SchemaError;
use crate::schema::fetcher::error::FetchError;
use crate::schema::xsd::error::XsdParseError;
use crate::transform::error::TransformError;
use crate::xpath::error::{XPathEvalError, XPathSyntaxError};

/// Location information for errors, providing line, column, byte offset, and optional XPath.
///
/// This struct provides a lightweight way to attach location information to any error.
/// It can be used across various modules (transform, parser, validator) to provide
/// consistent error location reporting.
///
/// # Examples
///
/// ```
/// use fastxml::error::ErrorLocation;
///
/// // Create from byte offset with line/column calculation
/// let input = "line1\nline2\nline3";
/// let loc = ErrorLocation::from_offset_with_input(6, input);
/// assert_eq!(loc.line, Some(2));
/// assert_eq!(loc.column, Some(1));
///
/// // Multi-byte UTF-8 characters are counted as single columns
/// let input = "あいう\nえお";
/// // "あいう" is 9 bytes (3 bytes each), "\n" is 1 byte, "え" starts at byte 10
/// let loc = ErrorLocation::from_offset_with_input(10, input);
/// assert_eq!(loc.line, Some(2));
/// assert_eq!(loc.column, Some(1)); // First char of line 2
///
/// // Add XPath information
/// let loc = loc.with_xpath("/root/item[1]".to_string());
/// assert!(loc.to_string().contains("/root/item[1]"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct ErrorLocation {
    /// Line number (1-indexed)
    pub line: Option<usize>,
    /// Column number (1-indexed)
    pub column: Option<usize>,
    /// Byte offset from the beginning of the input
    pub byte_offset: Option<usize>,
    /// XPath-like path to the error location. Shared: validators intern
    /// paths, which repeat heavily across errors in the same subtree.
    pub xpath: Option<std::sync::Arc<str>>,
}

impl ErrorLocation {
    /// Creates an empty error location.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an error location with only byte offset.
    pub fn from_offset(byte_offset: usize) -> Self {
        Self {
            byte_offset: Some(byte_offset),
            ..Default::default()
        }
    }

    /// Creates an error location with line and column (calculated from byte offset).
    pub fn from_offset_with_input(byte_offset: usize, input: &str) -> Self {
        let (line, column) = Self::calculate_line_column(input, byte_offset);
        Self {
            line: Some(line),
            column: Some(column),
            byte_offset: Some(byte_offset),
            xpath: None,
        }
    }

    /// Creates an error location with line and column directly.
    pub fn from_line_column(line: usize, column: usize) -> Self {
        Self {
            line: Some(line),
            column: Some(column),
            byte_offset: None,
            xpath: None,
        }
    }

    /// Sets the XPath-like path.
    pub fn with_xpath(mut self, xpath: impl Into<String>) -> Self {
        self.xpath = Some(std::sync::Arc::from(xpath.into()));
        self
    }

    /// Sets the byte offset.
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.byte_offset = Some(offset);
        self
    }

    /// Returns true if this location has any position information.
    pub fn has_position(&self) -> bool {
        self.line.is_some() || self.byte_offset.is_some()
    }

    /// Calculates line and column from byte offset in the input string.
    ///
    /// Returns (line, column) where both are 1-indexed.
    /// Column is counted in Unicode characters (not bytes), so multi-byte
    /// characters like Japanese are counted as single columns.
    pub fn calculate_line_column(input: &str, byte_offset: usize) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;

        for (pos, ch) in input.char_indices() {
            if pos >= byte_offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }

        (line, column)
    }
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if let (Some(line), Some(col)) = (self.line, self.column) {
            parts.push(format!("line {}:{}", line, col));
        } else if let Some(offset) = self.byte_offset {
            parts.push(format!("position {}", offset));
        }

        if let Some(xpath) = &self.xpath {
            parts.push(format!("at {}", xpath));
        }

        write!(f, "{}", parts.join(", "))
    }
}

/// Main error type for fastxml operations.
///
/// `#[non_exhaustive]`: match with a wildcard arm, as new variants may be added.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

    /// Stream-transformation error.
    ///
    /// Boxed to break the cycle with [`TransformError::Other`], which wraps an
    /// `Error`.
    #[error(transparent)]
    Transform(Box<TransformError>),
}

impl From<TransformError> for Error {
    fn from(err: TransformError) -> Self {
        match err {
            // Unwrap errors that already have a first-class `Error` variant so a
            // round-trip (`Error` -> `TransformError::Other` -> `Error`) is lossless.
            TransformError::Other(inner) => inner,
            TransformError::Io(io) => Error::Io(io),
            TransformError::Utf8(utf8) => Error::Utf8(utf8),
            other => Error::Transform(Box::new(other)),
        }
    }
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
    /// Error message. Stored as a shared string: validators intern
    /// repeated messages so a million identical errors hold one
    /// allocation.
    pub message: std::sync::Arc<str>,
    /// Location information (line, column, byte_offset, xpath)
    pub location: ErrorLocation,
    /// Error type classification
    pub error_type: ValidationErrorType,
    /// Error severity level
    pub level: ErrorLevel,
    /// Name of the element or attribute that caused the error
    pub node_name: Option<std::sync::Arc<str>>,
    /// Expected value or type (for type mismatch errors)
    pub expected: Option<std::sync::Arc<str>>,
    /// Actual value found (for type mismatch errors)
    pub found: Option<std::sync::Arc<str>>,
}

impl Default for StructuredError {
    fn default() -> Self {
        Self {
            message: std::sync::Arc::from(""),
            location: ErrorLocation::default(),
            error_type: ValidationErrorType::Other,
            level: ErrorLevel::Error,
            node_name: None,
            expected: None,
            found: None,
        }
    }
}

/// Returns the pooled copy of `s`, inserting it on first sight.
pub(crate) fn intern_arc(
    pool: &mut std::collections::HashSet<std::sync::Arc<str>>,
    s: &std::sync::Arc<str>,
) -> std::sync::Arc<str> {
    if let Some(existing) = pool.get(s.as_ref()) {
        std::sync::Arc::clone(existing)
    } else {
        pool.insert(std::sync::Arc::clone(s));
        std::sync::Arc::clone(s)
    }
}

impl StructuredError {
    /// Creates a new error with the given message and type.
    pub fn new(message: impl Into<String>, error_type: ValidationErrorType) -> Self {
        Self {
            message: std::sync::Arc::from(message.into()),
            error_type,
            ..Default::default()
        }
    }

    /// Interns the shared string fields (message, node name, element path,
    /// expected/found values) through `pool`, so identical strings across
    /// many errors share one allocation.
    pub(crate) fn interned(
        mut self,
        pool: &mut std::collections::HashSet<std::sync::Arc<str>>,
    ) -> Self {
        self.message = intern_arc(pool, &self.message);
        for field in [
            &mut self.node_name,
            &mut self.location.xpath,
            &mut self.expected,
            &mut self.found,
        ] {
            if let Some(s) = field.take() {
                *field = Some(intern_arc(pool, &s));
            }
        }
        self
    }

    /// Sets the line number.
    pub fn with_line(mut self, line: usize) -> Self {
        self.location.line = Some(line);
        self
    }

    /// Sets the column number.
    pub fn with_column(mut self, column: usize) -> Self {
        self.location.column = Some(column);
        self
    }

    /// Sets the byte offset.
    pub fn with_byte_offset(mut self, offset: usize) -> Self {
        self.location.byte_offset = Some(offset);
        self
    }

    /// Sets the error level.
    pub fn with_level(mut self, level: ErrorLevel) -> Self {
        self.level = level;
        self
    }

    /// Sets the element path (stored in location.xpath).
    pub fn with_element_path(mut self, path: impl Into<String>) -> Self {
        self.location.xpath = Some(std::sync::Arc::from(path.into()));
        self
    }

    /// Sets the node name.
    pub fn with_node_name(mut self, name: impl Into<String>) -> Self {
        self.node_name = Some(std::sync::Arc::from(name.into()));
        self
    }

    /// Sets the expected value.
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(std::sync::Arc::from(expected.into()));
        self
    }

    /// Sets the found value.
    pub fn with_found(mut self, found: impl Into<String>) -> Self {
        self.found = Some(std::sync::Arc::from(found.into()));
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

    /// Sets location information from an ErrorLocation (merges non-None fields).
    pub fn with_location(mut self, location: &ErrorLocation) -> Self {
        if let Some(line) = location.line {
            self.location.line = Some(line);
        }
        if let Some(column) = location.column {
            self.location.column = Some(column);
        }
        if let Some(offset) = location.byte_offset {
            self.location.byte_offset = Some(offset);
        }
        if let Some(ref xpath) = location.xpath {
            self.location.xpath = Some(std::sync::Arc::clone(xpath));
        }
        self
    }

    /// Sets the entire location, replacing any existing location.
    pub fn set_location(mut self, location: ErrorLocation) -> Self {
        self.location = location;
        self
    }

    /// Returns the line number (convenience accessor).
    pub fn line(&self) -> Option<usize> {
        self.location.line
    }

    /// Returns the column number (convenience accessor).
    pub fn column(&self) -> Option<usize> {
        self.location.column
    }

    /// Returns the byte offset (convenience accessor).
    pub fn byte_offset(&self) -> Option<usize> {
        self.location.byte_offset
    }

    /// Returns the element path (convenience accessor).
    pub fn element_path(&self) -> Option<&str> {
        self.location.xpath.as_deref()
    }

    /// Calculates and sets line/column from byte_offset using the given input.
    ///
    /// This is useful when you have a byte offset but need to display line/column.
    pub fn calculate_line_column(mut self, input: &str) -> Self {
        if let Some(offset) = self.location.byte_offset {
            let (line, column) = ErrorLocation::calculate_line_column(input, offset);
            self.location.line = Some(line);
            self.location.column = Some(column);
        }
        self
    }
}

impl From<&StructuredError> for ErrorLocation {
    fn from(err: &StructuredError) -> Self {
        err.location.clone()
    }
}

impl std::fmt::Display for StructuredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format: [level] location: message
        write!(f, "[{}] ", self.level)?;

        if let Some(ref path) = self.location.xpath {
            write!(f, "{}", path)?;
            if let Some(line) = self.location.line {
                write!(f, " (line {})", line)?;
            }
            write!(f, ": ")?;
        } else if let (Some(line), Some(col)) = (self.location.line, self.location.column) {
            write!(f, "{}:{}: ", line, col)?;
        } else if let Some(line) = self.location.line {
            write!(f, "line {}: ", line)?;
        } else if let Some(offset) = self.location.byte_offset {
            write!(f, "offset {}: ", offset)?;
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

#[cfg(test)]
mod transform_conversion_tests {
    use super::*;

    #[test]
    fn transform_error_converts_into_crate_error() {
        // A function returning the crate-wide Result can `?` a transform error.
        fn run() -> Result<()> {
            Err(TransformError::InvalidXPath("bad".into()))?;
            Ok(())
        }
        let err = run().unwrap_err();
        assert!(matches!(err, Error::Transform(_)));
        assert!(err.to_string().contains("bad"));
    }

    #[test]
    fn other_variant_unwraps_losslessly() {
        // Error -> TransformError::Other -> Error round-trips without nesting.
        let wrapped: TransformError = Error::InvalidOperation("x".into()).into();
        let back: Error = wrapped.into();
        assert!(matches!(back, Error::InvalidOperation(_)));
    }

    #[test]
    fn io_and_utf8_map_to_their_variants() {
        let io = TransformError::Io(io::Error::other("boom"));
        assert!(matches!(Error::from(io), Error::Io(_)));
    }
}
