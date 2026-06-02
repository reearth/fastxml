//! XML schema validators.
//!
//! This module provides multiple validation approaches for XML documents against XSD schemas:
//!
//! - [`DomSchemaValidator`] - Direct DOM tree validation (~17 MB/s)
//! - [`StreamValidator`] / [`OnePassSchemaValidator`] - Streaming single-pass validation (~41 MB/s)
//!
//! # Module Structure
//!
//! - `dom` - DOM-based validator for pre-parsed documents
//! - `streaming` - One-pass streaming validator (recommended for large files)
//! - `state` - Validation state management during streaming
//! - `context` - Schema validation context wrapper
//! - `lazy` - Lazy validators that initialize from xsi:schemaLocation
//! - `api` - Public API functions

mod api;
mod context;
mod dom;
mod facade;
mod lazy;
mod state;
mod streaming;
mod two_pass;

// Re-export ValidationMode
pub use self::mode::ValidationMode;

// Re-export the redesigned validation entry point
pub use facade::{Report, Validator};

// Re-export main types
pub use context::XmlSchemaValidationContext;
pub use dom::DomSchemaValidator;
pub use lazy::LazySchemaValidator;
pub use streaming::{OnePassSchemaValidator, StreamValidator, ValidationOptions};
#[allow(deprecated)]
pub use two_pass::{DocumentSkeleton, ElementSkeleton, TwoPassSchemaValidator};

// Re-export API functions
#[allow(deprecated)]
pub use api::two_pass_validate_with_schema_location_and_fetcher;
pub use api::{
    create_xml_schema_validation_context, create_xml_schema_validation_context_from_buffer,
    get_schema_from_schema_location_with_fetcher,
    streaming_validate_with_schema_location_and_fetcher, validate_document_by_schema,
    validate_document_by_schema_context, validate_with_schema_location_and_fetcher,
};

#[allow(deprecated)]
#[cfg(feature = "ureq")]
pub use api::two_pass_validate_with_schema_location;
#[cfg(feature = "ureq")]
pub use api::{
    get_schema_from_schema_location, streaming_validate_with_schema_location,
    validate_with_schema_location,
};

#[cfg(feature = "tokio")]
pub use api::{
    get_schema_from_schema_location_async, get_schema_from_schema_location_with_async_fetcher,
    validate_with_schema_location_async, validate_with_schema_location_with_async_fetcher,
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
