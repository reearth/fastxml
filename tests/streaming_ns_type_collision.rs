//! Regression: namespace-aware type resolution on the streaming validator's
//! inline (wildcard-matched) element path.
//!
//! Two namespaces each declare a global complex type with the SAME local name
//! (`ct-A`) but DIFFERENT content models. A `root` element accepts any element
//! via a strict wildcard, so both `a:e1` (namespace `ns-a`) and `e1` (no
//! namespace) are matched inline. The streaming validator used to resolve the
//! unprefixed type reference by local name only, binding the wrong `ct-A` and
//! reporting spurious datatype errors. DOM and streaming must agree that the
//! instance is valid.

use std::sync::Arc;

const SCHEMA_A: &[u8] = br###"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
    targetNamespace="ns-a" xmlns:a="ns-a" elementFormDefault="qualified">
  <xsd:import schemaLocation="b.xsd"/>
  <xsd:complexType name="ct-A">
    <xsd:sequence>
      <xsd:element name="a1" type="xsd:int"/>
      <xsd:element name="a2" type="xsd:boolean"/>
    </xsd:sequence>
  </xsd:complexType>
  <xsd:element name="e1" type="a:ct-A"/>
  <xsd:element name="root">
    <xsd:complexType>
      <xsd:choice maxOccurs="unbounded">
        <xsd:any namespace="##any" processContents="strict"/>
      </xsd:choice>
    </xsd:complexType>
  </xsd:element>
</xsd:schema>"###;

const SCHEMA_B: &[u8] = br###"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
    elementFormDefault="qualified">
  <xsd:complexType name="ct-A">
    <xsd:sequence>
      <xsd:element name="a1" type="xsd:boolean"/>
      <xsd:element name="a2" type="xsd:int"/>
    </xsd:sequence>
  </xsd:complexType>
  <xsd:element name="e1" type="ct-A"/>
</xsd:schema>"###;

// `a:e1` uses ns-a's ct-A (a1:int, a2:boolean); the unprefixed `e1` uses the
// no-namespace ct-A (a1:boolean, a2:int). Each is valid only against its own
// namespace's type.
const INSTANCE: &[u8] = br###"<a:root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xmlns:a="ns-a">
  <a:e1><a:a1>123</a:a1><a:a2>true</a:a2></a:e1>
  <e1><a1>true</a1><a2>123</a2></e1>
</a:root>"###;

fn schema() -> Arc<fastxml::schema::Schema> {
    Arc::new(
        fastxml::schema::Schema::builder()
            .add("a.xsd", SCHEMA_A)
            .add("b.xsd", SCHEMA_B)
            .resolve()
            .expect("compile schema set"),
    )
}

#[test]
fn dom_accepts_ns_qualified_wildcard_types() {
    let schema = schema();
    let doc = fastxml::Parser::from(INSTANCE).parse().expect("parse");
    let errors = fastxml::schema::Validator::from(&doc)
        .schema(schema)
        .run()
        .expect("validate")
        .into_entries();
    assert!(errors.is_empty(), "DOM should accept; got: {errors:?}");
}

#[test]
fn streaming_accepts_ns_qualified_wildcard_types() {
    let schema = schema();
    let errors = fastxml::schema::Validator::from(INSTANCE)
        .schema(schema)
        .run()
        .expect("validate")
        .into_entries();
    assert!(
        errors.is_empty(),
        "streaming should accept; got: {errors:?}"
    );
}
