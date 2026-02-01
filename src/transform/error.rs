//! Error types for the transform module.

use std::io;

/// Errors that can occur during XML transformation.
#[derive(Debug, thiserror::Error)]
pub enum TransformError {
    /// XPath expression is invalid or unsupported
    #[error("invalid xpath: {0}")]
    InvalidXPath(String),

    /// XML parsing error during transformation
    #[error("xml parse error: {0}")]
    XmlParse(String),

    /// IO error during output
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// UTF-8 encoding error
    #[error("utf8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Serialization error
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Node modification error
    #[error("modification error: {0}")]
    Modification(String),

    /// General error from the crate
    #[error(transparent)]
    Other(#[from] crate::error::Error),
}

impl From<quick_xml::Error> for TransformError {
    fn from(err: quick_xml::Error) -> Self {
        TransformError::XmlParse(err.to_string())
    }
}

/// Result type for transform operations.
pub type TransformResult<T> = std::result::Result<T, TransformError>;
