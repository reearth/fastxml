//! Occurrence validation (min/max occurs, sequence order) for DOM validation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{ErrorLevel, StructuredError, ValidationErrorType};
use crate::node::XmlNode;
use crate::schema::types::{ContentModelType, FlattenedChildren};

use super::DomSchemaValidator;

impl DomSchemaValidator {
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

                if self.should_add_error(errors) {
                    errors.push(error);
                }
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

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
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

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
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

                    if self.should_add_error(errors) {
                        errors.push(error);
                    }
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
