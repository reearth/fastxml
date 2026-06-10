//! xs:any wildcard validation tests (DOM and streaming engines).

use std::sync::Arc;

use fastxml::schema::{Schema, Validator};

const LAX_SCHEMA: &[u8] = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="doc">
    <xs:complexType>
      <xs:sequence>
        <xs:any minOccurs="2" maxOccurs="3" processContents="lax"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

const SKIP_SCHEMA: &[u8] = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="doc">
    <xs:complexType>
      <xs:sequence>
        <xs:any processContents="skip"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:element name="known" type="xs:int"/>
</xs:schema>"#;

fn validate_both(schema: &[u8], xml: &str) -> (bool, bool) {
    let schema = Arc::new(Schema::from_xsd(schema).expect("schema"));

    let doc = fastxml::Parser::from(xml).parse().expect("parse");
    let dom_valid = Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .expect("dom validate")
        .into_entries()
        .is_empty();

    let stream_valid = Validator::from(xml.as_bytes())
        .schema(schema)
        .run()
        .expect("stream validate")
        .into_entries()
        .is_empty();

    (dom_valid, stream_valid)
}

#[test]
fn lax_wildcard_admits_undeclared_elements() {
    let (dom, stream) = validate_both(LAX_SCHEMA, "<doc><a/><b/></doc>");
    assert!(dom, "DOM should accept undeclared children under lax");
    assert!(
        stream,
        "streaming should accept undeclared children under lax"
    );
}

#[test]
fn wildcard_min_occurs_enforced() {
    let (dom, stream) = validate_both(LAX_SCHEMA, "<doc><a/></doc>");
    assert!(!dom, "DOM should reject too few wildcard children");
    assert!(!stream, "streaming should reject too few wildcard children");
}

#[test]
fn wildcard_max_occurs_enforced() {
    let (dom, stream) = validate_both(LAX_SCHEMA, "<doc><a/><b/><c/><d/></doc>");
    assert!(!dom, "DOM should reject too many wildcard children");
    assert!(
        !stream,
        "streaming should reject too many wildcard children"
    );
}

#[test]
fn skip_wildcard_skips_subtree_even_when_declared() {
    // 'known' is declared as xs:int but its content is not an int; under a
    // skip wildcard it must not be validated.
    let (dom, stream) = validate_both(SKIP_SCHEMA, "<doc><known>not-an-int</known></doc>");
    assert!(dom, "DOM must not validate inside a skip wildcard");
    assert!(stream, "streaming must not validate inside a skip wildcard");
}

#[test]
fn lax_wildcard_validates_declared_elements() {
    const LAX_ONE: &[u8] = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="doc">
    <xs:complexType>
      <xs:sequence>
        <xs:any processContents="lax"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:element name="known" type="xs:int"/>
</xs:schema>"#;
    let (dom, stream) = validate_both(LAX_ONE, "<doc><known>not-an-int</known></doc>");
    assert!(!dom, "DOM must validate declared elements under lax");
    assert!(
        !stream,
        "streaming must validate declared elements under lax"
    );
}
