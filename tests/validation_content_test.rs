//! Content model validation tests.

mod common;

use fastxml::schema::validator::XmlSchemaValidationContext;
use fastxml::schema::xsd::content_model::{
    ContentElement, ContentModelError, ContentModelItem, ContentModelValidator, Occurrence,
};
use fastxml::schema::xsd::parse_xsd;

fn validate_xml(xml: &str, xsd: &str) -> bool {
    let doc = fastxml::parse(xml.as_bytes()).expect("Failed to parse XML");
    let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
    let ctx = XmlSchemaValidationContext::new(schema);
    let errors = ctx.validate(&doc).expect("Validation failed");
    errors.iter().all(|e| !e.is_error())
}

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
fn test_choice_invalid_element() {
    let elements = vec![
        ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
        ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
    ];

    let mut validator = ContentModelValidator::choice(elements);

    let result = validator.validate_element("c");
    assert!(
        matches!(&result, Err(ContentModelError::UnexpectedElement { element, .. }) if element == "c"),
        "Expected UnexpectedElement error for 'c', got: {:?}",
        result
    );
}

#[test]
fn test_choice_valid_first_option() {
    let elements = vec![
        ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
        ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
    ];

    let mut validator = ContentModelValidator::choice(elements);

    assert!(validator.validate_element("a").is_ok());
    assert!(validator.validate_complete().is_ok());
}

#[test]
fn test_choice_valid_second_option() {
    let elements = vec![
        ContentModelItem::Element(ContentElement::new("a", Occurrence::required())),
        ContentModelItem::Element(ContentElement::new("b", Occurrence::required())),
    ];

    let mut validator = ContentModelValidator::choice(elements);

    assert!(validator.validate_element("b").is_ok());
    assert!(validator.validate_complete().is_ok());
}

#[test]
fn test_unbounded_sequence() {
    let elements = vec![ContentModelItem::Element(ContentElement::new(
        "item",
        Occurrence::unbounded(0),
    ))];

    let mut validator = ContentModelValidator::sequence(elements);

    // Should accept any number of items
    for _ in 0..100 {
        assert!(validator.validate_element("item").is_ok());
    }
    assert!(validator.validate_complete().is_ok());
}

#[test]
fn test_optional_element() {
    let elements = vec![
        ContentModelItem::Element(ContentElement::new("required", Occurrence::required())),
        ContentModelItem::Element(ContentElement::new("optional", Occurrence::optional())),
    ];

    let mut validator = ContentModelValidator::sequence(elements);

    // Only provide required element
    assert!(validator.validate_element("required").is_ok());
    assert!(validator.validate_complete().is_ok());
}

// =========================================================================
// Integration tests with XML/XSD (libxml comparison)
// =========================================================================

#[test]
fn test_sequence_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
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

    let xml = r#"<?xml version="1.0"?>
<root>
  <first>a</first>
  <second>b</second>
</root>"#;

    assert!(validate_xml(xml, xsd), "Should be valid with correct order");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_sequence_wrong_order_integration() {
    let xsd = r#"<?xml version="1.0"?>
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

    let xml = r#"<?xml version="1.0"?>
<root>
  <second>b</second>
  <first>a</first>
</root>"#;

    assert!(
        !validate_xml(xml, xsd),
        "Should be invalid with wrong order"
    );
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_max_occurs_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" maxOccurs="3"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<root>
  <item>a</item>
  <item>b</item>
</root>"#;

    assert!(validate_xml(xml, xsd), "Should be valid under max");
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
  <item>a</item>
  <item>b</item>
  <item>c</item>
</root>"#;

    assert!(!validate_xml(xml, xsd), "Should be invalid exceeding max");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_min_occurs_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" minOccurs="2" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<root>
  <item>a</item>
  <item>b</item>
</root>"#;

    assert!(validate_xml(xml, xsd), "Should be valid meeting min");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_min_occurs_missing_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:string" minOccurs="2" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<root>
  <item>a</item>
</root>"#;

    assert!(!validate_xml(xml, xsd), "Should be invalid below min");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_choice_valid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:choice>
        <xs:element name="optionA" type="xs:string"/>
        <xs:element name="optionB" type="xs:string"/>
      </xs:choice>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<root>
  <optionB>value</optionB>
</root>"#;

    assert!(validate_xml(xml, xsd), "Should be valid with choice option");
    compare_with_libxml!(validate: xml, xsd);
}

#[test]
fn test_choice_invalid_integration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:choice>
        <xs:element name="optionA" type="xs:string"/>
        <xs:element name="optionB" type="xs:string"/>
      </xs:choice>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let xml = r#"<?xml version="1.0"?>
<root>
  <optionC>value</optionC>
</root>"#;

    assert!(
        !validate_xml(xml, xsd),
        "Should be invalid with unknown element"
    );
    compare_with_libxml!(validate: xml, xsd);
}
