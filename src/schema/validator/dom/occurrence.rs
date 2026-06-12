//! Occurrence validation (min/max occurs, sequence order) for DOM validation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{ErrorLevel, StructuredError, ValidationErrorType};
use crate::node::XmlNode;
use crate::schema::types::{ContentModelType, FlattenedChildren};
use crate::schema::xsd::content_automaton::{AutomatonState, FinishError, StepResult};

use super::DomSchemaValidator;

impl DomSchemaValidator {
    /// Validates this element's children against the type's content-model
    /// automaton. Returns `true` when an automaton was present (the
    /// count-based batch checks should then be skipped).
    pub(crate) fn validate_with_automaton(
        &self,
        node: &XmlNode,
        child_counts: &HashMap<String, u32>,
        flattened: &FlattenedChildren,
        nilled: bool,
        errors: &mut Vec<StructuredError>,
    ) -> bool {
        let Some(automaton) = &flattened.automaton else {
            return false;
        };
        if nilled {
            // Emptiness of nilled elements is enforced separately.
            return true;
        }

        let node_name = node.get_name();
        let mut st = AutomatonState::default();

        for child in node.get_child_elements() {
            let local = child.get_name();
            let qname = match child.get_prefix() {
                Some(p) if !p.is_empty() => format!("{}:{}", p, local),
                _ => local.clone(),
            };
            let ns = child.get_namespace_uri();

            match automaton.step(&mut st, &qname, &local, ns.as_deref()) {
                StepResult::Matched => {}
                StepResult::TooMany { max } => {
                    if !self.options.skip_max_occurs {
                        let error = self
                            .make_error(
                                ValidationErrorType::TooManyOccurrences,
                                format!(
                                    "element '{}' occurs more than the allowed {} time(s) in '{}'",
                                    qname, max, node_name
                                ),
                                &child,
                            )
                            .with_node_name(&qname)
                            .with_level(ErrorLevel::Error);
                        self.push_error(errors, error);
                    }
                }
                StepResult::NotExpected { expected } => {
                    // Undeclared children get their own "not declared"
                    // error from the declaration checks; only declared
                    // elements are reported as content-model violations
                    // (mirrors the streaming validator).
                    let declared = flattened.constraints.contains_key(&qname)
                        || flattened.constraints.contains_key(&local)
                        || self.schema.get_element(&qname).is_some();
                    if declared {
                        let mut expected_list = expected.join(", ");
                        if expected_list.is_empty() {
                            expected_list = "no further elements".to_string();
                        }
                        let error = self
                            .make_error(
                                ValidationErrorType::InvalidContent,
                                format!(
                                    "element '{}' is not expected here in '{}' (expected: {})",
                                    qname, node_name, expected_list
                                ),
                                &child,
                            )
                            .with_node_name(&qname)
                            .with_expected(expected_list)
                            .with_found(qname.clone())
                            .with_level(ErrorLevel::Error);
                        self.push_error(errors, error);
                    }
                }
            }
        }

        if let Err(err) = automaton.finish(&st)
            && !self.options.skip_min_occurs
        {
            let error = match err {
                FinishError::TooFew { name, min, found } => self
                    .make_error(
                        ValidationErrorType::TooFewOccurrences,
                        format!(
                            "element {} in '{}' occurs {} time(s), but minimum is {}",
                            name, node_name, found, min
                        ),
                        node,
                    )
                    .with_node_name(&node_name)
                    .with_level(ErrorLevel::Error),
                FinishError::Missing { expected } => {
                    let occurred = expected
                        .iter()
                        .any(|n| self.get_total_count(child_counts, n) > 0);
                    let expected_list = expected.join(", ");
                    let (error_type, message) = if occurred {
                        (
                            ValidationErrorType::TooFewOccurrences,
                            format!(
                                "element '{}' has incomplete content: more of {} required",
                                node_name, expected_list
                            ),
                        )
                    } else {
                        (
                            ValidationErrorType::MissingRequiredElement,
                            format!(
                                "element '{}' has incomplete content (expected: {})",
                                node_name, expected_list
                            ),
                        )
                    };
                    self.make_error(error_type, message, node)
                        .with_node_name(&node_name)
                        .with_expected(expected_list)
                        .with_level(ErrorLevel::Error)
                }
            };
            self.push_error(errors, error);
        }
        true
    }

    /// Batch validates min_occurs for all children.
    pub(crate) fn validate_min_occurs_batch(
        &self,
        node: &XmlNode,
        child_counts: &HashMap<String, u32>,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_min_occurs {
            return;
        }

        let node_name = node.get_name();

        // For Choice content model
        if flattened.content_model_type == ContentModelType::Choice {
            let any_choice_present = flattened
                .constraints
                .keys()
                .any(|child_name| self.get_total_count(child_counts, child_name) > 0);

            if !any_choice_present && !flattened.constraints.is_empty() {
                let choices: Vec<_> = flattened.constraints.keys().cloned().collect();
                let error = self
                    .make_error(
                        ValidationErrorType::MissingRequiredElement,
                        format!(
                            "element '{}' requires one of: {}",
                            node_name,
                            choices.join(", ")
                        ),
                        node,
                    )
                    .with_node_name(&node_name)
                    .with_expected(format!("one of: {}", choices.join(", ")))
                    .with_found("none".to_string())
                    .with_level(ErrorLevel::Error);

                self.push_error(errors, error);
            }
            return;
        }

        // For Sequence/All content models
        for (child_name, &(min_occurs, _)) in &flattened.constraints {
            if min_occurs > 0 {
                let actual_count = self.get_total_count(child_counts, child_name);

                if actual_count < min_occurs {
                    let error_type = if actual_count == 0 {
                        ValidationErrorType::MissingRequiredElement
                    } else {
                        ValidationErrorType::TooFewOccurrences
                    };

                    let error = self
                        .make_error(
                            error_type,
                            format!(
                                "element '{}' requires child '{}' at least {} time(s), but found {}",
                                node_name, child_name, min_occurs, actual_count
                            ),
                            node,
                        )
                        .with_node_name(&node_name)
                        .with_expected(format!(
                            "at least {} occurrence(s) of '{}'",
                            min_occurs, child_name
                        ))
                        .with_found(format!("{} occurrence(s)", actual_count))
                        .with_level(ErrorLevel::Error);

                    self.push_error(errors, error);
                }
            }
        }
    }

    /// Batch validates max_occurs for all children.
    pub(crate) fn validate_max_occurs_batch(
        &self,
        node: &XmlNode,
        child_counts: &HashMap<String, u32>,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        if self.options.skip_max_occurs {
            return;
        }

        for (child_name, &(_, max_occurs)) in &flattened.constraints {
            if let Some(max) = max_occurs {
                let total_count = self.get_total_count(child_counts, child_name);

                if total_count > max {
                    let error = self
                        .make_error(
                            ValidationErrorType::TooManyOccurrences,
                            format!(
                                "element '{}' (or substitutes) occurs {} times, but maximum is {}",
                                child_name, total_count, max
                            ),
                            node,
                        )
                        .with_node_name(child_name)
                        .with_level(ErrorLevel::Error);

                    self.push_error(errors, error);
                }
            }
        }
    }

    /// Validates that child elements appear in the correct sequence order.
    pub(crate) fn validate_sequence_order(
        &self,
        node: &XmlNode,
        flattened: &FlattenedChildren,
        errors: &mut Vec<StructuredError>,
    ) {
        // Only validate sequence content models
        if flattened.content_model_type != ContentModelType::Sequence {
            return;
        }

        // Skip if no ordered elements defined
        if flattened.ordered_elements.is_empty() {
            return;
        }

        // Get actual child element names in order
        let actual_children: Vec<String> = node
            .get_child_elements()
            .iter()
            .map(|c| c.get_name())
            .collect();

        // Track position in expected sequence
        let mut expected_index = 0;

        for actual_name in &actual_children {
            // Find the position of this element in the expected sequence (starting from current position)
            let found_pos = flattened.ordered_elements[expected_index..]
                .iter()
                .position(|e| e == actual_name)
                .map(|p| expected_index + p);

            if let Some(pos) = found_pos {
                expected_index = pos;
            } else {
                // Check if this element exists earlier in the sequence (out of order)
                let earlier_pos = flattened.ordered_elements[..expected_index]
                    .iter()
                    .position(|e| e == actual_name);

                if earlier_pos.is_some() {
                    // Element is out of order
                    let node_name = node.get_name();
                    let expected_after = if expected_index > 0 {
                        flattened.ordered_elements[expected_index - 1].clone()
                    } else {
                        "(beginning)".to_string()
                    };

                    let error = self
                        .make_error(
                            ValidationErrorType::InvalidContent,
                            format!(
                                "element '{}' in '{}' appears out of sequence order (expected after '{}')",
                                actual_name, node_name, expected_after
                            ),
                            node,
                        )
                        .with_node_name(&node_name)
                        .with_level(ErrorLevel::Error);

                    self.push_error(errors, error);
                    return;
                }
            }
        }
    }

    /// Gets the total count for an element including substitution group members.
    pub(crate) fn get_total_count(
        &self,
        child_counts: &HashMap<String, u32>,
        child_name: &str,
    ) -> u32 {
        let mut count = child_counts.get(child_name).copied().unwrap_or(0);

        // Try with local name if child_name has a prefix
        if let Some((_prefix, local)) = child_name.split_once(':') {
            count += child_counts.get(local).copied().unwrap_or(0);
        }

        // Add counts from substitution group members (unless skipped)
        if !self.options.skip_substitution_groups {
            let all_members = self.get_all_substitution_members(child_name);
            for member in all_members.iter() {
                count += child_counts.get(member).copied().unwrap_or(0);
            }
        }

        count
    }

    /// Gets all substitution group members for a head element.
    #[inline]
    pub(crate) fn get_all_substitution_members(&self, head_name: &str) -> Arc<Vec<String>> {
        // Fast path: direct cache lookup
        if let Some(members) = self.schema.transitive_substitution_groups.get(head_name) {
            return Arc::clone(members);
        }

        // Try with local name if head_name has a prefix
        if let Some((_prefix, local)) = head_name.split_once(':') {
            if let Some(members) = self.schema.transitive_substitution_groups.get(local) {
                return Arc::clone(members);
            }
        }

        Arc::new(Vec::new())
    }
}
