//! XmlEventHandler implementation for streaming validation.

use std::sync::Arc;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{XmlEvent, XmlEventHandler};

use super::OnePassSchemaValidator;

impl XmlEventHandler for OnePassSchemaValidator {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                namespace,
                attributes,
                namespace_decls,
                line,
                column,
            } => {
                self.current_line = *line;
                self.current_column = *column;
                self.state.push_namespaces(namespace_decls);
                // Use prefixed name to distinguish elements with same local name but different namespaces
                // e.g., gml:boundedBy vs brid:boundedBy
                let qualified_name = match prefix {
                    Some(p) if !p.is_empty() => Arc::from(format!("{}:{}", p, name)),
                    _ => Arc::clone(name),
                };
                self.state.push_element(
                    qualified_name,
                    namespace.as_ref().map(|s| Arc::from(s.as_str())),
                );
                let attrs: Vec<(&str, &str)> = attributes
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                // Pass Arc<str> references directly
                self.validate_element(name, prefix.as_ref(), namespace.as_deref(), &attrs);
            }
            XmlEvent::EndElement { name, .. } => {
                self.validate_element_end(name);
                self.state.pop_namespaces();
            }
            XmlEvent::Text(text) => {
                self.validate_text_content(text);
            }
            XmlEvent::CData(text) => {
                self.validate_text_content(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        // Final validation checks - report unclosed elements
        while let Some(ctx) = self.state.pop_element() {
            let error = StructuredError::new(
                format!("element '{}' is not closed", ctx.name),
                ValidationErrorType::UnclosedElement,
            )
            .with_node_name(ctx.name.as_ref())
            .with_level(ErrorLevel::Error);
            self.add_error(error);
        }

        // Validate keyref constraints
        if let Err(constraint_errors) = self.constraint_validator.validate_keyrefs() {
            for err in constraint_errors {
                let error =
                    StructuredError::new(err.to_string(), ValidationErrorType::IdentityConstraint)
                        .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }

        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
