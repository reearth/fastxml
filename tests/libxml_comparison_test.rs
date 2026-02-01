//! Comparison tests between fastxml and libxml schema validation.
//!
//! These tests ensure that fastxml's schema validation produces the same
//! results as libxml for various validation scenarios.

#![cfg(feature = "compare-libxml")]

use fastxml::schema::validator::XmlSchemaValidationContext;
use fastxml::schema::xsd::parse_xsd;

/// Helper to validate with fastxml
fn validate_with_fastxml(xml: &str, xsd: &str) -> (bool, Vec<String>) {
    let doc = fastxml::parse(xml.as_bytes()).expect("Failed to parse XML");
    let schema = parse_xsd(xsd.as_bytes()).expect("Failed to parse XSD");
    let ctx = XmlSchemaValidationContext::new(schema);
    let errors = ctx.validate(&doc).expect("Validation failed");
    let is_valid = errors.iter().all(|e| !e.is_error());
    let messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
    (is_valid, messages)
}

/// Helper to validate with libxml
fn validate_with_libxml(xml: &str, xsd: &str) -> (bool, Vec<String>) {
    use libxml::parser::Parser;
    use libxml::schemas::{SchemaParserContext, SchemaValidationContext};

    let parser = Parser::default();
    let doc = parser
        .parse_string(xml)
        .expect("libxml: Failed to parse XML");

    let mut schema_parser = SchemaParserContext::from_buffer(xsd.as_bytes());
    let mut ctx = SchemaValidationContext::from_parser(&mut schema_parser)
        .expect("libxml: Failed to create validation context");

    let result = ctx.validate_document(&doc);
    let is_valid = result.is_ok();

    let messages: Vec<String> = if let Err(errors) = result {
        errors
            .iter()
            .filter_map(|e| e.message.clone())
            .collect()
    } else {
        vec![]
    };

    (is_valid, messages)
}

// =============================================================================
// maxOccurs validation tests
// =============================================================================

#[test]
fn test_max_occurs_valid() {
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

    let (fastxml_valid, fastxml_msgs) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    if !fastxml_msgs.is_empty() {
        eprintln!("fastxml errors: {:?}", fastxml_msgs);
    }

    assert_eq!(
        fastxml_valid, libxml_valid,
        "maxOccurs=2 with 2 items: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_max_occurs_exceeded() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "maxOccurs=2 with 3 items: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}

// =============================================================================
// minOccurs validation tests
// =============================================================================

#[test]
fn test_min_occurs_valid() {
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

    let (fastxml_valid, fastxml_msgs) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    if !fastxml_msgs.is_empty() {
        eprintln!("fastxml errors: {:?}", fastxml_msgs);
    }

    assert_eq!(
        fastxml_valid, libxml_valid,
        "minOccurs with required present: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_min_occurs_missing() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "minOccurs=1 with missing element: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}

// =============================================================================
// Pattern validation tests
// =============================================================================

#[test]
fn test_pattern_valid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Pattern [A-Z]{{3}} with 'ABC': fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_pattern_invalid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Pattern [A-Z]{{3}} with 'abc': fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}

// =============================================================================
// Enumeration validation tests
// =============================================================================

#[test]
fn test_enumeration_valid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Enumeration with valid value: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_enumeration_invalid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Enumeration with invalid value: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}

// =============================================================================
// Length constraint tests
// =============================================================================

#[test]
fn test_min_length_valid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "minLength=3 with 'John': fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_min_length_invalid() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "minLength=3 with 'Jo': fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}

// =============================================================================
// Complex content tests
// =============================================================================

#[test]
fn test_sequence_order_valid() {
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

    let (fastxml_valid, fastxml_msgs) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    if !fastxml_msgs.is_empty() {
        eprintln!("fastxml errors: {:?}", fastxml_msgs);
    }

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Sequence in correct order: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(fastxml_valid, "Both should be valid");
}

#[test]
fn test_unknown_element() {
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

    let (fastxml_valid, _) = validate_with_fastxml(xml, xsd);
    let (libxml_valid, _) = validate_with_libxml(xml, xsd);

    assert_eq!(
        fastxml_valid, libxml_valid,
        "Unknown element in sequence: fastxml={}, libxml={}",
        fastxml_valid, libxml_valid
    );
    assert!(!fastxml_valid, "Both should be invalid");
}
