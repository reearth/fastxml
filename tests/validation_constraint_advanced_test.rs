//! Advanced identity constraint validation tests.
//!
//! Tests for identity constraints that are not covered in the main validation_test.rs:
//! - SelectorError
//! - FieldError
//! - Null value handling in different constraint types
//! - Multiple keyrefs referencing the same key
//! - Complex composite key scenarios

use fastxml::schema::xsd::constraints::{
    ConstraintError, ConstraintValidator, IdentityConstraint, KeyValue,
};

// =============================================================================
// Null Value Handling Tests
// =============================================================================

mod null_value_handling {
    use super::*;

    #[test]
    fn test_key_rejects_null_in_any_field() {
        let constraint =
            IdentityConstraint::key("composite-key", ".//item").with_fields(["@type", "@id"]);

        let mut validator = ConstraintValidator::new();
        validator.register_key("composite-key", 2);

        // First field is null
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["".into(), "123".into()]));
        assert!(
            matches!(
                &result,
                Err(ConstraintError::NullKeyValue {
                    constraint,
                    field_index: 0
                }) if constraint == "composite-key"
            ),
            "Key should reject null in first field, got: {:?}",
            result
        );

        // Second field is null
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["type1".into(), "".into()]));
        assert!(
            matches!(
                &result,
                Err(ConstraintError::NullKeyValue {
                    constraint,
                    field_index: 1
                }) if constraint == "composite-key"
            ),
            "Key should reject null in second field, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unique_allows_null_values() {
        let constraint = IdentityConstraint::unique("unique-id", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Multiple null values should be allowed for unique
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single(""))
                .is_ok(),
            "Unique should allow first null"
        );
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single(""))
                .is_ok(),
            "Unique should allow second null"
        );
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single(""))
                .is_ok(),
            "Unique should allow third null"
        );

        // Non-null values should still be unique
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single("value1"))
                .is_ok()
        );
        let result = validator.add_key_value(&constraint, KeyValue::single("value1"));
        assert!(
            matches!(&result, Err(ConstraintError::DuplicateKey { .. })),
            "Unique should still reject duplicate non-null values, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unique_composite_with_partial_null() {
        let constraint =
            IdentityConstraint::unique("composite-unique", ".//item").with_fields(["@a", "@b"]);

        let mut validator = ConstraintValidator::new();

        // Composite with one null field - should be allowed (doesn't participate in uniqueness)
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::new(vec!["value".into(), "".into()]))
                .is_ok()
        );
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::new(vec!["value".into(), "".into()]))
                .is_ok()
        );
    }

    #[test]
    fn test_keyref_with_null_skipped() {
        let key = IdentityConstraint::key("person-id", ".//person").with_field("@id");
        let keyref =
            IdentityConstraint::keyref("order-person", ".//order", "person-id").with_field("@ref");

        let mut validator = ConstraintValidator::new();
        validator.register_key("person-id", 1);

        // Add a key
        validator
            .add_key_value(&key, KeyValue::single("p1"))
            .unwrap();

        // Null keyref should be skipped (not validated)
        validator.add_keyref_value(&keyref, KeyValue::single(""));

        // Validation should pass because null keyref is skipped
        let result = validator.validate_keyrefs();
        assert!(
            result.is_ok(),
            "Null keyref values should be skipped, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Complex Composite Key Tests
// =============================================================================

mod composite_keys {
    use super::*;

    #[test]
    fn test_three_field_composite_key() {
        let constraint = IdentityConstraint::key("triple-key", ".//record")
            .with_fields(["@year", "@month", "@day"]);

        let mut validator = ConstraintValidator::new();
        validator.register_key("triple-key", 3);

        // Add several unique composite keys
        assert!(
            validator
                .add_key_value(
                    &constraint,
                    KeyValue::new(vec!["2024".into(), "01".into(), "01".into()])
                )
                .is_ok()
        );
        assert!(
            validator
                .add_key_value(
                    &constraint,
                    KeyValue::new(vec!["2024".into(), "01".into(), "02".into()])
                )
                .is_ok()
        );
        assert!(
            validator
                .add_key_value(
                    &constraint,
                    KeyValue::new(vec!["2024".into(), "02".into(), "01".into()])
                )
                .is_ok()
        );

        // Duplicate should fail
        let result = validator.add_key_value(
            &constraint,
            KeyValue::new(vec!["2024".into(), "01".into(), "01".into()]),
        );
        assert!(
            matches!(&result, Err(ConstraintError::DuplicateKey { .. })),
            "Duplicate composite key should fail, got: {:?}",
            result
        );
    }

    #[test]
    fn test_composite_keyref() {
        let key =
            IdentityConstraint::key("product-key", ".//product").with_fields(["@category", "@sku"]);
        let keyref = IdentityConstraint::keyref("order-product", ".//order-item", "product-key")
            .with_fields(["@cat", "@product-sku"]);

        let mut validator = ConstraintValidator::new();
        validator.register_key("product-key", 2);

        // Add keys
        validator
            .add_key_value(
                &key,
                KeyValue::new(vec!["electronics".into(), "TV-001".into()]),
            )
            .unwrap();
        validator
            .add_key_value(
                &key,
                KeyValue::new(vec!["electronics".into(), "TV-002".into()]),
            )
            .unwrap();

        // Valid keyref
        validator.add_keyref_value(
            &keyref,
            KeyValue::new(vec!["electronics".into(), "TV-001".into()]),
        );

        // Invalid keyref (wrong combination)
        validator.add_keyref_value(
            &keyref,
            KeyValue::new(vec!["electronics".into(), "TV-999".into()]),
        );

        let result = validator.validate_keyrefs();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(
                &errors[0],
                ConstraintError::KeyRefNotFound {
                    constraint,
                    refer,
                    ..
                } if constraint == "order-product" && refer == "product-key"
            ),
            "Expected KeyRefNotFound for composite key, got: {:?}",
            errors[0]
        );
    }
}

// =============================================================================
// Multiple KeyRefs Tests
// =============================================================================

mod multiple_keyrefs {
    use super::*;

    #[test]
    fn test_multiple_keyrefs_same_key() {
        let key = IdentityConstraint::key("user-id", ".//user").with_field("@id");
        let keyref1 =
            IdentityConstraint::keyref("post-author", ".//post", "user-id").with_field("@author");
        let keyref2 =
            IdentityConstraint::keyref("comment-author", ".//comment", "user-id").with_field("@by");

        let mut validator = ConstraintValidator::new();
        validator.register_key("user-id", 1);

        // Add keys
        validator
            .add_key_value(&key, KeyValue::single("user1"))
            .unwrap();
        validator
            .add_key_value(&key, KeyValue::single("user2"))
            .unwrap();

        // Valid keyrefs from different constraints
        validator.add_keyref_value(&keyref1, KeyValue::single("user1"));
        validator.add_keyref_value(&keyref2, KeyValue::single("user2"));

        // Invalid keyref from first constraint
        validator.add_keyref_value(&keyref1, KeyValue::single("user999"));

        let result = validator.validate_keyrefs();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(
                &errors[0],
                ConstraintError::KeyRefNotFound { constraint, .. } if constraint == "post-author"
            ),
            "Error should be from post-author keyref, got: {:?}",
            errors[0]
        );
    }

    #[test]
    fn test_keyref_to_nonexistent_key() {
        let keyref =
            IdentityConstraint::keyref("orphan-ref", ".//orphan", "missing-key").with_field("@ref");

        let mut validator = ConstraintValidator::new();
        // Don't register the key

        validator.add_keyref_value(&keyref, KeyValue::single("value"));

        let result = validator.validate_keyrefs();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(
                &errors[0],
                ConstraintError::KeyRefNotFound { refer, .. } if refer == "missing-key"
            ),
            "Should report missing key, got: {:?}",
            errors[0]
        );
    }
}

// =============================================================================
// Key Value Set Tests
// =============================================================================

mod key_value_set {
    use super::*;
    use fastxml::schema::xsd::constraints::KeyValueSet;

    #[test]
    fn test_key_value_set_basic_operations() {
        let mut set = KeyValueSet::new("test-set", 1);

        assert!(set.is_empty());
        assert_eq!(set.len(), 0);

        // Add values
        assert!(set.add(KeyValue::single("a")).is_ok());
        assert!(set.add(KeyValue::single("b")).is_ok());

        assert!(!set.is_empty());
        assert_eq!(set.len(), 2);

        // Check contains
        assert!(set.contains(&KeyValue::single("a")));
        assert!(set.contains(&KeyValue::single("b")));
        assert!(!set.contains(&KeyValue::single("c")));
    }

    #[test]
    fn test_key_value_set_duplicate_detection() {
        let mut set = KeyValueSet::new("dup-test", 1);

        assert!(set.add(KeyValue::single("value")).is_ok());
        let result = set.add(KeyValue::single("value"));
        assert!(
            matches!(
                &result,
                Err(ConstraintError::DuplicateKey {
                    constraint,
                    value
                }) if constraint == "dup-test" && value.values == vec!["value".to_string()]
            ),
            "Should detect duplicate, got: {:?}",
            result
        );
    }

    #[test]
    fn test_key_value_set_iteration() {
        let mut set = KeyValueSet::new("iter-test", 1);

        set.add(KeyValue::single("x")).unwrap();
        set.add(KeyValue::single("y")).unwrap();
        set.add(KeyValue::single("z")).unwrap();

        let values: Vec<_> = set.values().collect();
        assert_eq!(values.len(), 3);

        // Check all values are present (order may vary due to HashSet)
        let value_strings: Vec<&str> = values.iter().map(|kv| kv.values[0].as_str()).collect();
        assert!(value_strings.contains(&"x"));
        assert!(value_strings.contains(&"y"));
        assert!(value_strings.contains(&"z"));
    }
}

// =============================================================================
// KeyValue Tests
// =============================================================================

mod key_value {
    use super::*;

    #[test]
    fn test_key_value_has_null() {
        let complete = KeyValue::new(vec!["a".into(), "b".into()]);
        assert!(!complete.has_null());

        let with_null = KeyValue::new(vec!["a".into(), "".into()]);
        assert!(with_null.has_null());

        let all_null = KeyValue::new(vec!["".into(), "".into()]);
        assert!(all_null.has_null());

        let empty = KeyValue::new(vec![]);
        assert!(!empty.has_null()); // No values means no nulls
    }

    #[test]
    fn test_key_value_is_complete() {
        let value = KeyValue::new(vec!["a".into(), "b".into()]);

        assert!(value.is_complete(2));
        assert!(!value.is_complete(3)); // Wrong field count

        let with_null = KeyValue::new(vec!["a".into(), "".into()]);
        assert!(!with_null.is_complete(2)); // Has null

        let too_few = KeyValue::new(vec!["a".into()]);
        assert!(!too_few.is_complete(2)); // Not enough fields
    }

    #[test]
    fn test_key_value_equality() {
        let a = KeyValue::new(vec!["x".into(), "y".into()]);
        let b = KeyValue::new(vec!["x".into(), "y".into()]);
        let c = KeyValue::new(vec!["x".into(), "z".into()]);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_key_value_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();

        let a = KeyValue::new(vec!["x".into(), "y".into()]);
        let b = KeyValue::new(vec!["x".into(), "y".into()]);
        let c = KeyValue::new(vec!["x".into(), "z".into()]);

        set.insert(a.clone());
        assert!(set.contains(&b)); // Equal values should hash the same
        assert!(!set.contains(&c));

        set.insert(b);
        assert_eq!(set.len(), 1); // Should not add duplicate

        set.insert(c);
        assert_eq!(set.len(), 2);
    }
}

// =============================================================================
// Validator Reset and Reuse Tests
// =============================================================================

mod validator_lifecycle {
    use super::*;

    #[test]
    fn test_validator_reset() {
        let constraint = IdentityConstraint::unique("test-unique", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Add some values
        validator
            .add_key_value(&constraint, KeyValue::single("a"))
            .unwrap();
        validator
            .add_key_value(&constraint, KeyValue::single("b"))
            .unwrap();

        // Verify values exist
        let set = validator.get_key_values("test-unique");
        assert!(set.is_some());
        assert_eq!(set.unwrap().len(), 2);

        // Reset
        validator.reset();

        // Values should be cleared
        let set = validator.get_key_values("test-unique");
        assert!(set.is_none());

        // Should be able to add values again
        validator
            .add_key_value(&constraint, KeyValue::single("a"))
            .unwrap();
        let _ = validator.add_key_value(&constraint, KeyValue::single("a")); // Would fail if state wasn't reset
    }

    #[test]
    fn test_validator_multiple_constraints() {
        let unique1 = IdentityConstraint::unique("unique1", ".//a").with_field("@id");
        let unique2 = IdentityConstraint::unique("unique2", ".//b").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Same value in different constraints should be allowed
        validator
            .add_key_value(&unique1, KeyValue::single("shared"))
            .unwrap();
        validator
            .add_key_value(&unique2, KeyValue::single("shared"))
            .unwrap();

        // But duplicate in same constraint should fail
        let result = validator.add_key_value(&unique1, KeyValue::single("shared"));
        assert!(
            matches!(&result, Err(ConstraintError::DuplicateKey { .. })),
            "Duplicate in same constraint should fail, got: {:?}",
            result
        );
    }

    #[test]
    fn test_get_key_values() {
        let constraint = IdentityConstraint::key("test-key", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();
        validator.register_key("test-key", 1);

        // Before adding values
        let set = validator.get_key_values("test-key");
        assert!(set.is_some());
        assert!(set.unwrap().is_empty());

        // After adding values
        validator
            .add_key_value(&constraint, KeyValue::single("v1"))
            .unwrap();
        validator
            .add_key_value(&constraint, KeyValue::single("v2"))
            .unwrap();

        let set = validator.get_key_values("test-key");
        assert!(set.is_some());
        assert_eq!(set.unwrap().len(), 2);

        // Non-existent constraint
        let set = validator.get_key_values("nonexistent");
        assert!(set.is_none());
    }
}

// =============================================================================
// Error Display Tests
// =============================================================================

mod error_display {
    use super::*;

    #[test]
    fn test_duplicate_key_error_display() {
        let err = ConstraintError::DuplicateKey {
            constraint: "user-id".to_string(),
            value: KeyValue::single("duplicate"),
        };

        let msg = err.to_string();
        assert!(msg.contains("duplicate"));
        assert!(msg.contains("user-id"));
    }

    #[test]
    fn test_null_key_error_display() {
        let err = ConstraintError::NullKeyValue {
            constraint: "required-key".to_string(),
            field_index: 2,
        };

        let msg = err.to_string();
        assert!(msg.contains("null"));
        assert!(msg.contains("required-key"));
        assert!(msg.contains("2"));
    }

    #[test]
    fn test_keyref_not_found_error_display() {
        let err = ConstraintError::KeyRefNotFound {
            constraint: "order-ref".to_string(),
            refer: "product-key".to_string(),
            value: KeyValue::new(vec!["missing".into()]),
        };

        let msg = err.to_string();
        assert!(msg.contains("order-ref"));
        assert!(msg.contains("product-key"));
        assert!(msg.contains("missing"));
    }

    #[test]
    fn test_selector_error_display() {
        let err = ConstraintError::SelectorError {
            constraint: "bad-selector".to_string(),
            message: "Invalid XPath expression".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("bad-selector"));
        assert!(msg.contains("Invalid XPath"));
    }

    #[test]
    fn test_field_error_display() {
        let err = ConstraintError::FieldError {
            constraint: "bad-field".to_string(),
            field_index: 0,
            message: "Field not found".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("bad-field"));
        assert!(msg.contains("0"));
        assert!(msg.contains("Field not found"));
    }
}

// =============================================================================
// Identity Constraint Builder Tests
// =============================================================================

mod constraint_builder {
    use super::*;

    #[test]
    fn test_unique_builder() {
        let constraint = IdentityConstraint::unique("my-unique", ".//item")
            .with_field("@id")
            .with_field("@name");

        assert_eq!(constraint.name, "my-unique");
        assert_eq!(constraint.selector, ".//item");
        assert_eq!(constraint.fields, vec!["@id", "@name"]);
        assert!(constraint.refer.is_none());
    }

    #[test]
    fn test_key_builder() {
        let constraint =
            IdentityConstraint::key("my-key", ".//record").with_fields(["@year", "@month", "@day"]);

        assert_eq!(constraint.name, "my-key");
        assert_eq!(constraint.selector, ".//record");
        assert_eq!(constraint.fields, vec!["@year", "@month", "@day"]);
        assert!(constraint.refer.is_none());
    }

    #[test]
    fn test_keyref_builder() {
        let constraint = IdentityConstraint::keyref("my-keyref", ".//reference", "target-key")
            .with_field("@ref");

        assert_eq!(constraint.name, "my-keyref");
        assert_eq!(constraint.selector, ".//reference");
        assert_eq!(constraint.fields, vec!["@ref"]);
        assert_eq!(constraint.refer, Some("target-key".to_string()));
    }
}
