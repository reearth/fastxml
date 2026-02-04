//! XML parsing error types.

/// XML parsing errors that occur during document parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// Parse error at a specific position
    AtPosition {
        /// Position in the input where the error occurred
        position: u64,
        /// Error message
        message: String,
    },
    /// Memory limit exceeded
    MemoryLimitExceeded {
        /// Memory used (bytes)
        used: usize,
        /// Maximum allowed (bytes)
        max: usize,
    },
    /// Text decode error
    TextDecodeError {
        /// Error message
        message: String,
    },
    /// Attribute value decode error
    AttributeDecodeError {
        /// Error message
        message: String,
    },
    /// Attribute parsing error
    AttributeError {
        /// Error message
        message: String,
    },
    /// Generic parse error (from quick_xml or other sources)
    Generic {
        /// Error message
        message: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::AtPosition { position, message } => {
                write!(f, "parse error at position {}: {}", position, message)
            }
            ParseError::MemoryLimitExceeded { used, max } => {
                write!(f, "memory limit exceeded: {} > {}", used, max)
            }
            ParseError::TextDecodeError { message } => {
                write!(f, "text decode error: {}", message)
            }
            ParseError::AttributeDecodeError { message } => {
                write!(f, "attribute value decode error: {}", message)
            }
            ParseError::AttributeError { message } => {
                write!(f, "attribute error: {}", message)
            }
            ParseError::Generic { message } => {
                write!(f, "{}", message)
            }
        }
    }
}

impl std::error::Error for ParseError {}
