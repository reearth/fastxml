//! Schema error types.

/// Schema errors that occur during schema processing.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaError {
    /// Invalid occurrence value
    InvalidOccurs {
        /// The invalid value
        value: String,
    },
    /// minOccurs is greater than maxOccurs
    MinOccursGreaterThanMaxOccurs {
        /// The minOccurs value
        min: u32,
        /// The maxOccurs value
        max: u32,
    },
    /// Invalid facet value (not a non-negative integer)
    InvalidFacetValue {
        /// The facet name (e.g., "minLength", "maxLength")
        facet: String,
        /// The invalid value
        value: String,
        /// Reason for invalidity
        reason: String,
    },
    /// minLength is greater than maxLength
    MinLengthGreaterThanMaxLength {
        /// The minLength value
        min_length: u64,
        /// The maxLength value
        max_length: u64,
    },
    /// fractionDigits is greater than totalDigits
    FractionDigitsGreaterThanTotalDigits {
        /// The fractionDigits value
        fraction_digits: u64,
        /// The totalDigits value
        total_digits: u64,
    },
    /// Schema not found
    SchemaNotFound {
        /// The schema URI
        uri: String,
    },
    /// Invalid base URI
    InvalidBaseUri {
        /// The invalid URI
        uri: String,
        /// Error message
        message: String,
    },
    /// Failed to resolve URL
    UrlResolutionFailed {
        /// The relative URL
        relative: String,
        /// The base URL
        base: String,
        /// Error message
        message: String,
    },
    /// Circular dependency detected in schema imports
    CircularDependency {
        /// The URI that caused the circular dependency
        uri: String,
    },
    /// The schema document violates a schema-for-schemas constraint
    InvalidSchema {
        /// What is wrong
        message: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::InvalidOccurs { value } => {
                write!(f, "invalid occurs value: {}", value)
            }
            SchemaError::MinOccursGreaterThanMaxOccurs { min, max } => {
                write!(
                    f,
                    "minOccurs ({}) cannot be greater than maxOccurs ({})",
                    min, max
                )
            }
            SchemaError::InvalidFacetValue {
                facet,
                value,
                reason,
            } => {
                write!(f, "invalid {} value '{}': {}", facet, value, reason)
            }
            SchemaError::MinLengthGreaterThanMaxLength {
                min_length,
                max_length,
            } => {
                write!(
                    f,
                    "minLength ({}) cannot be greater than maxLength ({})",
                    min_length, max_length
                )
            }
            SchemaError::FractionDigitsGreaterThanTotalDigits {
                fraction_digits,
                total_digits,
            } => {
                write!(
                    f,
                    "fractionDigits ({}) cannot be greater than totalDigits ({})",
                    fraction_digits, total_digits
                )
            }
            SchemaError::SchemaNotFound { uri } => {
                write!(f, "schema not found: {}", uri)
            }
            SchemaError::InvalidBaseUri { uri, message } => {
                write!(f, "invalid base URI '{}': {}", uri, message)
            }
            SchemaError::UrlResolutionFailed {
                relative,
                base,
                message,
            } => {
                write!(
                    f,
                    "failed to resolve '{}' against '{}': {}",
                    relative, base, message
                )
            }
            SchemaError::InvalidSchema { message } => {
                write!(f, "invalid schema: {}", message)
            }
            SchemaError::CircularDependency { uri } => {
                write!(f, "circular dependency in schema imports: {}", uri)
            }
        }
    }
}

impl std::error::Error for SchemaError {}
