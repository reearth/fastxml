//! XML schema validators.
//!
//! The public entry point is [`Validator`], which selects the engine from the
//! input type: a DOM tree validator for `&XmlDocument`, and a one-pass streaming
//! validator for `&str` / `&[u8]` / a reader. The engines themselves live in the
//! private submodules below.
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
mod attributes;
mod context;
mod dom;
mod facade;
mod lazy;
mod state;
mod streaming;
mod xsi_type;

// The public validation surface is the `Validator` front door and its `Report`,
// plus `ValidationMode`. The engine types (DomSchemaValidator,
// OnePassSchemaValidator, LazySchemaValidator, XmlSchemaValidationContext) and
// the `validate_*` / `get_schema_*` / `create_xml_schema_validation_context*`
// free functions live in the private `dom` / `streaming` / `lazy` / `api`
// submodules and are reached internally via `super::`.
pub use self::mode::ValidationMode;
pub use facade::{Report, Validator};

/// Work counters incremented unconditionally by the streaming validator.
///
/// These exist as an anti-regression guardrail: an "optimization" that
/// silently skips work would change these counts. Every performance
/// comparison must show identical counter values before and after.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationCounters {
    /// Elements passed to `validate_element` (incremented unconditionally at
    /// the top, before any lookup or early return).
    pub elements_validated: u64,
    /// Text nodes passed to `validate_text_content_against_type`.
    pub text_nodes_checked: u64,
}

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
