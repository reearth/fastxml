//! Streaming validation and unified validation tests.

mod common;

// =============================================================================
// Unified Validation Tests (DOM, TwoPass, OnePass, libxml comparison)
// =============================================================================

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
// Note: max_occurs error position differs by validator design:
// - DOM/TwoPass: report parent element position (line 2, <root>)
// - OnePass: report child element position (line 5, <item>)
// This is because DOM/TwoPass detect the violation when validating parent's content model,
// while OnePass detects it when processing the child element itself.
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
