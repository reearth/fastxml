//! Tests for facet constraints and validation.

use super::*;

#[test]
fn test_length_validation() {
    let constraints = FacetConstraints::new()
        .with_min_length(2)
        .with_max_length(5);

    let validator = FacetValidator::new(&constraints);

    assert!(validator.validate("ab").is_ok());
    assert!(validator.validate("abcde").is_ok());
    assert!(validator.validate("a").is_err());
    assert!(validator.validate("abcdef").is_err());
}

#[test]
fn test_exact_length() {
    let constraints = FacetConstraints::new().with_length(3);
    let validator = FacetValidator::new(&constraints);

    assert!(validator.validate("abc").is_ok());
    assert!(validator.validate("ab").is_err());
    assert!(validator.validate("abcd").is_err());
}

#[test]
fn test_enumeration() {
    let constraints = FacetConstraints::new().with_enumeration(["red", "green", "blue"]);

    let validator = FacetValidator::new(&constraints);

    assert!(validator.validate("red").is_ok());
    assert!(validator.validate("green").is_ok());
    assert!(validator.validate("yellow").is_err());
}

#[test]
fn test_pattern() {
    // Pattern `[a-z]+` matches one or more lowercase letters
    let mut constraints = FacetConstraints::new().with_pattern(r"[a-z]+");
    constraints.compile_patterns().unwrap();

    let validator = FacetValidator::new(&constraints);

    // Valid: all lowercase letters
    assert!(validator.validate("hello").is_ok());
    assert!(validator.validate("world").is_ok());

    // Invalid: contains uppercase
    assert!(validator.validate("Hello").is_err());

    // Invalid: contains numbers
    assert!(validator.validate("hello123").is_err());

    // Invalid: empty string (pattern requires at least one char)
    assert!(validator.validate("").is_err());

    // Pattern is stored
    assert_eq!(constraints.patterns.len(), 1);
    assert_eq!(constraints.compiled_patterns.len(), 1);
}

#[test]
fn test_pattern_multiple() {
    // Multiple patterns - all must match
    let mut constraints = FacetConstraints::new()
        .with_pattern(r"[a-z]+")
        .with_pattern(r".{3,}"); // At least 3 characters
    constraints.compile_patterns().unwrap();

    let validator = FacetValidator::new(&constraints);

    // Valid: lowercase and at least 3 chars
    assert!(validator.validate("hello").is_ok());

    // Invalid: too short
    assert!(validator.validate("hi").is_err());

    // Invalid: contains uppercase
    assert!(validator.validate("Hello").is_err());
}

#[test]
fn test_numeric_range() {
    let constraints = FacetConstraints::new()
        .with_min_inclusive("0")
        .with_max_inclusive("100");

    let validator = FacetValidator::new(&constraints);

    assert!(validator.validate("0").is_ok());
    assert!(validator.validate("50").is_ok());
    assert!(validator.validate("100").is_ok());
    assert!(validator.validate("-1").is_err());
    assert!(validator.validate("101").is_err());
}

#[test]
fn test_whitespace_collapse() {
    let constraints = FacetConstraints::new()
        .with_whitespace(WhitespaceHandling::Collapse)
        .with_enumeration(["hello world"]);

    let validator = FacetValidator::new(&constraints);

    // Multiple spaces should collapse to one
    assert!(validator.validate("hello  world").is_ok());
    assert!(validator.validate("  hello   world  ").is_ok());
}

#[test]
fn test_fraction_digits() {
    let constraints = FacetConstraints {
        fraction_digits: Some(2),
        ..Default::default()
    };

    let validator = FacetValidator::new(&constraints);

    assert!(validator.validate("1.23").is_ok());
    assert!(validator.validate("1.2").is_ok());
    assert!(validator.validate("1").is_ok());
    assert!(validator.validate("1.234").is_err());
}

#[test]
fn test_count_significant_digits() {
    assert_eq!(count_significant_digits("123"), 3);
    assert_eq!(count_significant_digits("1.23"), 3);
    assert_eq!(count_significant_digits("0.123"), 3);
    assert_eq!(count_significant_digits("-123"), 3);
    assert_eq!(count_significant_digits("00123"), 3);
}

#[test]
fn test_count_fraction_digits() {
    assert_eq!(count_fraction_digits("1.23"), 2);
    assert_eq!(count_fraction_digits("1"), 0);
    assert_eq!(count_fraction_digits("1.0"), 1);
    assert_eq!(count_fraction_digits("1.234"), 3);
}
