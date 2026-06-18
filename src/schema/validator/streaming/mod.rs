//! One-pass streaming schema validator implementation.

mod content;
mod event_handler;
mod identity;
mod lookup;
mod occurrence;

use std::io::BufRead;
use std::sync::Arc;

use crate::error::{Result, StructuredError, ValidationErrorType};
use crate::event::StreamingParser;
use crate::schema::types::CompiledSchema;
use crate::schema::xsd::constraints::ConstraintValidator;

use super::ValidationMode;
use super::state::ValidationState;

/// Options for controlling which validations are performed.
///
/// By default, all validations are enabled. Disabling specific validations
/// can significantly improve performance for large documents.
#[derive(Debug, Clone, Default)]
#[doc(hidden)]
pub struct ValidationOptions {
    /// Skip minOccurs validation (required child element checks).
    /// Disabling this can improve performance but may miss missing required elements.
    pub skip_min_occurs: bool,

    /// Skip maxOccurs validation (element count limit checks).
    /// Disabling this can significantly improve performance (~50%) but may miss
    /// element count violations.
    pub skip_max_occurs: bool,

    /// Skip substitution group resolution in occurs validation.
    /// Disabling this can improve performance but may cause false positives/negatives
    /// for elements that use substitution groups.
    pub skip_substitution_groups: bool,

    /// Collapse identical errors into one entry with a count, keeping
    /// memory bounded on error-dense documents.
    pub aggregate_errors: bool,
}

/// One-pass streaming schema validator.
///
/// Validates XML documents against an XSD schema during streaming parsing
/// in a single pass. Best for memory-constrained environments or non-seekable streams.
#[doc(hidden)]
pub struct OnePassSchemaValidator {
    pub(crate) schema: Arc<CompiledSchema>,
    pub(crate) state: ValidationState,
    pub(crate) errors: Vec<StructuredError>,
    pub(crate) current_line: Option<usize>,
    pub(crate) current_column: Option<usize>,
    /// Constraint validator for identity constraints (unique, key, keyref)
    pub(crate) constraint_validator: ConstraintValidator,
    /// Validation mode (strict or lenient)
    pub(crate) mode: ValidationMode,
    /// Maximum number of errors to collect (0 = unlimited)
    pub(crate) max_errors: usize,
    /// Options for controlling which validations are performed
    pub(crate) options: ValidationOptions,
    /// `xs:ID` values seen in the document (for uniqueness checking)
    pub(crate) seen_ids: rustc_hash::FxHashSet<String>,
    /// `xs:IDREF` values with their locations, resolved at `finish()`
    pub(crate) pending_idrefs: Vec<(String, Option<usize>, Option<usize>)>,
    /// In-scope identity constraints being tracked
    pub(crate) identity_scopes: Vec<identity::ScopeState>,
    /// Memoized facet constraints per named simple type
    pub(crate) facet_cache: crate::schema::xsd::facets::FacetCache,
    /// Memoized inherited-element lists per complex type name
    pub(crate) elements_cache:
        rustc_hash::FxHashMap<String, std::sync::Arc<Vec<crate::schema::types::ElementDef>>>,
    /// Interned error strings (messages repeat heavily on invalid files)
    pub(crate) error_strings: rustc_hash::FxHashSet<std::sync::Arc<str>>,
    /// (message, node_name) -> index into `errors`, used when
    /// `aggregate_errors` is on.
    pub(crate) aggregate_index: crate::error::ErrorAggregateIndex,
    /// Interned element/namespace names, so per-element qualified names and
    /// namespace URIs don't allocate on every start tag.
    pub(crate) name_pool: rustc_hash::FxHashSet<std::sync::Arc<str>>,
    /// Reusable buffer for building qualified names.
    pub(crate) qname_buf: String,
}

impl OnePassSchemaValidator {
    /// Creates a new one-pass validator in strict mode.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            state: ValidationState::new(),
            errors: Vec::new(),
            current_line: None,
            current_column: Some(1),
            constraint_validator: ConstraintValidator::new(),
            mode: ValidationMode::Strict,
            max_errors: 0,
            options: ValidationOptions::default(),
            seen_ids: rustc_hash::FxHashSet::default(),
            pending_idrefs: Vec::new(),
            identity_scopes: Vec::new(),
            facet_cache: Default::default(),
            elements_cache: Default::default(),
            error_strings: Default::default(),
            aggregate_index: Default::default(),
            name_pool: Default::default(),
            qname_buf: String::new(),
        }
    }

    /// Sets the validation mode (builder pattern).
    pub fn set_mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the maximum number of errors to collect (builder pattern).
    ///
    /// Set to 0 for unlimited errors (default).
    /// Collapses identical errors into one entry with a count (builder).
    pub fn with_aggregate_errors(mut self) -> Self {
        self.options.aggregate_errors = true;
        self
    }

    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Validates an XML document from a reader and returns validation errors.
    ///
    /// This is a convenience method that internally creates a `StreamingParser`,
    /// runs validation, and returns the collected errors.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::fs::File;
    /// use std::io::BufReader;
    /// use std::sync::Arc;
    /// use fastxml::schema::validator::OnePassSchemaValidator;
    ///
    /// let file = File::open("document.xml")?;
    /// let reader = BufReader::new(file);
    ///
    /// let errors = OnePassSchemaValidator::new(schema)
    ///     .with_max_errors(100)
    ///     .validate(reader)?;
    /// ```
    pub fn validate<R: BufRead>(self, reader: R) -> Result<Vec<StructuredError>> {
        let mut parser = StreamingParser::new(reader);
        parser.add_handler(Box::new(self));
        parser.parse()?;

        // Extract the validator from the parser to get errors
        let handlers = parser.into_handlers();
        for handler in handlers {
            if let Ok(validator) = handler.as_any().downcast::<OnePassSchemaValidator>() {
                return Ok(validator.into_errors());
            }
        }

        // Fallback: return empty errors (should not happen)
        Ok(Vec::new())
    }

    /// Returns collected validation errors.
    pub fn errors(&self) -> &[StructuredError] {
        &self.errors
    }

    /// Takes ownership of collected errors.
    pub fn into_errors(self) -> Vec<StructuredError> {
        self.errors
    }

    pub(crate) fn should_collect_more(&self) -> bool {
        self.max_errors == 0 || self.errors.len() < self.max_errors
    }

    pub(crate) fn add_error(&mut self, error: StructuredError) {
        let error = error.interned(&mut self.error_strings);
        if self.options.aggregate_errors {
            // Identical errors collapse into the first occurrence's entry.
            // Messages are interned, so equal content means equal Arcs.
            let key = (
                std::sync::Arc::clone(&error.message),
                error.node_name.clone(),
            );
            if let Some(&idx) = self.aggregate_index.get(&key) {
                self.errors[idx].record_occurrence(error.location.line, error.location.column);
                return;
            }
            if self.should_collect_more() {
                self.aggregate_index.insert(key, self.errors.len());
                self.errors.push(error);
            }
            return;
        }
        if self.should_collect_more() {
            self.errors.push(error);
        }
    }

    pub(crate) fn make_error(
        &self,
        error_type: ValidationErrorType,
        message: impl Into<String>,
    ) -> StructuredError {
        let mut error = StructuredError::new(message, error_type);
        if let Some(line) = self.current_line {
            error = error.with_line(line);
        }
        if let Some(column) = self.current_column {
            error = error.with_column(column);
        }
        error = error.with_element_path(self.state.element_path());
        error
    }
}

/// Error-introspection helpers used only by the in-crate engine tests.
#[cfg(test)]
impl OnePassSchemaValidator {
    /// Sets the maximum number of errors to collect (setter pattern).
    pub fn set_max_errors(&mut self, max: usize) {
        self.max_errors = max;
    }

    /// Returns only errors (excludes warnings).
    pub fn errors_only(&self) -> Vec<&StructuredError> {
        self.errors.iter().filter(|e| e.is_error()).collect()
    }

    /// Returns only warnings.
    pub fn warnings(&self) -> Vec<&StructuredError> {
        self.errors.iter().filter(|e| e.is_warning()).collect()
    }

    /// Returns true if validation passed without errors (warnings are OK).
    pub fn is_valid(&self) -> bool {
        !self.errors.iter().any(|e| e.is_error())
    }

    /// Returns true if there are no errors or warnings.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the error count (excluding warnings).
    pub fn error_count(&self) -> usize {
        self.errors.iter().filter(|e| e.is_error()).count()
    }

    /// Returns the warning count.
    pub fn warning_count(&self) -> usize {
        self.errors.iter().filter(|e| e.is_warning()).count()
    }
}

#[cfg(test)]
mod tests;
