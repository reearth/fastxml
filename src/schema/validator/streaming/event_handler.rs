//! XmlEventHandler implementation for streaming validation.

use std::sync::Arc;

use crate::error::{ErrorLevel, Result, StructuredError, ValidationErrorType};
use crate::event::{RawEvent, XmlEventHandler};

use super::OnePassSchemaValidator;

impl XmlEventHandler for OnePassSchemaValidator {
    fn handle(&mut self, event: &RawEvent<'_>) -> Result<()> {
        match event {
            RawEvent::StartElement {
                name,
                prefix,
                attributes,
                namespace_decls,
                line,
                column,
            } => {
                self.current_line = *line;
                self.current_column = *column;
                self.state.push_namespaces(namespace_decls);
                // Use the prefixed name to distinguish elements with the same
                // local name but different namespaces (gml:boundedBy vs
                // brid:boundedBy). Intern the qualified name once — its record
                // links to the eagerly-interned local-name symbol, so both the
                // qname and local symbols are obtained with a single probe.
                let qname_sym = match prefix {
                    Some(p) if !p.is_empty() => {
                        self.qname_buf.clear();
                        self.qname_buf.push_str(p);
                        self.qname_buf.push(':');
                        self.qname_buf.push_str(name);
                        self.symbols.intern(&self.qname_buf)
                    }
                    _ => self.symbols.intern(name),
                };
                let name_sym = self.symbols.local(qname_sym);
                let name_arc = Arc::clone(self.symbols.arc(name_sym));
                let qname_arc = Arc::clone(self.symbols.arc(qname_sym));
                // Streaming events carry no resolved namespace URI.
                self.state
                    .push_element_sym(Arc::clone(&qname_arc), None, qname_sym.0);
                let attrs: smallvec::SmallVec<[(&str, &str); 8]> =
                    attributes.iter().map(|(k, v)| (*k, v.as_ref())).collect();
                self.validate_element(&name_arc, prefix.as_deref(), &qname_arc, None, &attrs);
            }
            RawEvent::EndElement { .. } => {
                // The closing name is unused — the element being closed is the
                // top of the state stack — so no interning is needed here.
                self.validate_element_end();
                self.state.pop_namespaces();
            }
            RawEvent::Text(text) => {
                self.validate_text_content(text);
            }
            RawEvent::CData(text) => {
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

        // Resolve IDREF references against the IDs seen in the document
        let unresolved: Vec<_> = self
            .pending_idrefs
            .drain(..)
            .filter(|(idref, _, _)| !self.seen_ids.contains(idref))
            .collect();
        for (idref, line, column) in unresolved {
            let mut error = StructuredError::new(
                format!("IDREF '{}' does not match any ID in the document", idref),
                ValidationErrorType::IdentityConstraint,
            )
            .with_level(ErrorLevel::Error);
            if let Some(line) = line {
                error = error.with_line(line);
            }
            if let Some(column) = column {
                error = error.with_column(column);
            }
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
