//! Facet violation tests.

mod common;

use fastxml::Parser;
use fastxml::schema::xsd::facets::{FacetConstraints, FacetError, FacetValidator};
use fastxml::schema::{Schema, Validator};

fn validate_xml(xml: &str, xsd: &str) -> bool {
    let doc = Parser::from(xml).parse().expect("Failed to parse XML");
    let schema = Schema::from_xsd(xsd).expect("Failed to parse XSD");
    Validator::from(&doc)
        .schema(schema)
        .run()
        .expect("Validation failed")
        .is_valid()
}

#[test]
fn test_min_length_violation() {
    let constraints = FacetConstraints::new().with_min_length(5);
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("ab");
    assert!(
        matches!(result, Err(FacetError::TooShort { min_len, value_len }) if min_len == 5 && value_len == 2),
        "Expected TooShort error, got: {:?}",
        result
    );
}

#[test]
fn test_max_length_violation() {
    let constraints = FacetConstraints::new().with_max_length(5);
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("toolongstring");
    assert!(
        matches!(result, Err(FacetError::TooLong { max_len, value_len }) if max_len == 5 && value_len == 13),
        "Expected TooLong error, got: {:?}",
        result
    );
}

#[test]
fn test_exact_length_violation() {
    let constraints = FacetConstraints::new().with_length(5);
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("abc");
    assert!(
        matches!(result, Err(FacetError::WrongLength { required_len, value_len }) if required_len == 5 && value_len == 3),
        "Expected WrongLength error, got: {:?}",
        result
    );

    let result = validator.validate("abcdefgh");
    assert!(
        matches!(result, Err(FacetError::WrongLength { required_len, value_len }) if required_len == 5 && value_len == 8),
        "Expected WrongLength error, got: {:?}",
        result
    );

    let result = validator.validate("abcde");
    assert!(result.is_ok(), "String matching exact length should pass");
}

#[test]
fn test_enumeration_violation() {
    let constraints = FacetConstraints::new().with_enumeration(vec!["red", "green", "blue"]);
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("yellow");
    assert!(
        matches!(&result, Err(FacetError::NotInEnumeration { value, .. }) if value == "yellow"),
        "Expected NotInEnumeration error, got: {:?}",
        result
    );

    let result = validator.validate("red");
    assert!(result.is_ok(), "Value in enumeration should pass");
}

#[test]
fn test_min_inclusive_violation() {
    let constraints = FacetConstraints::new().with_min_inclusive("10");
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("5");
    assert!(
        matches!(result, Err(FacetError::BelowMinInclusive { .. })),
        "Expected BelowMinInclusive error, got: {:?}",
        result
    );

    let result = validator.validate("10");
    assert!(result.is_ok(), "Value equal to minInclusive should pass");
}

#[test]
fn test_max_inclusive_violation() {
    let constraints = FacetConstraints::new().with_max_inclusive("100");
    let validator = FacetValidator::new(&constraints);

    let result = validator.validate("150");
    assert!(
        matches!(result, Err(FacetError::AboveMaxInclusive { .. })),
        "Expected AboveMaxInclusive error, got: {:?}",
        result
    );

    let result = validator.validate("100");
    assert!(result.is_ok(), "Value equal to maxInclusive should pass");
}

#[test]
fn test_combined_facets() {
    let constraints = FacetConstraints::new()
        .with_min_length(2)
        .with_max_length(5)
        .with_enumeration(vec!["abc", "def"]);
    let validator = FacetValidator::new(&constraints);

    // Must satisfy all facets
    let result = validator.validate("a");
    assert!(
        matches!(result, Err(FacetError::TooShort { .. })),
        "Expected TooShort error, got: {:?}",
        result
    );

    let result = validator.validate("abcdef");
    assert!(
        matches!(result, Err(FacetError::TooLong { .. })),
        "Expected TooLong error, got: {:?}",
        result
    );

    let result = validator.validate("xyz");
    assert!(
        matches!(result, Err(FacetError::NotInEnumeration { .. })),
        "Expected NotInEnumeration error, got: {:?}",
        result
    );

    let result = validator.validate("abc");
    assert!(result.is_ok(), "Should pass all facets");
}

// =========================================================================
// Integration tests with XML/XSD (libxml comparison)
// =========================================================================

#[test]
fn test_pattern_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="code">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:pattern value="[A-Z]{3}"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<code>ABC</code>"#;

    assert!(validate_xml(xml, xsd), "Should be valid with pattern match");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_pattern_invalid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="code">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:pattern value="[A-Z]{3}"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<code>abc</code>"#;

    assert!(
        !validate_xml(xml, xsd),
        "Should be invalid with pattern mismatch"
    );
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_enumeration_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="color">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:enumeration value="red"/>
        <xs:enumeration value="green"/>
        <xs:enumeration value="blue"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<color>green</color>"#;

    assert!(
        validate_xml(xml, xsd),
        "Should be valid with enumeration value"
    );
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_enumeration_invalid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="color">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:enumeration value="red"/>
        <xs:enumeration value="green"/>
        <xs:enumeration value="blue"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<color>yellow</color>"#;

    assert!(
        !validate_xml(xml, xsd),
        "Should be invalid with non-enumeration value"
    );
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_min_length_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="name">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:minLength value="3"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<name>John</name>"#;

    assert!(
        validate_xml(xml, xsd),
        "Should be valid with sufficient length"
    );
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_min_length_invalid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="name">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:minLength value="3"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<name>Jo</name>"#;

    assert!(
        !validate_xml(xml, xsd),
        "Should be invalid with insufficient length"
    );
    compare_with_libxml!(validate: xml, xsd);
}
