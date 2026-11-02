//! Unique Particle Attribution compares expanded names: two local element
//! declarations with the same local name in different target namespaces never
//! compete, whether they sit in one content model through type derivation or
//! not. A genuine same-namespace clash is still rejected.

use fastxml::schema::SchemaBuilder;

const BASE: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:base" elementFormDefault="qualified">
  <xs:element name="Thing" type="BaseType"/>
  <xs:complexType name="BaseType">
    <xs:sequence>
      <xs:element name="label" minOccurs="0" type="xs:string"/>
      <xs:element name="other" minOccurs="0" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>"#;

/// Extends `base:BaseType` and declares its own `label` in another
/// namespace, beside the inherited one.
const DERIVED: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:base="urn:base" targetNamespace="urn:derived" elementFormDefault="qualified">
  <xs:import namespace="urn:base"/>
  <xs:element name="DerivedThing" type="DerivedType"/>
  <xs:complexType name="DerivedType">
    <xs:complexContent>
      <xs:extension base="base:BaseType">
        <xs:sequence>
          <xs:element name="label" minOccurs="0" type="xs:string"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>"#;

const SAME_NAMESPACE_CLASH: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:clash" elementFormDefault="qualified">
  <xs:element name="Clash" type="ClashType"/>
  <xs:complexType name="ClashType">
    <xs:sequence>
      <xs:element name="a" minOccurs="0" type="xs:string"/>
      <xs:element name="a" minOccurs="0" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>"#;

#[test]
fn same_local_name_in_different_namespaces_through_extension_is_not_a_upa_clash() {
    let compiled = SchemaBuilder::new()
        .add("urn:base", BASE)
        .add("urn:derived", DERIVED)
        .resolve();
    assert!(
        compiled.is_ok(),
        "a valid schema was rejected: {:?}",
        compiled.err()
    );
}

#[test]
fn same_local_name_in_the_same_namespace_is_still_a_upa_clash() {
    let compiled = SchemaBuilder::new()
        .add("urn:clash", SAME_NAMESPACE_CLASH)
        .resolve();
    let err = compiled.expect_err("a real UPA violation must be rejected");
    assert!(
        err.to_string().contains("Unique Particle Attribution"),
        "unexpected error: {err}"
    );
}
