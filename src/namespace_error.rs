//! Namespace-related error types.

use std::fmt;

/// Errors related to XML namespace operations.
#[derive(Debug, Clone, PartialEq)]
pub enum NamespaceError {
    /// Unknown namespace prefix in XPath or element access
    UnknownPrefix {
        /// The unresolved prefix
        prefix: String,
    },
    /// Namespace URI mismatch between element and schema
    UriMismatch {
        /// The expected namespace URI
        expected: String,
        /// The actual namespace URI found
        found: String,
    },
    /// Missing required namespace declaration
    MissingDeclaration {
        /// The namespace URI that should have been declared
        uri: String,
    },
}

impl fmt::Display for NamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamespaceError::UnknownPrefix { prefix } => {
                write!(f, "unknown namespace prefix: '{}'", prefix)
            }
            NamespaceError::UriMismatch { expected, found } => {
                write!(
                    f,
                    "namespace URI mismatch: expected '{}', found '{}'",
                    expected, found
                )
            }
            NamespaceError::MissingDeclaration { uri } => {
                write!(f, "missing namespace declaration for URI: '{}'", uri)
            }
        }
    }
}

impl std::error::Error for NamespaceError {}
