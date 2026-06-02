//! Two-pass streaming schema validator.
//!
//! This module provides a two-pass validation approach:
//! 1. Pass 1: Build a lightweight skeleton of the document structure
//! 2. Pass 2: Perform batch validation using the skeleton
//!
//! This approach can be more efficient for large documents as it allows
//! batch processing of occurrence counts and other constraints.

mod builder;
mod skeleton;
mod validation;

use std::io::{BufRead, Seek, SeekFrom};
use std::sync::Arc;

use crate::error::{Result, StructuredError};
use crate::event::StreamingParser;
use crate::schema::types::CompiledSchema;

pub use self::skeleton::{DocumentSkeleton, ElementSkeleton};

use self::builder::SkeletonBuilder;
use super::ValidationMode;
use super::streaming::ValidationOptions;

/// Two-pass streaming schema validator.
///
/// This validator performs validation in two passes:
/// 1. Build a lightweight skeleton of the document structure
/// 2. Validate the skeleton against the schema
///
/// # Example
///
/// ```ignore
/// use std::fs::File;
/// use std::io::BufReader;
/// use fastxml::schema::validator::TwoPassSchemaValidator;
///
/// let file = File::open("document.xml")?;
/// let reader = BufReader::new(file);
///
/// let errors = TwoPassSchemaValidator::new(schema)
///     .with_max_errors(100)
///     .validate(reader)?;
/// ```
#[deprecated(
    since = "0.5.0",
    note = "TwoPassSchemaValidator is no longer recommended. Use OnePassSchemaValidator (or StreamValidator) instead for better performance."
)]
#[doc(hidden)]
pub struct TwoPassSchemaValidator {
    pub(crate) schema: Arc<CompiledSchema>,
    pub(crate) options: ValidationOptions,
    pub(crate) mode: ValidationMode,
    pub(crate) max_errors: usize,
}

#[allow(deprecated)]
impl TwoPassSchemaValidator {
    /// Creates a new two-pass validator.
    pub fn new(schema: Arc<CompiledSchema>) -> Self {
        Self {
            schema,
            options: ValidationOptions::default(),
            mode: ValidationMode::Strict,
            max_errors: 0,
        }
    }

    /// Sets the validation mode.
    pub fn with_mode(mut self, mode: ValidationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the validation options.
    pub fn with_options(mut self, options: ValidationOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets the maximum number of errors to collect.
    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Performs two-pass validation and returns any errors found.
    pub fn validate<R: BufRead + Seek>(self, mut reader: R) -> Result<Vec<StructuredError>> {
        // Pass 1: Build skeleton
        let skeleton = Self::build_skeleton(&mut reader)?;

        // Seek back to start for potential future use or debugging
        let _ = reader.seek(SeekFrom::Start(0));

        // Pass 2: Validate with skeleton
        self.validate_with_skeleton(&skeleton)
    }

    /// Pass 1: Builds the document skeleton.
    fn build_skeleton<R: BufRead>(reader: &mut R) -> Result<DocumentSkeleton> {
        let mut parser = StreamingParser::new(reader);
        let builder = SkeletonBuilder::new();
        parser.add_handler(Box::new(builder));
        parser.parse()?;

        // Extract the builder from the parser
        let handlers = parser.into_handlers();
        for handler in handlers {
            if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
                return Ok(builder.into_skeleton());
            }
        }

        // Fallback: return empty skeleton
        Ok(DocumentSkeleton::new())
    }

    /// Pass 2: Validates the document using the pre-built skeleton.
    fn validate_with_skeleton(&self, skeleton: &DocumentSkeleton) -> Result<Vec<StructuredError>> {
        let mut errors = Vec::new();

        // Start validation from root
        if let Some(root_index) = skeleton.root_index {
            self.validate_node_recursive(skeleton, root_index, None, &mut errors);
        }

        Ok(errors)
    }
}

#[cfg(test)]
mod tests;
