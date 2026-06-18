//! Identity-constraint key tuples must be compared in the *value space* of
//! each field's type, not lexically. e.g. the xs:integer values "1" and "01"
//! denote the same key, so a keyref to "01" resolves a key "1", and two keys
//! "1" / "01" collide.
//!
//! The DOM validator already canonicalizes field values; these tests pin the
//! same behavior for the streaming (OnePass) validator.

mod common;

use common::{validate_dom, validate_onepass};

/// A keyref whose lexical form differs from the key it points at, but which
/// is equal in the integer value space, must resolve.
const KEYREF_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" maxOccurs="unbounded">
          <xs:complexType>
            <xs:attribute name="id" type="xs:integer"/>
          </xs:complexType>
        </xs:element>
        <xs:element name="ref" maxOccurs="unbounded">
          <xs:complexType>
            <xs:attribute name="to" type="xs:integer"/>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="itemKey">
      <xs:selector xpath="item"/>
      <xs:field xpath="@id"/>
    </xs:key>
    <xs:keyref name="itemRef" refer="itemKey">
      <xs:selector xpath="ref"/>
      <xs:field xpath="@to"/>
    </xs:keyref>
  </xs:element>
</xs:schema>"#;

const KEYREF_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <item id="1"/>
  <ref to="01"/>
</root>"#;

#[test]
fn keyref_resolves_in_value_space_dom() {
    let result = validate_dom(KEYREF_XML, KEYREF_XSD);
    assert!(
        result.is_valid(),
        "DOM: keyref '01' should resolve key '1' in the integer value space, errors: {:?}",
        result.errors()
    );
}

#[test]
fn keyref_resolves_in_value_space_onepass() {
    let result = validate_onepass(KEYREF_XML, KEYREF_XSD);
    assert!(
        result.is_valid(),
        "OnePass: keyref '01' should resolve key '1' in the integer value space, errors: {:?}",
        result.errors()
    );
}

/// Two keys equal in the value space ("1" and "01") collide and must be
/// rejected as duplicates.
const DUP_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" maxOccurs="unbounded">
          <xs:complexType>
            <xs:attribute name="id" type="xs:integer"/>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="itemKey">
      <xs:selector xpath="item"/>
      <xs:field xpath="@id"/>
    </xs:key>
  </xs:element>
</xs:schema>"#;

const DUP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <item id="1"/>
  <item id="01"/>
</root>"#;

#[test]
fn duplicate_key_detected_in_value_space_dom() {
    let result = validate_dom(DUP_XML, DUP_XSD);
    assert!(
        !result.is_valid(),
        "DOM: keys '1' and '01' are equal in the integer value space and must collide"
    );
}

#[test]
fn duplicate_key_detected_in_value_space_onepass() {
    let result = validate_onepass(DUP_XML, DUP_XSD);
    assert!(
        !result.is_valid(),
        "OnePass: keys '1' and '01' are equal in the integer value space and must collide"
    );
}

/// The same value-space comparison must apply to element-text fields (`.`),
/// not only attributes.
const TEXT_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" type="xs:integer" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="itemKey">
      <xs:selector xpath="item"/>
      <xs:field xpath="."/>
    </xs:key>
  </xs:element>
</xs:schema>"#;

const TEXT_DUP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <item>1</item>
  <item>01</item>
</root>"#;

#[test]
fn duplicate_text_key_detected_in_value_space_dom() {
    let result = validate_dom(TEXT_DUP_XML, TEXT_XSD);
    assert!(
        !result.is_valid(),
        "DOM: element-text keys '1' and '01' are equal in the integer value space and must collide"
    );
}

#[test]
fn duplicate_text_key_detected_in_value_space_onepass() {
    let result = validate_onepass(TEXT_DUP_XML, TEXT_XSD);
    assert!(
        !result.is_valid(),
        "OnePass: element-text keys '1' and '01' are equal in the integer value space and must collide"
    );
}
