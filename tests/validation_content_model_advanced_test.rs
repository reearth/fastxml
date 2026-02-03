//! Advanced content model validation tests.
//!
//! Tests for content model validation that are not covered in the main validation_test.rs:
//! - Nested compositor groups (sequence within choice, etc.)
//! - xs:any validation
//! - Complex occurrence combinations
//! - Edge cases in sequence/choice/all validation

use fastxml::schema::xsd::content_model::{
    CompositorGroup, CompositorType, ContentElement, ContentModelError, ContentModelItem,
    ContentModelValidator, Occurrence,
};

// =============================================================================
// Nested Compositor Tests
// =============================================================================

mod nested_compositors {
    use super::*;

    #[test]
    fn test_sequence_with_nested_choice() {
        // <sequence>
        //   <element name="header"/>
        //   <choice>
        //     <element name="optionA"/>
        //     <element name="optionB"/>
        //   </choice>
        //   <element name="footer"/>
        // </sequence>
        let nested_choice = ContentModelItem::Group(CompositorGroup {
            compositor_type: CompositorType::Choice,
            elements: vec![
                ContentModelItem::Element(ContentElement::new("optionA", Occurrence::required())),
                ContentModelItem::Element(ContentElement::new("optionB", Occurrence::required())),
            ],
            occurrence: Occurrence::required(),
        });

        let mut validator = ContentModelValidator::sequence(vec![
            ContentModelItem::Element(ContentElement::new("header", Occurrence::required())),
            nested_choice,
            ContentModelItem::Element(ContentElement::new("footer", Occurrence::required())),
        ]);

        // Valid: header, optionA, footer
        assert!(validator.validate_element("header").is_ok());
        assert!(validator.validate_element("optionA").is_ok());
        assert!(validator.validate_element("footer").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_sequence_with_nested_choice_option_b() {
        let nested_choice = ContentModelItem::Group(CompositorGroup {
            compositor_type: CompositorType::Choice,
            elements: vec![
                ContentModelItem::Element(ContentElement::new("optionA", Occurrence::required())),
                ContentModelItem::Element(ContentElement::new("optionB", Occurrence::required())),
            ],
            occurrence: Occurrence::required(),
        });

        let mut validator = ContentModelValidator::sequence(vec![
            ContentModelItem::Element(ContentElement::new("header", Occurrence::required())),
            nested_choice,
            ContentModelItem::Element(ContentElement::new("footer", Occurrence::required())),
        ]);

        // Valid: header, optionB, footer
        assert!(validator.validate_element("header").is_ok());
        assert!(validator.validate_element("optionB").is_ok());
        assert!(validator.validate_element("footer").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_choice_with_nested_sequence() {
        // <choice>
        //   <sequence>
        //     <element name="a"/>
        //     <element name="b"/>
        //   </sequence>
        //   <element name="c"/>
        // </choice>
        let nested_sequence = ContentModelItem::Group(CompositorGroup {
            compositor_type: CompositorType::Sequence,
            elements: vec![
                ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
                ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
            ],
            occurrence: Occurrence::required(),
        });

        let mut validator = ContentModelValidator::choice(vec![
            nested_sequence,
            ContentModelItem::Element(ContentElement::new("c", Occurrence::required())),
        ]);

        // Valid: just c
        assert!(validator.validate_element("c").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_deeply_nested_compositors() {
        // <sequence>
        //   <choice>
        //     <sequence>
        //       <element name="a"/>
        //       <element name="b"/>
        //     </sequence>
        //     <element name="c"/>
        //   </choice>
        // </sequence>
        let inner_sequence = ContentModelItem::Group(CompositorGroup {
            compositor_type: CompositorType::Sequence,
            elements: vec![
                ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
                ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
            ],
            occurrence: Occurrence::required(),
        });

        let middle_choice = ContentModelItem::Group(CompositorGroup {
            compositor_type: CompositorType::Choice,
            elements: vec![
                inner_sequence,
                ContentModelItem::Element(ContentElement::new("c", Occurrence::required())),
            ],
            occurrence: Occurrence::required(),
        });

        let mut validator = ContentModelValidator::sequence(vec![middle_choice]);

        // Valid: a, b (via nested sequence in choice)
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_complete().is_ok());
    }
}

// =============================================================================
// xs:any Validation Tests
// =============================================================================

mod any_element {
    use super::*;

    #[test]
    fn test_any_in_sequence() {
        // Note: xs:any followed by named elements in sequence can be ambiguous
        // This test verifies the current implementation behavior
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("header", Occurrence::required())),
            ContentModelItem::Any {
                namespace: None,
                occurrence: Occurrence::unbounded(0),
            },
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Valid: header, then any elements via xs:any
        assert!(validator.validate_element("header").is_ok());
        assert!(
            validator.validate_element("anything").is_ok(),
            "xs:any should accept any element"
        );
        assert!(
            validator.validate_element("something").is_ok(),
            "xs:any should accept multiple elements"
        );
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_any_at_end_of_sequence() {
        // xs:any at the end of a sequence is the most common and unambiguous use case
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("header", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("body", Occurrence::required())),
            ContentModelItem::Any {
                namespace: None,
                occurrence: Occurrence::unbounded(0),
            },
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("header").is_ok());
        assert!(validator.validate_element("body").is_ok());
        assert!(validator.validate_element("extension1").is_ok());
        assert!(validator.validate_element("extension2").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_any_with_min_occurs() {
        let elements = vec![ContentModelItem::Any {
            namespace: None,
            occurrence: Occurrence::new(2, Some(5)),
        }];

        let mut validator = ContentModelValidator::sequence(elements);

        // Only one element - should fail on complete
        assert!(validator.validate_element("first").is_ok());
        let result = validator.validate_complete();
        assert!(
            matches!(result, Err(ContentModelError::TooFewOccurrences { .. })),
            "xs:any with minOccurs=2 should fail with 1 element, got: {:?}",
            result
        );
    }

    #[test]
    fn test_any_with_max_occurs() {
        let elements = vec![ContentModelItem::Any {
            namespace: None,
            occurrence: Occurrence::new(0, Some(2)),
        }];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("first").is_ok());
        assert!(validator.validate_element("second").is_ok());
        let result = validator.validate_element("third");
        assert!(
            result.is_err(),
            "xs:any with maxOccurs=2 should fail on 3rd element"
        );
    }

    #[test]
    fn test_any_in_choice() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("known", Occurrence::required())),
            ContentModelItem::Any {
                namespace: None,
                occurrence: Occurrence::required(),
            },
        ];

        let mut validator = ContentModelValidator::choice(elements);

        // Any element should match the xs:any branch
        assert!(
            validator.validate_element("unknown").is_ok(),
            "Unknown element should match xs:any in choice"
        );
        assert!(validator.validate_complete().is_ok());
    }
}

// =============================================================================
// Occurrence Edge Cases
// =============================================================================

mod occurrence_edge_cases {
    use super::*;

    #[test]
    fn test_optional_in_sequence() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("required", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("optional", Occurrence::optional())),
            ContentModelItem::Element(ContentElement::new("also_required", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Skip optional element
        assert!(validator.validate_element("required").is_ok());
        assert!(validator.validate_element("also_required").is_ok());
        assert!(
            validator.validate_complete().is_ok(),
            "Should pass without optional element"
        );
    }

    #[test]
    fn test_unbounded_in_sequence() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("single", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("many", Occurrence::unbounded(1))),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("single").is_ok());
        for _ in 0..100 {
            assert!(
                validator.validate_element("many").is_ok(),
                "Unbounded should allow many"
            );
        }
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_zero_to_unbounded() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "optional_many",
            Occurrence::unbounded(0),
        ))];

        let validator = ContentModelValidator::sequence(elements);

        // Zero occurrences should be valid
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_exactly_n_occurrences() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "exactly3",
            Occurrence::new(3, Some(3)),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        // 2 occurrences - should fail on complete
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_element("exactly3").is_ok());
        let result = validator.validate_complete();
        assert!(
            matches!(
                result,
                Err(ContentModelError::TooFewOccurrences {
                    expected: 3,
                    found: 2,
                    ..
                })
            ),
            "Expected TooFewOccurrences for 2/3, got: {:?}",
            result
        );

        // Reset and try with exactly 3
        validator.reset();
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_complete().is_ok());

        // Reset and try with 4
        validator.reset();
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_element("exactly3").is_ok());
        assert!(validator.validate_element("exactly3").is_ok());
        let result = validator.validate_element("exactly3");
        assert!(
            matches!(
                result,
                Err(ContentModelError::TooManyOccurrences { max: 3, .. })
            ),
            "Expected TooManyOccurrences for 4/3, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Sequence Order Strictness Tests
// =============================================================================

mod sequence_order {
    use super::*;

    #[test]
    fn test_strict_sequence_no_backtrack() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::new(1, Some(2)))),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::new(1, Some(2)))),
            ContentModelItem::Element(ContentElement::new("c", Occurrence::new(1, Some(2)))),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_element("c").is_ok());

        // Try to go back to b after c
        let result = validator.validate_element("b");
        assert!(
            matches!(result, Err(ContentModelError::OutOfOrder { .. })),
            "Should not allow backtracking in sequence, got: {:?}",
            result
        );
    }

    #[test]
    fn test_sequence_allows_repeating_current() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::new(1, Some(3)))),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Multiple a's should be allowed
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_sequence_skips_optional_elements() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::optional())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::optional())),
            ContentModelItem::Element(ContentElement::new("c", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Skip directly to c
        assert!(validator.validate_element("c").is_ok());
        assert!(validator.validate_complete().is_ok());
    }
}

// =============================================================================
// All Compositor Tests
// =============================================================================

mod all_compositor {
    use super::*;

    #[test]
    fn test_all_elements_any_order() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("c", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        // Provide in reverse order
        assert!(validator.validate_element("c").is_ok());
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_all_with_optional() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("required1", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("optional1", Occurrence::optional())),
            ContentModelItem::Element(ContentElement::new("required2", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        // Provide only required elements
        assert!(validator.validate_element("required2").is_ok());
        assert!(validator.validate_element("required1").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_all_rejects_duplicates() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        assert!(validator.validate_element("a").is_ok());
        let result = validator.validate_element("a");
        assert!(
            matches!(
                result,
                Err(ContentModelError::TooManyOccurrences { max: 1, .. })
            ),
            "All should reject duplicate elements, got: {:?}",
            result
        );
    }

    #[test]
    fn test_all_rejects_unknown() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        let result = validator.validate_element("c");
        assert!(
            matches!(result, Err(ContentModelError::UnexpectedElement { .. })),
            "All should reject unknown elements, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Choice Compositor Tests
// =============================================================================

mod choice_compositor {
    use super::*;

    #[test]
    fn test_choice_with_occurrence() {
        // Choice can repeat if it has maxOccurs > 1
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::new(1, Some(2)))),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::new(1, Some(2)))),
        ];

        let mut validator = ContentModelValidator::choice(elements);

        // Can choose same element twice
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("a").is_ok());
        let result = validator.validate_element("a");
        assert!(
            matches!(result, Err(ContentModelError::TooManyOccurrences { .. })),
            "Choice element should respect maxOccurs, got: {:?}",
            result
        );
    }

    #[test]
    fn test_empty_choice_fails() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let validator = ContentModelValidator::choice(elements);

        // No elements provided
        let result = validator.validate_complete();
        assert!(
            result.is_err(),
            "Empty choice should fail validation, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Error Message Quality Tests
// =============================================================================

mod error_messages {
    use super::*;

    #[test]
    fn test_unexpected_element_lists_expected() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("expected1", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("expected2", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        let result = validator.validate_element("unexpected");
        if let Err(ContentModelError::UnexpectedElement { element, expected }) = result {
            assert_eq!(element, "unexpected");
            assert!(expected.contains(&"expected1".to_string()));
            assert!(expected.contains(&"expected2".to_string()));
        } else {
            panic!("Expected UnexpectedElement error, got: {:?}", result);
        }
    }

    #[test]
    fn test_out_of_order_shows_context() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("first", Occurrence::new(1, Some(2)))),
            ContentModelItem::Element(ContentElement::new("second", Occurrence::new(1, Some(2)))),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("first").is_ok());
        assert!(validator.validate_element("second").is_ok());

        let result = validator.validate_element("first");
        if let Err(ContentModelError::OutOfOrder { element, after }) = result {
            assert_eq!(element, "first");
            assert_eq!(after, "second");
        } else {
            panic!("Expected OutOfOrder error, got: {:?}", result);
        }
    }

    #[test]
    fn test_too_few_occurrences_shows_counts() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "needed",
            Occurrence::new(3, Some(5)),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("needed").is_ok());
        // Only 1 out of minimum 3

        let result = validator.validate_complete();
        if let Err(ContentModelError::TooFewOccurrences {
            element,
            expected,
            found,
        }) = result
        {
            assert_eq!(element, "needed");
            assert_eq!(expected, 3);
            assert_eq!(found, 1);
        } else {
            panic!("Expected TooFewOccurrences error, got: {:?}", result);
        }
    }
}

// =============================================================================
// Validator Reset Tests
// =============================================================================

mod validator_reset {
    use super::*;

    #[test]
    fn test_reset_allows_revalidation() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // First validation
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_complete().is_ok());

        // Reset
        validator.reset();

        // Second validation should work the same
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_reset_clears_error_state() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "item",
            Occurrence::new(1, Some(2)),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        // Exceed max
        assert!(validator.validate_element("item").is_ok());
        assert!(validator.validate_element("item").is_ok());
        assert!(validator.validate_element("item").is_err());

        // Reset should clear the count
        validator.reset();
        assert!(
            validator.validate_element("item").is_ok(),
            "Reset should clear occurrence counts"
        );
    }
}
