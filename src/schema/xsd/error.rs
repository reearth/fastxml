//! XSD parsing error types.

/// XSD parsing errors that occur during schema parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum XsdParseError {
    /// Unexpected end of schema - parsing stack not empty
    UnexpectedEndOfSchema {
        /// Number of frames remaining in the stack
        remaining_frames: usize,
    },
    /// Parse error at a specific position
    ParseError {
        /// Position in the input where the error occurred
        position: usize,
        /// Error message
        message: String,
    },
}

impl std::fmt::Display for XsdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XsdParseError::UnexpectedEndOfSchema { remaining_frames } => {
                write!(
                    f,
                    "unexpected end of schema, stack not empty: {} frames remaining",
                    remaining_frames
                )
            }
            XsdParseError::ParseError { position, message } => {
                write!(f, "parse error at position {}: {}", position, message)
            }
        }
    }
}

impl std::error::Error for XsdParseError {}
