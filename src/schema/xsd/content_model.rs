//! XSD Content Model validation.
//!
//! This module implements validation for XSD content models:
//!
//! ## Compositor Types
//! - `sequence` - elements must appear in order
//! - `choice` - exactly one element must appear
//! - `all` - all elements must appear (in any order)
//!
//! ## Occurrence Constraints
//! - `minOccurs` - minimum occurrences (default: 1)
//! - `maxOccurs` - maximum occurrences (default: 1, or "unbounded")

use std::collections::HashMap;

/// Occurrence constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    /// Minimum occurrences (default 1)
    pub min: u32,
    /// Maximum occurrences (None means unbounded)
    pub max: Option<u32>,
}

impl Default for Occurrence {
    fn default() -> Self {
        Self {
            min: 1,
            max: Some(1),
        }
    }
}

impl Occurrence {
    /// Creates a new occurrence constraint.
    pub fn new(min: u32, max: Option<u32>) -> Self {
        Self { min, max }
    }

    /// Creates an optional occurrence (0..1).
    pub fn optional() -> Self {
        Self {
            min: 0,
            max: Some(1),
        }
    }

    /// Creates a required occurrence (1..1).
    pub fn required() -> Self {
        Self {
            min: 1,
            max: Some(1),
        }
    }

    /// Creates an unbounded occurrence (min..unbounded).
    pub fn unbounded(min: u32) -> Self {
        Self { min, max: None }
    }

    /// Checks if the count satisfies this occurrence constraint.
    pub fn is_satisfied(&self, count: u32) -> bool {
        count >= self.min && self.max.is_none_or(|max| count <= max)
    }

    /// Checks if more elements can be added.
    pub fn can_add_more(&self, count: u32) -> bool {
        self.max.is_none_or(|max| count < max)
    }

    /// Checks if the minimum is satisfied.
    pub fn min_satisfied(&self, count: u32) -> bool {
        count >= self.min
    }
}

/// Content model compositor type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorType {
    /// Elements must appear in order
    Sequence,
    /// Exactly one element must appear
    Choice,
    /// All elements must appear (any order)
    All,
}

/// Error types for content model validation.
#[derive(Debug, Clone)]
pub enum ContentModelError {
    /// Unexpected element encountered
    UnexpectedElement {
        /// The element that was unexpected
        element: String,
        /// List of expected element names
        expected: Vec<String>,
    },
    /// Missing required element
    MissingElement {
        /// The element that is missing
        element: String,
    },
    /// Too few occurrences of an element
    TooFewOccurrences {
        /// The element with insufficient occurrences
        element: String,
        /// Minimum expected occurrences
        expected: u32,
        /// Actual number of occurrences found
        found: u32,
    },
    /// Too many occurrences of an element
    TooManyOccurrences {
        /// The element with excessive occurrences
        element: String,
        /// Maximum allowed occurrences
        max: u32,
        /// Actual number of occurrences found
        found: u32,
    },
    /// Element out of order in sequence
    OutOfOrder {
        /// The element that is out of order
        element: String,
        /// The element after which it appeared
        after: String,
    },
    /// Invalid content model state
    InvalidState {
        /// Error message describing the invalid state
        message: String,
    },
}

impl std::fmt::Display for ContentModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentModelError::UnexpectedElement { element, expected } => {
                write!(
                    f,
                    "unexpected element '{}', expected one of: {:?}",
                    element, expected
                )
            }
            ContentModelError::MissingElement { element } => {
                write!(f, "missing required element '{}'", element)
            }
            ContentModelError::TooFewOccurrences {
                element,
                expected,
                found,
            } => {
                write!(
                    f,
                    "element '{}' appears {} times, minimum is {}",
                    element, found, expected
                )
            }
            ContentModelError::TooManyOccurrences {
                element,
                max,
                found,
            } => {
                write!(
                    f,
                    "element '{}' appears {} times, maximum is {}",
                    element, found, max
                )
            }
            ContentModelError::OutOfOrder { element, after } => {
                write!(
                    f,
                    "element '{}' appears after '{}' in sequence",
                    element, after
                )
            }
            ContentModelError::InvalidState { message } => {
                write!(f, "invalid content model state: {}", message)
            }
        }
    }
}

impl std::error::Error for ContentModelError {}

/// Element definition in a content model.
#[derive(Debug, Clone)]
pub struct ContentElement {
    /// Element name
    pub name: String,
    /// Namespace (if any)
    pub namespace: Option<String>,
    /// Occurrence constraint
    pub occurrence: Occurrence,
}

impl ContentElement {
    /// Creates a new content element.
    pub fn new(name: impl Into<String>, occurrence: Occurrence) -> Self {
        Self {
            name: name.into(),
            namespace: None,
            occurrence,
        }
    }

    /// Creates a content element with namespace.
    pub fn with_namespace(
        name: impl Into<String>,
        namespace: impl Into<String>,
        occurrence: Occurrence,
    ) -> Self {
        Self {
            name: name.into(),
            namespace: Some(namespace.into()),
            occurrence,
        }
    }
}

/// A compositor group (sequence, choice, or all).
#[derive(Debug, Clone)]
pub struct CompositorGroup {
    /// Type of compositor
    pub compositor_type: CompositorType,
    /// Elements in this group
    pub elements: Vec<ContentModelItem>,
    /// Occurrence constraint for the group itself
    pub occurrence: Occurrence,
}

/// Item in a content model (element or nested compositor).
#[derive(Debug, Clone)]
pub enum ContentModelItem {
    /// A single element
    Element(ContentElement),
    /// A nested compositor group
    Group(CompositorGroup),
    /// Any element (xs:any)
    Any {
        /// Namespace constraint for xs:any
        namespace: Option<String>,
        /// Occurrence constraint
        occurrence: Occurrence,
    },
}

/// Validation state for occurrence tracking.
#[derive(Debug, Clone, Default)]
struct OccurrenceState {
    /// Count of occurrences per element name
    counts: HashMap<String, u32>,
    /// Current position in sequence
    sequence_position: usize,
    /// Count of xs:any matches per index (for multiple xs:any in same content model)
    any_counts: HashMap<usize, u32>,
}

impl OccurrenceState {
    fn new() -> Self {
        Self::default()
    }

    fn increment(&mut self, name: &str) -> u32 {
        let count = self.counts.entry(name.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    fn get_count(&self, name: &str) -> u32 {
        *self.counts.get(name).unwrap_or(&0)
    }

    fn increment_any(&mut self, index: usize) -> u32 {
        let count = self.any_counts.entry(index).or_insert(0);
        *count += 1;
        *count
    }

    fn get_any_count(&self, index: usize) -> u32 {
        *self.any_counts.get(&index).unwrap_or(&0)
    }
}

/// Content model validator.
///
/// This validates XML element sequences against XSD content models.
pub struct ContentModelValidator {
    /// Root compositor group
    root: CompositorGroup,
    /// Current validation state
    state: OccurrenceState,
    /// Stack of compositor states (for nested groups)
    compositor_stack: Vec<CompositorState>,
}

/// State for tracking nested compositor validation.
///
/// Used for future support of complex nested content model validation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CompositorState {
    /// Type of compositor (sequence, choice, all)
    compositor_type: CompositorType,
    /// Current position in the compositor
    position: usize,
    /// Number of times this compositor has been traversed
    count: u32,
    /// Whether a choice has been made (for choice compositors)
    choice_made: bool,
}

impl ContentModelValidator {
    /// Creates a new content model validator.
    pub fn new(root: CompositorGroup) -> Self {
        Self {
            root,
            state: OccurrenceState::new(),
            compositor_stack: Vec::new(),
        }
    }

    /// Creates a validator for a sequence.
    pub fn sequence(elements: Vec<ContentModelItem>) -> Self {
        Self::new(CompositorGroup {
            compositor_type: CompositorType::Sequence,
            elements,
            occurrence: Occurrence::default(),
        })
    }

    /// Creates a validator for a choice.
    pub fn choice(elements: Vec<ContentModelItem>) -> Self {
        Self::new(CompositorGroup {
            compositor_type: CompositorType::Choice,
            elements,
            occurrence: Occurrence::default(),
        })
    }

    /// Creates a validator for an all group.
    pub fn all(elements: Vec<ContentModelItem>) -> Self {
        Self::new(CompositorGroup {
            compositor_type: CompositorType::All,
            elements,
            occurrence: Occurrence::default(),
        })
    }

    /// Validates that an element can appear at the current position.
    pub fn validate_element(&mut self, name: &str) -> Result<(), ContentModelError> {
        self.validate_element_in_group(name, &self.root.clone())
    }

    fn validate_element_in_group(
        &mut self,
        name: &str,
        group: &CompositorGroup,
    ) -> Result<(), ContentModelError> {
        match group.compositor_type {
            CompositorType::Sequence => self.validate_in_sequence(name, &group.elements),
            CompositorType::Choice => self.validate_in_choice(name, &group.elements),
            CompositorType::All => self.validate_in_all(name, &group.elements),
        }
    }

    fn validate_in_sequence(
        &mut self,
        name: &str,
        elements: &[ContentModelItem],
    ) -> Result<(), ContentModelError> {
        // Find the element and check it appears in valid sequence position
        for (idx, item) in elements.iter().enumerate() {
            match item {
                ContentModelItem::Element(elem) => {
                    if elem.name == name {
                        // Check if we can still add this element
                        let count = self.state.get_count(name);
                        if !elem.occurrence.can_add_more(count) {
                            return Err(ContentModelError::TooManyOccurrences {
                                element: name.to_string(),
                                max: elem.occurrence.max.unwrap_or(u32::MAX),
                                found: count + 1,
                            });
                        }

                        // Check strict sequence order
                        if idx < self.state.sequence_position {
                            // Element appears before current position - not allowed
                            // Find the name of the element at the current position
                            let after = elements
                                .get(self.state.sequence_position)
                                .and_then(|item| match item {
                                    ContentModelItem::Element(e) => Some(e.name.clone()),
                                    _ => None,
                                })
                                .unwrap_or_else(|| "previous element".to_string());
                            return Err(ContentModelError::OutOfOrder {
                                element: name.to_string(),
                                after,
                            });
                        }

                        self.state.increment(name);
                        self.state.sequence_position = idx;
                        return Ok(());
                    }
                }
                ContentModelItem::Group(nested) => {
                    if self.validate_element_in_group(name, nested).is_ok() {
                        return Ok(());
                    }
                }
                ContentModelItem::Any { occurrence, .. } => {
                    // Any element matches - check occurrence constraint
                    let count = self.state.get_any_count(idx);
                    if !occurrence.can_add_more(count) {
                        continue; // This xs:any is full, try next item
                    }
                    self.state.increment_any(idx);
                    self.state.sequence_position = idx;
                    return Ok(());
                }
            }
        }

        // Element not found
        let expected: Vec<String> = elements
            .iter()
            .filter_map(|item| match item {
                ContentModelItem::Element(e) => Some(e.name.clone()),
                ContentModelItem::Any { .. } => Some("(any)".to_string()),
                _ => None,
            })
            .collect();

        Err(ContentModelError::UnexpectedElement {
            element: name.to_string(),
            expected,
        })
    }

    fn validate_in_choice(
        &mut self,
        name: &str,
        elements: &[ContentModelItem],
    ) -> Result<(), ContentModelError> {
        // In a choice, any one element is valid
        for (idx, item) in elements.iter().enumerate() {
            match item {
                ContentModelItem::Element(elem) => {
                    if elem.name == name {
                        let count = self.state.get_count(name);
                        if !elem.occurrence.can_add_more(count) {
                            return Err(ContentModelError::TooManyOccurrences {
                                element: name.to_string(),
                                max: elem.occurrence.max.unwrap_or(u32::MAX),
                                found: count + 1,
                            });
                        }
                        self.state.increment(name);
                        return Ok(());
                    }
                }
                ContentModelItem::Group(nested) => {
                    if self.validate_element_in_group(name, nested).is_ok() {
                        return Ok(());
                    }
                }
                ContentModelItem::Any { occurrence, .. } => {
                    let count = self.state.get_any_count(idx);
                    if !occurrence.can_add_more(count) {
                        continue;
                    }
                    self.state.increment_any(idx);
                    return Ok(());
                }
            }
        }

        let expected: Vec<String> = elements
            .iter()
            .filter_map(|item| match item {
                ContentModelItem::Element(e) => Some(e.name.clone()),
                _ => None,
            })
            .collect();

        Err(ContentModelError::UnexpectedElement {
            element: name.to_string(),
            expected,
        })
    }

    fn validate_in_all(
        &mut self,
        name: &str,
        elements: &[ContentModelItem],
    ) -> Result<(), ContentModelError> {
        // In an all group, elements can appear in any order
        // but each must appear exactly according to its occurrence
        for (idx, item) in elements.iter().enumerate() {
            match item {
                ContentModelItem::Element(elem) => {
                    if elem.name == name {
                        let count = self.state.get_count(name);
                        if !elem.occurrence.can_add_more(count) {
                            return Err(ContentModelError::TooManyOccurrences {
                                element: name.to_string(),
                                max: elem.occurrence.max.unwrap_or(u32::MAX),
                                found: count + 1,
                            });
                        }
                        self.state.increment(name);
                        return Ok(());
                    }
                }
                ContentModelItem::Group(nested) => {
                    if self.validate_element_in_group(name, nested).is_ok() {
                        return Ok(());
                    }
                }
                ContentModelItem::Any { occurrence, .. } => {
                    let count = self.state.get_any_count(idx);
                    if !occurrence.can_add_more(count) {
                        continue;
                    }
                    self.state.increment_any(idx);
                    return Ok(());
                }
            }
        }

        let expected: Vec<String> = elements
            .iter()
            .filter_map(|item| match item {
                ContentModelItem::Element(e) => Some(e.name.clone()),
                ContentModelItem::Any { .. } => Some("(any)".to_string()),
                _ => None,
            })
            .collect();

        Err(ContentModelError::UnexpectedElement {
            element: name.to_string(),
            expected,
        })
    }

    /// Validates that all required elements have appeared.
    pub fn validate_complete(&self) -> Result<(), ContentModelError> {
        self.validate_complete_group(&self.root)
    }

    fn validate_complete_group(&self, group: &CompositorGroup) -> Result<(), ContentModelError> {
        match group.compositor_type {
            CompositorType::Choice => {
                // For choice, at least one item must have been provided
                let any_provided =
                    group
                        .elements
                        .iter()
                        .enumerate()
                        .any(|(idx, item)| match item {
                            ContentModelItem::Element(elem) => {
                                self.state.get_count(&elem.name) > 0
                            }
                            ContentModelItem::Group(nested) => {
                                self.validate_complete_group(nested).is_ok()
                            }
                            ContentModelItem::Any { .. } => self.state.get_any_count(idx) > 0,
                        });
                if !any_provided && group.occurrence.min > 0 {
                    return Err(ContentModelError::InvalidState {
                        message: "choice requires at least one element".into(),
                    });
                }
                Ok(())
            }
            CompositorType::Sequence | CompositorType::All => {
                // For sequence and all, all required items must be provided
                for (idx, item) in group.elements.iter().enumerate() {
                    match item {
                        ContentModelItem::Element(elem) => {
                            let count = self.state.get_count(&elem.name);
                            if !elem.occurrence.min_satisfied(count) {
                                return Err(ContentModelError::TooFewOccurrences {
                                    element: elem.name.clone(),
                                    expected: elem.occurrence.min,
                                    found: count,
                                });
                            }
                        }
                        ContentModelItem::Group(nested) => {
                            self.validate_complete_group(nested)?;
                        }
                        ContentModelItem::Any { occurrence, .. } => {
                            // xs:any validation - check occurrence constraint
                            let count = self.state.get_any_count(idx);
                            if !occurrence.min_satisfied(count) {
                                return Err(ContentModelError::TooFewOccurrences {
                                    element: "(any)".to_string(),
                                    expected: occurrence.min,
                                    found: count,
                                });
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// Resets the validator state.
    pub fn reset(&mut self) {
        self.state = OccurrenceState::new();
        self.compositor_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_validation() {
        let mut validator = ContentModelValidator::sequence(vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("c", Occurrence::optional())),
        ]);

        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_element("c").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_sequence_missing_required() {
        let mut validator = ContentModelValidator::sequence(vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ]);

        assert!(validator.validate_element("a").is_ok());
        // Missing "b"
        assert!(validator.validate_complete().is_err());
    }

    #[test]
    fn test_choice_validation() {
        let mut validator = ContentModelValidator::choice(vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ]);

        // Either a or b is valid
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_all_validation() {
        let mut validator = ContentModelValidator::all(vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ]);

        // Order doesn't matter
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_occurrence_bounds() {
        let mut validator = ContentModelValidator::sequence(vec![ContentModelItem::Element(
            ContentElement::new("item", Occurrence::new(1, Some(3))),
        )]);

        assert!(validator.validate_element("item").is_ok()); // 1
        assert!(validator.validate_element("item").is_ok()); // 2
        assert!(validator.validate_element("item").is_ok()); // 3
        assert!(validator.validate_element("item").is_err()); // 4 - too many
    }

    #[test]
    fn test_unbounded_occurrence() {
        let mut validator = ContentModelValidator::sequence(vec![ContentModelItem::Element(
            ContentElement::new("item", Occurrence::unbounded(0)),
        )]);

        for _ in 0..100 {
            assert!(validator.validate_element("item").is_ok());
        }
    }

    #[test]
    fn test_unexpected_element() {
        let mut validator = ContentModelValidator::sequence(vec![ContentModelItem::Element(
            ContentElement::new("expected", Occurrence::required()),
        )]);

        let result = validator.validate_element("unexpected");
        assert!(matches!(
            result,
            Err(ContentModelError::UnexpectedElement { .. })
        ));
    }
}
