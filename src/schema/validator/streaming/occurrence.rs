//! Occurrence validation (min/max occurs, sequence order, substitution groups).

use std::sync::Arc;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::ContentModelType;

use super::super::state::ElementContext;
use super::OnePassSchemaValidator;

impl OnePassSchemaValidator {
    /// Validates max_occurs constraint for a child element.
    ///
    /// This method also considers substitution groups. If the child element is a
    /// substitution group member, the constraint from the head element is used,
    /// and the total count includes all members of the substitution group (transitively).
    pub(crate) fn validate_max_occurs(&mut self, child_name: &str) {
        // Skip if disabled via options
        if self.options.skip_max_occurs {
            return;
        }

        if self.state.element_stack.len() < 2 {
            return;
        }

        let parent_idx = self.state.element_stack.len() - 2;
        if let Some(parent) = self.state.element_stack.get(parent_idx) {
            // First, try to find which substitution group head this element belongs to
            let head_name = if self.options.skip_substitution_groups {
                None
            } else {
                self.find_substitution_group_head(child_name)
            };

            // Determine the constraint source (head or direct)
            let constraint_name = head_name.as_deref().unwrap_or(child_name);

            // Check against parent's expected children constraints
            if let Some((_, max_occurs)) = parent.get_child_constraint(constraint_name) {
                if let Some(max) = max_occurs {
                    // Calculate total count including all substitution group members (transitively)
                    let mut total_count = parent.get_child_count(constraint_name);

                    // Also try with local name if constraint_name has a prefix
                    if let Some((_prefix, local)) = constraint_name.split_once(':') {
                        total_count += parent.get_child_count(local);
                    }

                    // Add counts from all transitive members (unless skipped)
                    if !self.options.skip_substitution_groups {
                        let all_members = self.get_all_substitution_members(constraint_name);
                        for member in all_members.iter() {
                            total_count += parent.get_child_count(member);
                        }
                    }

                    if total_count > max {
                        let error = self
                            .make_error(
                                ValidationErrorType::TooManyOccurrences,
                                format!(
                                    "element '{}' (or substitutes) occurs {} times, but maximum is {}",
                                    constraint_name, total_count, max
                                ),
                            )
                            .with_node_name(child_name)
                            .with_level(ErrorLevel::Error);
                        self.add_error(error);
                    }
                }
            }
        }
    }

    /// Validates sequence order constraint for a child element.
    ///
    /// This method checks that child elements appear in the correct sequence order
    /// as defined by the parent's content model.
    pub(crate) fn validate_sequence_order(&mut self, child_name: &str) {
        if self.state.element_stack.len() < 2 {
            return;
        }

        let parent_idx = self.state.element_stack.len() - 2;

        // Get parent's flattened children to check sequence order
        let (content_model_type, ordered_elements, current_index) = {
            let parent = match self.state.element_stack.get(parent_idx) {
                Some(p) => p,
                None => return,
            };

            let fc = match &parent.flattened_children {
                Some(fc) => fc,
                None => return,
            };

            // Only validate for sequence content models
            if fc.content_model_type != ContentModelType::Sequence {
                return;
            }

            // Skip if no ordered elements defined
            if fc.ordered_elements.is_empty() {
                return;
            }

            (
                fc.content_model_type,
                fc.ordered_elements.clone(),
                parent.sequence_index,
            )
        };

        // Only validate for sequence content models
        if content_model_type != ContentModelType::Sequence {
            return;
        }

        // Find the position of this element in the expected sequence (starting from current position)
        let found_pos = ordered_elements[current_index..]
            .iter()
            .position(|e| e.as_str() == child_name)
            .map(|p| current_index + p);

        if let Some(new_index) = found_pos {
            // Update parent's sequence index
            if let Some(parent) = self.state.element_stack.get_mut(parent_idx) {
                parent.sequence_index = new_index;
            }
        } else {
            // Check if this element exists earlier in the sequence (out of order)
            let earlier_pos = ordered_elements[..current_index]
                .iter()
                .position(|e| e.as_str() == child_name);

            if earlier_pos.is_some() {
                // Element is out of order
                let expected_after = if current_index > 0 {
                    ordered_elements[current_index - 1].clone()
                } else {
                    "(beginning)".to_string()
                };

                // Get parent name for error message
                let parent_name = self
                    .state
                    .element_stack
                    .get(parent_idx)
                    .map(|p| p.name.to_string())
                    .unwrap_or_default();

                let error = self
                    .make_error(
                        ValidationErrorType::InvalidContent,
                        format!(
                            "element '{}' in '{}' appears out of sequence order (expected after '{}')",
                            child_name, parent_name, expected_after
                        ),
                    )
                    .with_node_name(&parent_name)
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
        }
    }

    /// Finds the substitution group head for a given element name.
    ///
    /// Returns Some(head_name) if the element is a member of a substitution group,
    /// or None if it's not a member of any substitution group.
    /// Uses the pre-computed substitution_group_heads cache for O(1) lookup.
    #[inline]
    pub(crate) fn find_substitution_group_head(&self, element_name: &str) -> Option<String> {
        // Fast path: direct cache lookup (most common case)
        if let Some(head) = self.schema.substitution_group_heads.get(element_name) {
            return Some(head.clone());
        }

        // Try with local name if element_name has a prefix
        if let Some((_prefix, local)) = element_name.split_once(':') {
            if let Some(head) = self.schema.substitution_group_heads.get(local) {
                return Some(head.clone());
            }
        }

        // Check if the element itself declares a substitution_group
        // This is needed for elements not in the pre-computed cache
        if let Some(elem) = self.schema.get_element(element_name) {
            if let Some(ref sg) = elem.substitution_group {
                return Some(sg.clone());
            }
        }

        // No substitution group found - this is the common case for most elements
        None
    }

    /// Gets all substitution group members for a head element, including transitive members.
    ///
    /// Uses the pre-computed transitive_substitution_groups cache for O(1) lookup.
    /// Returns empty Vec if not found in cache (most elements are not substitution group heads).
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

        // Not a substitution group head - return empty (common case)
        Arc::new(Vec::new())
    }

    /// Validates that all required child elements are present (minOccurs).
    ///
    /// This method also considers substitution group members when counting occurrences.
    /// If the expected child is a substitution group head, occurrences of any member
    /// element (including transitive members) are counted toward the head's requirement.
    pub(crate) fn validate_min_occurs(&mut self, ctx: &ElementContext) {
        // Skip if disabled via options
        if self.options.skip_min_occurs {
            return;
        }

        let flattened = match &ctx.flattened_children {
            Some(f) => f,
            None => return, // No constraints to validate
        };

        // For Choice content model, we only need ONE of the choices to be present
        if flattened.content_model_type == ContentModelType::Choice {
            // Check if at least one choice element is present
            let mut any_choice_present = false;

            for child_name in flattened.constraints.keys() {
                let mut count = ctx.get_child_count(child_name);

                // Also try with local name if child_name has a prefix
                if let Some((_prefix, local)) = child_name.split_once(':') {
                    count += ctx.get_child_count(local);
                }

                // Add counts from substitution group members (unless skipped)
                if !self.options.skip_substitution_groups {
                    let all_members = self.get_all_substitution_members(child_name);
                    for member in all_members.iter() {
                        count += ctx.get_child_count(member);
                    }
                }

                if count > 0 {
                    any_choice_present = true;
                    break;
                }
            }

            // Only report error if no choice element is present and there are expected children
            if !any_choice_present && !flattened.constraints.is_empty() {
                let choices: Vec<_> = flattened.constraints.keys().cloned().collect();
                let error = self
                    .make_error(
                        ValidationErrorType::MissingRequiredElement,
                        format!(
                            "element '{}' requires one of: {}",
                            ctx.name,
                            choices.join(", ")
                        ),
                    )
                    .with_node_name(ctx.name.as_ref())
                    .with_expected(format!("one of: {}", choices.join(", ")))
                    .with_found("none".to_string())
                    .with_level(ErrorLevel::Error);
                self.add_error(error);
            }
            return;
        }

        // For Sequence/All content models, check each element's min_occurs
        for (child_name, &(min_occurs, _)) in &flattened.constraints {
            if min_occurs > 0 {
                // Calculate actual count including substitution group members (transitively)
                let mut actual_count = ctx.get_child_count(child_name);

                // Also try with local name if child_name has a prefix
                if let Some((_prefix, local)) = child_name.split_once(':') {
                    actual_count += ctx.get_child_count(local);
                }

                // Add counts from all substitution group members (unless skipped)
                if !self.options.skip_substitution_groups {
                    let all_members = self.get_all_substitution_members(child_name);
                    for member in all_members.iter() {
                        actual_count += ctx.get_child_count(member);
                    }
                }

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
                                ctx.name, child_name, min_occurs, actual_count
                            ),
                        )
                        .with_node_name(ctx.name.as_ref())
                        .with_expected(format!(
                            "at least {} occurrence(s) of '{}'",
                            min_occurs, child_name
                        ))
                        .with_found(format!("{} occurrence(s)", actual_count))
                        .with_level(ErrorLevel::Error);
                    self.add_error(error);
                }
            }
        }
    }
}
