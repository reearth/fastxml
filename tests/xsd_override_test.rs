//! `xs:override` (XSD 1.1) is recorded in the AST and rejected by the
//! compiler instead of being silently ignored.
//!
//! Before, the parser fell through for `override`, so its child definitions
//! were read as if declared at the top level of the overriding document and
//! the overridden document was never loaded: a schema compiled with a
//! component set that matched neither XSD 1.0 nor XSD 1.1.

use fastxml::schema::Schema;
use fastxml::schema::xsd::parse_xsd_ast;

const OVERRIDING: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns:t="urn:t">
  <xs:override schemaLocation="base.xsd">
    <xs:annotation><xs:documentation>replaces T and e</xs:documentation></xs:annotation>
    <xs:simpleType name="T"><xs:restriction base="xs:int"/></xs:simpleType>
    <xs:complexType name="C"><xs:sequence><xs:element name="x" type="t:T"/></xs:sequence></xs:complexType>
    <xs:element name="e" type="t:T"/>
    <xs:attribute name="a" type="t:T"/>
    <xs:group name="G"><xs:sequence><xs:element name="y" type="xs:string"/></xs:sequence></xs:group>
    <xs:attributeGroup name="AG"><xs:attribute name="b" type="xs:string"/></xs:attributeGroup>
  </xs:override>
  <xs:element name="own" type="xs:string"/>
</xs:schema>"#;

const BASE: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t">
  <xs:simpleType name="T"><xs:restriction base="xs:string"/></xs:simpleType>
  <xs:element name="e" type="xs:string"/>
</xs:schema>"#;

#[test]
fn override_is_recorded_in_the_ast() {
    let ast = parse_xsd_ast(OVERRIDING.as_bytes()).expect("parse");
    assert_eq!(ast.overrides.len(), 1);
    let over = &ast.overrides[0];
    assert_eq!(over.schema_location, "base.xsd");
    assert_eq!(over.simple_types.len(), 1);
    assert_eq!(over.complex_types.len(), 1);
    assert_eq!(over.elements.len(), 1);
    assert_eq!(over.attributes.len(), 1);
    assert_eq!(over.groups.len(), 1);
    assert_eq!(over.attribute_groups.len(), 1);
}

#[test]
fn override_children_are_not_top_level_definitions() {
    let ast = parse_xsd_ast(OVERRIDING.as_bytes()).expect("parse");
    let elements: Vec<&str> = ast.elements.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(elements, ["own"]);
    assert!(ast.types.is_empty());
    assert!(ast.attributes.is_empty());
    assert!(ast.groups.is_empty());
    assert!(ast.attribute_groups.is_empty());
}

#[test]
fn compiling_a_schema_with_override_is_an_error() {
    let err = Schema::builder()
        .add("main.xsd", OVERRIDING.as_bytes().to_vec())
        .add("base.xsd", BASE.as_bytes().to_vec())
        .resolve()
        .expect_err("xs:override is not supported");
    let message = err.to_string();
    assert!(message.contains("override"), "{message}");
    assert!(message.contains("base.xsd"), "{message}");
}
