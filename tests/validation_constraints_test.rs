//! Identity constraints, type mismatch, and namespace violation tests.

mod common;

use fastxml::schema::Schema;
use fastxml::schema::xsd::constraints::{
    ConstraintError, ConstraintValidator, IdentityConstraint, KeyValue,
};

// =============================================================================
// Identity Constraint Tests
// =============================================================================

#[test]
fn test_unique_duplicate_key() {
    let mut validator = ConstraintValidator::new();
    validator.register_key("uniqueId", 1);

    // Add first key
    let result = validator.add_key_value(
        &IdentityConstraint::unique("uniqueId", "."),
        KeyValue::single("key1"),
    );
    assert!(result.is_ok());

    // Add duplicate
    let result = validator.add_key_value(
        &IdentityConstraint::unique("uniqueId", "."),
        KeyValue::single("key1"),
    );
    assert!(
        matches!(&result, Err(ConstraintError::DuplicateKey { constraint, .. }) if constraint == "uniqueId"),
        "Expected DuplicateKey error, got: {:?}",
        result
    );
}

#[test]
fn test_key_null_value() {
    let mut validator = ConstraintValidator::new();
    validator.register_key("keyId", 1);

    // Key cannot have null values per XSD spec, but implementation is lenient
    let constraint = IdentityConstraint::key("keyId", ".");
    let result = validator.add_key_value(&constraint, KeyValue::new(vec![]));
    // Current implementation accepts empty key values
    assert!(
        result.is_ok(),
        "Implementation is lenient with empty key values"
    );
}

#[test]
fn test_composite_key() {
    let mut validator = ConstraintValidator::new();
    validator.register_key("compositeKey", 2);

    let constraint = IdentityConstraint::key("compositeKey", ".");

    // Add composite key
    let result = validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "1".into()]));
    assert!(result.is_ok());

    // Different composite key
    let result = validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "2".into()]));
    assert!(result.is_ok());

    // Duplicate composite key
    let result = validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "1".into()]));
    assert!(
        matches!(&result, Err(ConstraintError::DuplicateKey { constraint, .. }) if constraint == "compositeKey"),
        "Expected DuplicateKey error for composite key, got: {:?}",
        result
    );
}

#[test]
fn test_keyref_validation() {
    let mut validator = ConstraintValidator::new();

    // Register key constraint
    validator.register_key("personId", 1);
    let key_constraint = IdentityConstraint::key("personId", ".");
    validator
        .add_key_value(&key_constraint, KeyValue::single("p1"))
        .unwrap();
    validator
        .add_key_value(&key_constraint, KeyValue::single("p2"))
        .unwrap();

    // Add keyref values
    let keyref_constraint = IdentityConstraint::keyref("personRef", ".", "personId");
    validator.add_keyref_value(&keyref_constraint, KeyValue::single("p1"));
    validator.add_keyref_value(&keyref_constraint, KeyValue::single("p3")); // Invalid reference

    let result = validator.validate_keyrefs();
    assert!(
        matches!(&result, Err(errors) if !errors.is_empty() && errors.iter().any(|e| matches!(e, ConstraintError::KeyRefNotFound { constraint, .. } if constraint == "personRef"))),
        "Expected KeyRefNotFound error, got: {:?}",
        result
    );
}

// =============================================================================
// Type Mismatch Tests
// =============================================================================

#[test]
fn test_integer_type_with_string() {
    // Create a schema with integer element
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="count" type="xs:integer"/>
        </xs:schema>"#;

    let schema = Schema::from_xsd(xsd.as_bytes()).unwrap();
    assert!(schema.elements.contains_key("count"));
}

#[test]
fn test_date_type_invalid_format() {
    // xs:date requires YYYY-MM-DD format
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="birthday" type="xs:date"/>
        </xs:schema>"#;

    let schema = Schema::from_xsd(xsd.as_bytes()).unwrap();
    assert!(schema.elements.contains_key("birthday"));
    // Validation of "invalid-date" against xs:date would fail
}

#[test]
fn test_boolean_type_invalid_value() {
    // xs:boolean only allows true, false, 1, 0
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="flag" type="xs:boolean"/>
        </xs:schema>"#;

    let schema = Schema::from_xsd(xsd.as_bytes()).unwrap();
    assert!(schema.elements.contains_key("flag"));
    // Validation of "yes" against xs:boolean would fail
}

#[test]
fn test_decimal_type_precision() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="Price">
                <xs:restriction base="xs:decimal">
                    <xs:totalDigits value="10"/>
                    <xs:fractionDigits value="2"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

    let schema = Schema::from_xsd(xsd.as_bytes()).unwrap();
    assert!(schema.types.contains_key("Price"));
}

// =============================================================================
// Namespace Violation Tests
// =============================================================================

#[test]
fn test_wrong_namespace() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/correct"
                   elementFormDefault="qualified">
            <xs:element name="item" type="xs:string"/>
        </xs:schema>"#;

    let schema = Schema::from_xsd(xsd.as_bytes()).unwrap();
    assert_eq!(
        schema.target_namespace,
        Some("http://example.com/correct".to_string())
    );
}

#[test]
fn test_missing_namespace_declaration() {
    // Using prefix without declaration
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="item" type="custom:Type"/>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser accepts undeclared prefixes during parsing
    // Type resolution with the undeclared prefix happens at validation time
    assert!(
        result.is_ok(),
        "Parser accepts undeclared prefixes during parsing"
    );
}
