//! Streaming XML schema validator.
//!
//! This module provides streaming validation of XML documents against XSD schemas.
//!
//! # Module Structure
//!
//! - `state` - Validation state management during streaming
//! - `streaming` - Core streaming schema validator
//! - `context` - Schema validation context wrapper
//! - `lazy` - Lazy validators that initialize from xsi:schemaLocation
//! - `api` - Public API functions

mod api;
mod context;
mod lazy;
mod state;
mod streaming;

// Re-export ValidationMode
pub use self::mode::ValidationMode;

// Re-export main types
pub use context::XmlSchemaValidationContext;
pub use lazy::LazySchemaValidator;
pub use streaming::{StreamingSchemaValidator, ValidationOptions};

// Re-export API functions
pub use api::{
    create_xml_schema_validation_context, create_xml_schema_validation_context_from_buffer,
    get_schema_from_schema_location_with_fetcher,
    streaming_validate_with_schema_location_and_fetcher, validate_document_by_schema,
    validate_document_by_schema_context, validate_with_schema_location_and_fetcher,
};

#[cfg(feature = "ureq")]
pub use api::{
    get_schema_from_schema_location, streaming_validate_with_schema_location,
    validate_with_schema_location,
};

/// Validation mode module.
mod mode {
    /// Validation mode controlling strictness.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum ValidationMode {
        /// Lenient mode - only report definite errors
        Lenient,
        /// Strict mode (default) - report all schema violations
        #[default]
        Strict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_mode_default() {
        let mode = ValidationMode::default();
        assert_eq!(mode, ValidationMode::Strict);
    }

    #[test]
    fn test_validation_mode_equality() {
        assert_eq!(ValidationMode::Strict, ValidationMode::Strict);
        assert_eq!(ValidationMode::Lenient, ValidationMode::Lenient);
        assert_ne!(ValidationMode::Strict, ValidationMode::Lenient);
    }

    #[test]
    fn test_validation_mode_eq() {
        let mode1 = ValidationMode::Strict;
        let mode2 = ValidationMode::Strict;
        assert!(mode1 == mode2);
    }

    #[test]
    fn test_validation_mode_clone() {
        let mode = ValidationMode::Lenient;
        let cloned = mode;
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_validation_mode_debug() {
        let mode = ValidationMode::Strict;
        let debug_str = format!("{:?}", mode);
        assert!(debug_str.contains("Strict"));

        let mode_lenient = ValidationMode::Lenient;
        let debug_str_lenient = format!("{:?}", mode_lenient);
        assert!(debug_str_lenient.contains("Lenient"));
    }
}
