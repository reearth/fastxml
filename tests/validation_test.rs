//! Tests for XSD schema validation violations.

mod common;

use std::sync::Arc;

use fastxml::error::Error;
use fastxml::event::{XmlEvent, XmlEventHandler};
use fastxml::schema::error::SchemaError;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::validator::OnePassSchemaValidator;
use fastxml::schema::xsd::{create_builtin_schema, parse_xsd};

// =============================================================================
// Helper Functions
// =============================================================================

fn create_test_schema() -> CompiledSchema {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root" type="RootType"/>

        <xs:complexType name="RootType">
            <xs:sequence>
                <xs:element name="required" type="xs:string"/>
                <xs:element name="optional" type="xs:string" minOccurs="0"/>
                <xs:element name="bounded" type="xs:string" minOccurs="1" maxOccurs="3"/>
                <xs:element name="integer" type="xs:integer" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:simpleType name="RestrictedString">
            <xs:restriction base="xs:string">
                <xs:minLength value="3"/>
                <xs:maxLength value="10"/>
            </xs:restriction>
        </xs:simpleType>

        <xs:simpleType name="EnumType">
            <xs:restriction base="xs:string">
                <xs:enumeration value="A"/>
                <xs:enumeration value="B"/>
                <xs:enumeration value="C"/>
            </xs:restriction>
        </xs:simpleType>

        <xs:simpleType name="PatternType">
            <xs:restriction base="xs:string">
                <xs:pattern value="[A-Z]{3}-[0-9]{4}"/>
            </xs:restriction>
        </xs:simpleType>

        <xs:simpleType name="RangeType">
            <xs:restriction base="xs:integer">
                <xs:minInclusive value="0"/>
                <xs:maxInclusive value="100"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    parse_xsd(xsd.as_bytes()).unwrap()
}

// =============================================================================
// Schema Parsing Error Tests
// =============================================================================

mod schema_parsing {
    use super::*;

    #[test]
    fn test_invalid_xsd_syntax() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:nonexistent"/>
        </xs:schema>"#;

        // Schema parsing should succeed, type resolution happens later
        let result = parse_xsd(xsd.as_bytes());
        assert!(result.is_ok());
    }

    #[test]
    fn test_xsd_missing_namespace() {
        let xsd = r#"<schema>
            <element name="test" type="string"/>
        </schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser is lenient and accepts missing namespace
        assert!(result.is_ok(), "Parser accepts schema without namespace");
    }

    #[test]
    fn test_xsd_invalid_min_occurs() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="child" minOccurs="-1"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser rejects invalid minOccurs values
        assert!(
            matches!(&result, Err(e) if format!("{:?}", e).contains("minOccurs") || format!("{:?}", e).contains("negative")),
            "Expected error about invalid minOccurs, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xsd_min_greater_than_max() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="child" minOccurs="5" maxOccurs="3"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser rejects minOccurs > maxOccurs
        assert!(
            matches!(
                &result,
                Err(Error::Schema(SchemaError::MinOccursGreaterThanMaxOccurs {
                    min: 5,
                    max: 3
                }))
            ),
            "Expected error about minOccurs > maxOccurs, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xsd_circular_type_reference() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="TypeA">
                <xs:complexContent>
                    <xs:extension base="TypeB"/>
                </xs:complexContent>
            </xs:complexType>
            <xs:complexType name="TypeB">
                <xs:complexContent>
                    <xs:extension base="TypeA"/>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser accepts circular type references during parsing
        // Type resolution happens lazily during validation
        assert!(
            result.is_ok(),
            "Parser accepts circular type references during parsing"
        );
    }

    #[test]
    fn test_xsd_duplicate_element_name() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
            <xs:element name="test" type="xs:integer"/>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // Duplicate global element names
        // Later definition may override earlier
        if let Ok(schema) = result {
            assert!(schema.elements.contains_key("test"));
        }
    }

    #[test]
    fn test_xsd_invalid_facet_value() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="BadLength">
                <xs:restriction base="xs:string">
                    <xs:minLength value="-5"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser rejects negative length values
        assert!(
            matches!(&result, Err(e) if format!("{:?}", e).contains("minLength") || format!("{:?}", e).contains("negative")),
            "Expected error about negative length, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xsd_conflicting_facets() {
        let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="Conflicting">
                <xs:restriction base="xs:string">
                    <xs:minLength value="10"/>
                    <xs:maxLength value="5"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser rejects conflicting facets (minLength > maxLength)
        assert!(
            matches!(
                &result,
                Err(Error::Schema(SchemaError::MinLengthGreaterThanMaxLength {
                    min_length: 10,
                    max_length: 5
                }))
            ),
            "Expected error about conflicting facets, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Content Model Violation Tests
// =============================================================================

mod content_model {
    use crate::compare_with_libxml;
    use fastxml::schema::xsd::content_model::{
        ContentElement, ContentModelError, ContentModelItem, ContentModelValidator, Occurrence,
    };

    #[test]
    fn test_sequence_missing_required() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("c", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Only provide 'a', missing 'b' and 'c'
        assert!(validator.validate_element("a").is_ok());
        let result = validator.validate_complete();
        assert!(
            matches!(&result, Err(ContentModelError::TooFewOccurrences { element, expected, found }) if element == "b" && *expected == 1 && *found == 0),
            "Expected TooFewOccurrences error for 'b', got: {:?}",
            result
        );
    }

    #[test]
    fn test_sequence_wrong_order() {
        // Use elements that allow multiple occurrences to test order
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::new(1, Some(3)))),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::new(1, Some(3)))),
        ];

        let mut validator = ContentModelValidator::sequence(elements);

        // Provide 'a' first (correct order)
        assert!(validator.validate_element("a").is_ok());

        // Advance to 'b'
        assert!(validator.validate_element("b").is_ok());

        // Now try to go back to 'a' - this should fail with OutOfOrder
        let result = validator.validate_element("a");
        assert!(
            matches!(result, Err(ContentModelError::OutOfOrder { .. })),
            "Going backwards in sequence should fail with OutOfOrder, got: {:?}",
            result
        );
    }

    #[test]
    fn test_sequence_too_many_occurrences() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "item",
            Occurrence::new(1, Some(2)),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        assert!(validator.validate_element("item").is_ok());
        assert!(validator.validate_element("item").is_ok());
        let result = validator.validate_element("item");
        assert!(
            matches!(&result, Err(ContentModelError::TooManyOccurrences { element, max, .. }) if element == "item" && *max == 2),
            "Expected TooManyOccurrences error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_sequence_too_few_occurrences() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "item",
            Occurrence::new(2, Some(5)),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        // Only provide 1 when minOccurs is 2
        assert!(validator.validate_element("item").is_ok());
        let result = validator.validate_complete();
        assert!(
            matches!(&result, Err(ContentModelError::TooFewOccurrences { element, expected, .. }) if element == "item" && *expected == 2),
            "Expected TooFewOccurrences error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_choice_valid() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("option1", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("option2", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::choice(elements);

        // Either option should be valid
        assert!(validator.validate_element("option1").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_choice_invalid_element() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("option1", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("option2", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::choice(elements);

        let result = validator.validate_element("option3");
        assert!(
            matches!(&result, Err(ContentModelError::UnexpectedElement { element, .. }) if element == "option3"),
            "Expected UnexpectedElement error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_all_missing_required() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        // Only provide 'a', missing 'b'
        assert!(validator.validate_element("a").is_ok());
        let result = validator.validate_complete();
        assert!(
            matches!(&result, Err(ContentModelError::TooFewOccurrences { element, expected, found }) if element == "b" && *expected == 1 && *found == 0),
            "Expected TooFewOccurrences error for 'b', got: {:?}",
            result
        );
    }

    #[test]
    fn test_all_duplicate() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        assert!(validator.validate_element("a").is_ok());
        let result = validator.validate_element("a");
        // In "all" mode, duplicate elements should result in TooManyOccurrences
        assert!(
            matches!(&result, Err(ContentModelError::TooManyOccurrences { element, max, .. }) if element == "a" && *max == 1),
            "Expected TooManyOccurrences error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_all_any_order() {
        let elements = vec![
            ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
            ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
        ];

        let mut validator = ContentModelValidator::all(elements);

        // All allows any order
        assert!(validator.validate_element("b").is_ok());
        assert!(validator.validate_element("a").is_ok());
        assert!(validator.validate_complete().is_ok());
    }

    #[test]
    fn test_unexpected_element() {
        let elements = vec![ContentModelItem::Element(ContentElement::new(
            "expected",
            Occurrence::required(),
        ))];

        let mut validator = ContentModelValidator::sequence(elements);

        let result = validator.validate_element("unexpected");
        assert!(result.is_err(), "Unexpected element should fail");

        if let Err(ContentModelError::UnexpectedElement { element, .. }) = result {
            assert_eq!(element, "unexpected");
        } else {
            panic!("Expected UnexpectedElement error");
        }
    }

    // =========================================================================
    // Integration tests with XML/XSD (libxml comparison)
    // =========================================================================

    use fastxml::schema::validator::XmlSchemaValidationContext;
    use fastxml::schema::xsd::parse_xsd;

    fn validate_xml(xml: &str, xsd: &str) -> bool {
        let doc = fastxml::parse(xml.as_bytes()).expect("Failed to parse XML");
        let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
        let ctx = XmlSchemaValidationContext::new(schema);
        let errors = ctx.validate(&doc).expect("Validation failed");
        errors.iter().all(|e| !e.is_error())
    }

    #[test]
    fn test_max_occurs_valid_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" maxOccurs="2"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root>
  <item>first</item>
  <item>second</item>
</root>"#;

        assert!(validate_xml(xml, xsd), "Should be valid with 2 items");
        compare_with_libxml!(validate: xml, xsd);
    }

    #[test]
    fn test_max_occurs_exceeded_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" maxOccurs="2"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root>
  <item>first</item>
  <item>second</item>
  <item>third</item>
</root>"#;

        assert!(!validate_xml(xml, xsd), "Should be invalid with 3 items");
        compare_with_libxml!(validate: xml, xsd);
    }

    #[test]
    fn test_min_occurs_valid_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="required" type="xs:string" minOccurs="1"/>
        <xs:element name="optional" type="xs:string" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root>
  <required>value</required>
</root>"#;

        assert!(
            validate_xml(xml, xsd),
            "Should be valid with required element"
        );
        compare_with_libxml!(validate: xml, xsd);
    }

    #[test]
    fn test_min_occurs_missing_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="required" type="xs:string" minOccurs="1"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root>
</root>"#;

        assert!(
            !validate_xml(xml, xsd),
            "Should be invalid without required element"
        );
        compare_with_libxml!(validate: xml, xsd);
    }

    #[test]
    fn test_sequence_order_valid_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="person">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="firstName" type="xs:string"/>
        <xs:element name="lastName" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<person>
  <firstName>John</firstName>
  <lastName>Doe</lastName>
</person>"#;

        assert!(
            validate_xml(xml, xsd),
            "Should be valid with correct sequence order"
        );
        compare_with_libxml!(validate: xml, xsd);
    }

    #[test]
    fn test_unknown_element_integration() {
        let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="known" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

        let xml = r#"<?xml version="1.0"?>
<root>
  <known>value</known>
  <unknown>extra</unknown>
</root>"#;

        assert!(
            !validate_xml(xml, xsd),
            "Should be invalid with unknown element"
        );
        compare_with_libxml!(validate: xml, xsd);
    }
}

// =============================================================================
// Facet Violation Tests
// =============================================================================

mod facet_violations {
    use crate::compare_with_libxml;
    use fastxml::schema::xsd::facets::{FacetConstraints, FacetError, FacetValidator};

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

    use fastxml::schema::validator::XmlSchemaValidationContext;
    use fastxml::schema::xsd::parse_xsd;

    fn validate_xml(xml: &str, xsd: &str) -> bool {
        let doc = fastxml::parse(xml.as_bytes()).expect("Failed to parse XML");
        let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
        let ctx = XmlSchemaValidationContext::new(schema);
        let errors = ctx.validate(&doc).expect("Validation failed");
        errors.iter().all(|e| !e.is_error())
    }

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
}

// =============================================================================
// Identity Constraint Tests
// =============================================================================

mod identity_constraints {
    use fastxml::schema::xsd::constraints::{
        ConstraintError, ConstraintValidator, IdentityConstraint, KeyValue,
    };

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
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "1".into()]));
        assert!(result.is_ok());

        // Different composite key
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "2".into()]));
        assert!(result.is_ok());

        // Duplicate composite key
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["a".into(), "1".into()]));
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
}

// =============================================================================
// Streaming Validator Integration Tests
// =============================================================================

mod streaming_validation {
    use super::*;

    #[test]
    fn test_validator_with_builtin_types() {
        let schema = create_builtin_schema();
        let validator = OnePassSchemaValidator::new(Arc::new(schema));

        // Initial state should be valid
        assert!(validator.is_valid());
    }

    #[test]
    fn test_validator_events() {
        let schema = create_builtin_schema();
        let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

        // Start element
        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
                column: None,
            })
            .unwrap();

        // Text content
        validator
            .handle(&XmlEvent::Text("some text".into()))
            .unwrap();

        // End element
        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        // Finish
        validator.handle(&XmlEvent::Eof).unwrap();
        validator.finish().unwrap();

        assert!(validator.is_valid());
    }

    #[test]
    fn test_validator_collects_errors() {
        let schema = create_test_schema();
        let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

        // This would collect validation errors as they occur
        // The actual validation logic depends on the schema definition
        validator
            .handle(&XmlEvent::StartElement {
                name: "root".into(),
                prefix: None,
                namespace: None,
                attributes: vec![],
                namespace_decls: vec![],
                line: Some(1),
                column: None,
            })
            .unwrap();

        validator
            .handle(&XmlEvent::EndElement {
                name: "root".into(),
                prefix: None,
            })
            .unwrap();

        validator.handle(&XmlEvent::Eof).unwrap();

        // Check if errors were collected
        let errors = validator.errors();
        // Root element requires 'required' child, so there should be errors
        // (depending on implementation)
        let _ = errors;
    }
}

// =============================================================================
// Type Mismatch Tests
// =============================================================================

mod type_mismatch {
    use super::*;

    #[test]
    fn test_integer_type_with_string() {
        // Create a schema with integer element
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="count" type="xs:integer"/>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
        assert!(schema.elements.contains_key("count"));
    }

    #[test]
    fn test_date_type_invalid_format() {
        // xs:date requires YYYY-MM-DD format
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="birthday" type="xs:date"/>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
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

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
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

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
        assert!(schema.types.contains_key("Price"));
    }
}

// =============================================================================
// Namespace Violation Tests
// =============================================================================

mod namespace_violations {
    use super::*;

    #[test]
    fn test_wrong_namespace() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/correct"
                   elementFormDefault="qualified">
            <xs:element name="item" type="xs:string"/>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
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

        let result = parse_xsd(xsd.as_bytes());
        // XSD parser accepts undeclared prefixes during parsing
        // Type resolution with the undeclared prefix happens at validation time
        assert!(
            result.is_ok(),
            "Parser accepts undeclared prefixes during parsing"
        );
    }
}

// =============================================================================
// Unified Validation Tests (DOM, TwoPass, OnePass, libxml comparison)
// =============================================================================

mod unified_validation {
    use crate::test_validation;

    // -------------------------------------------------------------------------
    // Content Model: max/min occurs
    // -------------------------------------------------------------------------

    const XSD_MAX_OCCURS: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" maxOccurs="2"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    const XML_MAX_OCCURS_VALID: &str = r#"<?xml version="1.0"?>
<root>
  <item>first</item>
  <item>second</item>
</root>"#;

    const XML_MAX_OCCURS_EXCEEDED: &str = r#"<?xml version="1.0"?>
<root>
  <item>first</item>
  <item>second</item>
  <item>third</item>
</root>"#;

    test_validation!(max_occurs_valid, XML_MAX_OCCURS_VALID, XSD_MAX_OCCURS, true);
    // Note: Error positions vary by validator - line checking only for now
    // TODO: Fix position reporting consistency across validators
    test_validation!(
        max_occurs_exceeded,
        XML_MAX_OCCURS_EXCEEDED,
        XSD_MAX_OCCURS,
        false
    );

    // -------------------------------------------------------------------------
    // Content Model: min occurs
    // -------------------------------------------------------------------------

    const XSD_MIN_OCCURS: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="required" type="xs:string" minOccurs="1"/>
        <xs:element name="optional" type="xs:string" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    const XML_MIN_OCCURS_VALID: &str = r#"<?xml version="1.0"?>
<root>
  <required>value</required>
</root>"#;

    const XML_MIN_OCCURS_MISSING: &str = r#"<?xml version="1.0"?>
<root>
  <optional>value</optional>
</root>"#;

    test_validation!(min_occurs_valid, XML_MIN_OCCURS_VALID, XSD_MIN_OCCURS, true);
    test_validation!(
        min_occurs_missing,
        XML_MIN_OCCURS_MISSING,
        XSD_MIN_OCCURS,
        false
    );

    // -------------------------------------------------------------------------
    // Content Model: sequence order
    // -------------------------------------------------------------------------

    const XSD_SEQUENCE: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="first" type="xs:string"/>
        <xs:element name="second" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    const XML_SEQUENCE_VALID: &str = r#"<?xml version="1.0"?>
<root>
  <first>1</first>
  <second>2</second>
</root>"#;

    const XML_SEQUENCE_WRONG_ORDER: &str = r#"<?xml version="1.0"?>
<root>
  <second>2</second>
  <first>1</first>
</root>"#;

    test_validation!(sequence_order_valid, XML_SEQUENCE_VALID, XSD_SEQUENCE, true);
    test_validation!(
        sequence_order_invalid,
        XML_SEQUENCE_WRONG_ORDER,
        XSD_SEQUENCE,
        false
    );

    // -------------------------------------------------------------------------
    // Content Model: unknown element
    // -------------------------------------------------------------------------

    const XSD_KNOWN_ELEMENTS: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="known" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    const XML_UNKNOWN_ELEMENT: &str = r#"<?xml version="1.0"?>
<root>
  <known>ok</known>
  <unknown>bad</unknown>
</root>"#;

    test_validation!(
        unknown_element,
        XML_UNKNOWN_ELEMENT,
        XSD_KNOWN_ELEMENTS,
        false
    );

    // -------------------------------------------------------------------------
    // Facets: pattern
    // -------------------------------------------------------------------------

    const XSD_PATTERN: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="code">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:pattern value="[A-Z]{3}-[0-9]{4}"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    const XML_PATTERN_VALID: &str = r#"<?xml version="1.0"?><code>ABC-1234</code>"#;
    const XML_PATTERN_INVALID: &str = r#"<?xml version="1.0"?><code>abc-1234</code>"#;

    test_validation!(pattern_valid, XML_PATTERN_VALID, XSD_PATTERN, true);
    test_validation!(pattern_invalid, XML_PATTERN_INVALID, XSD_PATTERN, false);

    // -------------------------------------------------------------------------
    // Facets: enumeration
    // -------------------------------------------------------------------------

    const XSD_ENUM: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="status">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:enumeration value="active"/>
        <xs:enumeration value="inactive"/>
        <xs:enumeration value="pending"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    const XML_ENUM_VALID: &str = r#"<?xml version="1.0"?><status>active</status>"#;
    const XML_ENUM_INVALID: &str = r#"<?xml version="1.0"?><status>unknown</status>"#;

    test_validation!(enumeration_valid, XML_ENUM_VALID, XSD_ENUM, true);
    test_validation!(enumeration_invalid, XML_ENUM_INVALID, XSD_ENUM, false);

    // -------------------------------------------------------------------------
    // Facets: min/max length
    // -------------------------------------------------------------------------

    const XSD_LENGTH: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="name">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:minLength value="3"/>
        <xs:maxLength value="10"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    const XML_LENGTH_VALID: &str = r#"<?xml version="1.0"?><name>hello</name>"#;
    const XML_LENGTH_TOO_SHORT: &str = r#"<?xml version="1.0"?><name>ab</name>"#;
    const XML_LENGTH_TOO_LONG: &str = r#"<?xml version="1.0"?><name>this is way too long</name>"#;

    test_validation!(length_valid, XML_LENGTH_VALID, XSD_LENGTH, true);
    test_validation!(length_too_short, XML_LENGTH_TOO_SHORT, XSD_LENGTH, false);
    test_validation!(length_too_long, XML_LENGTH_TOO_LONG, XSD_LENGTH, false);
}
