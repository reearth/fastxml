//! XML schema validation.
//!
//! The public entry point is [`Validator`], which selects the engine from the
//! input type: a DOM tree validator for `&XmlDocument`, and a one-pass streaming
//! validator for `&str` / `&[u8]` / a reader. The schema is either given
//! explicitly or loaded from the document's `xsi:schemaLocation` /
//! `xsi:noNamespaceSchemaLocation` hints; see [`Validator::run`] for what
//! happens when that schema cannot be loaded. Results come back as a
//! [`Report`].

mod api;
mod attributes;
mod dom;
mod facade;
mod lazy;
mod state;
mod streaming;
mod xsi_type;

// Internal layout (maintainer note): `dom` and `streaming` are the two
// engines, `state` holds streaming per-element state, `api` + `lazy`
// implement schema auto-detection from the document's hints, and `facade`
// is the public `Validator` / `Report` surface.
pub use self::mode::ValidationMode;
pub use facade::{Report, Validator};

/// Counts of work done by the streaming validator during one run.
///
/// Exposed for benchmarking tools; not part of the stable API.
// Maintainer note: these exist as an anti-regression guardrail — an
// "optimization" that silently skips work changes the counts, so every
// performance comparison must show identical values before and after.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationCounters {
    /// Number of elements validated.
    pub elements_validated: u64,
    /// Number of text nodes checked against a simple type.
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
