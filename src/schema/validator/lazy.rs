//! Streaming validator that loads its schema from the root element's hints.

use std::sync::Arc;

use crate::error::{Result, StructuredError};
use crate::event::{RawEvent, XmlEventHandler};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::hints::SchemaHints;

use super::api::{EngineSettings, load_hinted_schema, no_hints_error};
use super::streaming::OnePassSchemaValidator;

enum Stage {
    /// No start tag seen yet.
    AwaitingRoot,
    /// The hinted schema loaded; every event goes to the inner validator.
    Validating(Box<OnePassSchemaValidator>),
    /// The hinted schema could not be loaded; the reasons are in
    /// `diagnostics` and the content is not validated.
    SchemaUnavailable,
}

/// Wraps [`OnePassSchemaValidator`]: on the root start tag it reads the
/// schema-location hints, loads the schema (see [`super::api`] for the
/// policy), then forwards every event — and `finish` — to the inner
/// validator. Its entries are the load diagnostics followed by the inner
/// validator's errors, each produced exactly once.
pub(crate) struct AutodetectStreamingValidator<F: SchemaFetcher> {
    fetcher: F,
    settings: EngineSettings,
    stage: Stage,
    diagnostics: Vec<StructuredError>,
}

impl<F: SchemaFetcher> AutodetectStreamingValidator<F> {
    pub fn new(fetcher: F, settings: EngineSettings) -> Self {
        Self {
            fetcher,
            settings,
            stage: Stage::AwaitingRoot,
            diagnostics: Vec::new(),
        }
    }

    /// All collected entries: schema-load diagnostics, then validation errors.
    pub fn into_entries(self) -> Vec<StructuredError> {
        let mut entries = self.diagnostics;
        if let Stage::Validating(v) = self.stage {
            entries.extend(v.into_errors());
        }
        entries
    }
}

impl<F: SchemaFetcher + 'static> XmlEventHandler for AutodetectStreamingValidator<F> {
    fn handle(&mut self, event: &RawEvent<'_>) -> Result<()> {
        if let (
            Stage::AwaitingRoot,
            RawEvent::StartElement {
                attributes,
                namespace_decls,
                ..
            },
        ) = (&self.stage, event)
        {
            let hints = SchemaHints::from_start_tag(attributes, namespace_decls);
            self.stage = match load_hinted_schema(&hints, &self.fetcher) {
                Ok(schema) => {
                    Stage::Validating(Box::new(self.settings.streaming(Arc::new(schema))))
                }
                Err(errors) => {
                    self.diagnostics.extend(errors);
                    Stage::SchemaUnavailable
                }
            };
        }

        if let Stage::Validating(v) = &mut self.stage {
            v.handle(event)?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        match &mut self.stage {
            Stage::Validating(v) => v.finish(),
            Stage::AwaitingRoot => {
                // No root element, hence no hints.
                self.diagnostics.push(no_hints_error());
                Ok(())
            }
            Stage::SchemaUnavailable => Ok(()),
        }
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ValidationErrorType;
    use crate::schema::ValidationMode;
    use crate::schema::fetcher::NoopFetcher;

    #[test]
    fn root_without_hints_yields_one_schema_error() {
        let mut validator = AutodetectStreamingValidator::new(
            NoopFetcher,
            EngineSettings {
                mode: ValidationMode::Strict,
                max_errors: None,
                aggregate_errors: false,
            },
        );

        validator
            .handle(&RawEvent::StartElement {
                name: "root",
                prefix: None,
                attributes: &[],
                namespace_decls: &[],
                line: None,
                column: Some(1),
            })
            .unwrap();
        validator
            .handle(&RawEvent::EndElement {
                name: "root",
                prefix: None,
            })
            .unwrap();
        validator.finish().unwrap();

        let entries = validator.into_entries();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert!(entries[0].is_error());
        assert_eq!(entries[0].error_type, ValidationErrorType::SchemaNotFound);
    }
}
