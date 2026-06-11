//! Advanced facet validation tests.
//!
//! Tests for facet constraints that are not covered in the main validation_test.rs:
//! - minExclusive/maxExclusive
//! - pattern validation
//! - totalDigits
//! - fractionDigits
//! - whitespace handling
//! - invalid pattern (regex compilation error)

use fastxml::schema::xsd::facets::{
    FacetConstraints, FacetError, FacetValidator, WhitespaceHandling,
};

// =============================================================================
// minExclusive/maxExclusive Tests
// =============================================================================

mod exclusive_bounds {
    use super::*;

    #[test]
    fn test_min_exclusive_violation() {
        let mut constraints = FacetConstraints::new();
        constraints.min_exclusive = Some("10".to_string());
        let validator = FacetValidator::new(&constraints);

        // Value equal to bound should fail (exclusive)
        let result = validator.validate("10");
        assert!(
            matches!(result, Err(FacetError::BelowMinExclusive { .. })),
            "Value equal to minExclusive should fail, got: {:?}",
            result
        );

        // Value below bound should fail
        let result = validator.validate("5");
        assert!(
            matches!(result, Err(FacetError::BelowMinExclusive { .. })),
            "Value below minExclusive should fail, got: {:?}",
            result
        );

        // Value above bound should pass
        let result = validator.validate("11");
        assert!(result.is_ok(), "Value above minExclusive should pass");
    }

    #[test]
    fn test_max_exclusive_violation() {
        let mut constraints = FacetConstraints::new();
        constraints.max_exclusive = Some("100".to_string());
        let validator = FacetValidator::new(&constraints);

        // Value equal to bound should fail (exclusive)
        let result = validator.validate("100");
        assert!(
            matches!(result, Err(FacetError::AboveMaxExclusive { .. })),
            "Value equal to maxExclusive should fail, got: {:?}",
            result
        );

        // Value above bound should fail
        let result = validator.validate("150");
        assert!(
            matches!(result, Err(FacetError::AboveMaxExclusive { .. })),
            "Value above maxExclusive should fail, got: {:?}",
            result
        );

        // Value below bound should pass
        let result = validator.validate("99");
        assert!(result.is_ok(), "Value below maxExclusive should pass");
    }

    #[test]
    fn test_exclusive_bounds_combined() {
        let mut constraints = FacetConstraints::new();
        constraints.min_exclusive = Some("0".to_string());
        constraints.max_exclusive = Some("10".to_string());
        let validator = FacetValidator::new(&constraints);

        // Test boundary values
        assert!(
            validator.validate("0").is_err(),
            "Value at minExclusive should fail"
        );
        assert!(
            validator.validate("10").is_err(),
            "Value at maxExclusive should fail"
        );

        // Test valid range
        assert!(validator.validate("1").is_ok(), "Value 1 should pass");
        assert!(validator.validate("5").is_ok(), "Value 5 should pass");
        assert!(validator.validate("9").is_ok(), "Value 9 should pass");
    }

    #[test]
    fn test_exclusive_with_decimal_values() {
        let mut constraints = FacetConstraints::new();
        constraints.min_exclusive = Some("0.0".to_string());
        constraints.max_exclusive = Some("1.0".to_string());
        let validator = FacetValidator::new(&constraints);

        assert!(
            validator.validate("0.0").is_err(),
            "Value at minExclusive should fail"
        );
        assert!(
            validator.validate("1.0").is_err(),
            "Value at maxExclusive should fail"
        );
        assert!(validator.validate("0.5").is_ok(), "Value 0.5 should pass");
        assert!(
            validator.validate("0.001").is_ok(),
            "Value just above 0 should pass"
        );
        assert!(
            validator.validate("0.999").is_ok(),
            "Value just below 1 should pass"
        );
    }
}

// =============================================================================
// Pattern Validation Tests
// =============================================================================

mod pattern_validation {
    use super::*;

    #[test]
    fn test_pattern_violation() {
        let mut constraints = FacetConstraints::new().with_pattern(r"[A-Z]{3}-[0-9]{4}");
        constraints.compile_patterns().unwrap();
        let validator = FacetValidator::new(&constraints);

        // Valid pattern
        let result = validator.validate("ABC-1234");
        assert!(result.is_ok(), "Valid pattern should pass");

        // Invalid patterns
        let result = validator.validate("abc-1234");
        assert!(
            matches!(&result, Err(FacetError::PatternMismatch { value, .. }) if value == "abc-1234"),
            "Lowercase should fail, got: {:?}",
            result
        );

        let result = validator.validate("ABC1234");
        assert!(
            matches!(result, Err(FacetError::PatternMismatch { .. })),
            "Missing hyphen should fail"
        );

        let result = validator.validate("AB-1234");
        assert!(
            matches!(result, Err(FacetError::PatternMismatch { .. })),
            "Too few letters should fail"
        );
    }

    #[test]
    fn test_multiple_patterns_all_must_match() {
        let mut constraints = FacetConstraints::new()
            .with_pattern(r"[a-zA-Z]+") // Must contain only letters
            .with_pattern(r".{5,10}"); // Must be 5-10 characters
        constraints.compile_patterns().unwrap();
        let validator = FacetValidator::new(&constraints);

        // Matches both patterns
        assert!(validator.validate("Hello").is_ok());
        assert!(validator.validate("HelloWorld").is_ok());

        // Too short (fails second pattern)
        let result = validator.validate("Hi");
        assert!(
            matches!(result, Err(FacetError::PatternMismatch { .. })),
            "Too short should fail"
        );

        // Contains numbers (fails first pattern)
        let result = validator.validate("Hello123");
        assert!(
            matches!(result, Err(FacetError::PatternMismatch { .. })),
            "Contains numbers should fail"
        );
    }

    #[test]
    fn test_email_pattern() {
        let mut constraints =
            FacetConstraints::new().with_pattern(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}");
        constraints.compile_patterns().unwrap();
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("test@example.com").is_ok());
        assert!(validator.validate("user.name@domain.co.jp").is_ok());

        assert!(
            validator.validate("invalid-email").is_err(),
            "Missing @ should fail"
        );
        assert!(
            validator.validate("@domain.com").is_err(),
            "Missing local part should fail"
        );
    }

    #[test]
    fn test_invalid_pattern_regex() {
        // Pattern with invalid regex syntax
        let mut constraints = FacetConstraints::new().with_pattern(r"[invalid(");
        // compile_patterns logs a warning but doesn't fail
        let _ = constraints.compile_patterns();

        let validator = FacetValidator::new(&constraints);

        // Patterns the regex engine cannot express are skipped rather than
        // turned into validation errors, so values pass the pattern facet.
        let result = validator.validate("test");
        assert!(
            result.is_ok(),
            "Unsupported pattern regex should be skipped, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Digit Constraints Tests
// =============================================================================

mod digit_constraints {
    use super::*;

    #[test]
    fn test_total_digits_violation() {
        let constraints = FacetConstraints {
            total_digits: Some(5),
            ..Default::default()
        };
        let validator = FacetValidator::new(&constraints);

        // Valid: within limit
        assert!(validator.validate("12345").is_ok());
        assert!(validator.validate("1234").is_ok());
        assert!(validator.validate("123.45").is_ok()); // 5 significant digits

        // Invalid: too many digits
        let result = validator.validate("123456");
        assert!(
            matches!(result, Err(FacetError::TooManyDigits { found: 6, max: 5 })),
            "6 digits should fail with totalDigits=5, got: {:?}",
            result
        );
    }

    #[test]
    fn test_total_digits_with_leading_zeros() {
        let constraints = FacetConstraints {
            total_digits: Some(3),
            ..Default::default()
        };
        let validator = FacetValidator::new(&constraints);

        // Leading zeros are not significant
        assert!(
            validator.validate("00123").is_ok(),
            "Leading zeros should not count"
        );
        assert!(
            validator.validate("0.123").is_ok(),
            "Leading zero before decimal should not count"
        );
    }

    #[test]
    fn test_fraction_digits_violation() {
        let constraints = FacetConstraints {
            fraction_digits: Some(2),
            ..Default::default()
        };
        let validator = FacetValidator::new(&constraints);

        // Valid
        assert!(validator.validate("1.23").is_ok());
        assert!(validator.validate("1.2").is_ok());
        assert!(validator.validate("1").is_ok());

        // Invalid: too many fraction digits
        let result = validator.validate("1.234");
        assert!(
            matches!(
                result,
                Err(FacetError::TooManyFractionDigits { found: 3, max: 2 })
            ),
            "3 fraction digits should fail with fractionDigits=2, got: {:?}",
            result
        );
    }

    #[test]
    fn test_total_and_fraction_digits_combined() {
        let constraints = FacetConstraints {
            total_digits: Some(5),
            fraction_digits: Some(2),
            ..Default::default()
        };
        let validator = FacetValidator::new(&constraints);

        // Valid
        assert!(validator.validate("123.45").is_ok());
        assert!(validator.validate("12.34").is_ok());

        // Too many total digits
        let result = validator.validate("1234.56");
        assert!(
            matches!(result, Err(FacetError::TooManyDigits { .. })),
            "6 total digits should fail"
        );

        // Too many fraction digits
        let result = validator.validate("12.345");
        assert!(
            matches!(result, Err(FacetError::TooManyFractionDigits { .. })),
            "3 fraction digits should fail"
        );
    }
}

// =============================================================================
// Whitespace Handling Tests
// =============================================================================

mod whitespace_handling {
    use super::*;

    #[test]
    fn test_whitespace_preserve() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Preserve)
            .with_enumeration(vec!["hello  world"]);
        let validator = FacetValidator::new(&constraints);

        // With preserve, whitespace is kept as-is
        assert!(
            validator.validate("hello  world").is_ok(),
            "Exact match should pass"
        );
        assert!(
            validator.validate("hello world").is_err(),
            "Single space should fail"
        );
    }

    #[test]
    fn test_whitespace_replace() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Replace)
            .with_enumeration(vec!["hello world"]);
        let validator = FacetValidator::new(&constraints);

        // With replace, tabs and newlines become spaces but consecutive spaces remain
        assert!(
            validator.validate("hello\tworld").is_ok(),
            "Tab should be replaced with space"
        );
        assert!(
            validator.validate("hello\nworld").is_ok(),
            "Newline should be replaced with space"
        );
    }

    #[test]
    fn test_whitespace_collapse() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Collapse)
            .with_enumeration(vec!["hello world"]);
        let validator = FacetValidator::new(&constraints);

        // With collapse, consecutive whitespace becomes single space, trimmed
        assert!(
            validator.validate("hello  world").is_ok(),
            "Multiple spaces should collapse"
        );
        assert!(
            validator.validate("  hello   world  ").is_ok(),
            "Leading/trailing spaces should be trimmed"
        );
        assert!(
            validator.validate("hello\t\nworld").is_ok(),
            "Mixed whitespace should collapse"
        );
    }

    #[test]
    fn test_whitespace_collapse_with_length() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Collapse)
            .with_min_length(5)
            .with_max_length(15);
        let validator = FacetValidator::new(&constraints);

        // Length is checked after whitespace normalization
        // "  a  " collapses to "a" (length 1)
        let result = validator.validate("  a  ");
        assert!(
            matches!(result, Err(FacetError::TooShort { .. })),
            "Collapsed string is too short"
        );

        // "hello   world" collapses to "hello world" (length 11)
        assert!(
            validator.validate("hello   world").is_ok(),
            "Collapsed string should be valid length"
        );
    }
}

// =============================================================================
// Combined Facet Edge Cases
// =============================================================================

mod combined_facets {
    use super::*;

    #[test]
    fn test_all_length_facets() {
        // minLength and maxLength together
        let constraints = FacetConstraints::new()
            .with_min_length(3)
            .with_max_length(10);
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("ab").is_err()); // too short
        assert!(validator.validate("abc").is_ok()); // min boundary
        assert!(validator.validate("abcdef").is_ok()); // middle
        assert!(validator.validate("abcdefghij").is_ok()); // max boundary
        assert!(validator.validate("abcdefghijk").is_err()); // too long
    }

    #[test]
    fn test_inclusive_and_exclusive_bounds() {
        // minInclusive with maxExclusive
        let mut constraints = FacetConstraints::new().with_min_inclusive("0");
        constraints.max_exclusive = Some("100".to_string());
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("-1").is_err()); // below minInclusive
        assert!(validator.validate("0").is_ok()); // at minInclusive
        assert!(validator.validate("50").is_ok()); // in range
        assert!(validator.validate("99").is_ok()); // just below maxExclusive
        assert!(validator.validate("100").is_err()); // at maxExclusive (fails)
    }

    #[test]
    fn test_pattern_with_length() {
        let mut constraints = FacetConstraints::new()
            .with_pattern(r"[A-Z]+")
            .with_min_length(2)
            .with_max_length(5);
        constraints.compile_patterns().unwrap();
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("A").is_err()); // too short
        assert!(validator.validate("AB").is_ok()); // valid
        assert!(validator.validate("ABCDE").is_ok()); // max length
        assert!(validator.validate("ABCDEF").is_err()); // too long
        assert!(validator.validate("abc").is_err()); // wrong pattern
    }

    #[test]
    fn test_enumeration_with_whitespace() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Collapse)
            .with_enumeration(vec!["red", "green", "blue"]);
        let validator = FacetValidator::new(&constraints);

        // Whitespace is collapsed before checking enumeration
        assert!(validator.validate(" red ").is_ok());
        assert!(validator.validate("  green  ").is_ok());
        assert!(validator.validate("\tblue\n").is_ok());
    }
}

// =============================================================================
// Negative Value Tests
// =============================================================================

mod negative_values {
    use super::*;

    #[test]
    fn test_negative_number_with_min_inclusive() {
        let constraints = FacetConstraints::new().with_min_inclusive("-10");
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("-10").is_ok()); // at boundary
        assert!(validator.validate("-5").is_ok()); // above min
        assert!(validator.validate("0").is_ok()); // positive
        assert!(validator.validate("-11").is_err()); // below min
        assert!(validator.validate("-100").is_err()); // well below
    }

    #[test]
    fn test_negative_number_with_max_inclusive() {
        let constraints = FacetConstraints::new().with_max_inclusive("-5");
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("-5").is_ok()); // at boundary
        assert!(validator.validate("-10").is_ok()); // below max
        assert!(validator.validate("-100").is_ok()); // well below
        assert!(validator.validate("-4").is_err()); // above max
        assert!(validator.validate("0").is_err()); // positive
    }
}
