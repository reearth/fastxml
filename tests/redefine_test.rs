//! xs:redefine: redefined components replace the originals, and
//! self-references inside the redefinition point to the original.

use fastxml::Parser;
use fastxml::schema::{Schema, Validator};

const BASE_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="PersonType">
    <xs:sequence>
      <xs:element name="name" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
  <xs:simpleType name="SizeType">
    <xs:restriction base="xs:integer">
      <xs:maxInclusive value="100"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:group name="InfoGroup">
    <xs:sequence>
      <xs:element name="info" type="xs:string"/>
    </xs:sequence>
  </xs:group>
</xs:schema>"#;

/// Redefines PersonType (adds an `age` element by extending the ORIGINAL),
/// SizeType (narrows the range by restricting the ORIGINAL), and InfoGroup
/// (wraps the ORIGINAL group plus an extra element).
const REDEFINING_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:redefine schemaLocation="base.xsd">
    <xs:complexType name="PersonType">
      <xs:complexContent>
        <xs:extension base="PersonType">
          <xs:sequence>
            <xs:element name="age" type="xs:int"/>
          </xs:sequence>
        </xs:extension>
      </xs:complexContent>
    </xs:complexType>
    <xs:simpleType name="SizeType">
      <xs:restriction base="SizeType">
        <xs:maxInclusive value="10"/>
      </xs:restriction>
    </xs:simpleType>
    <xs:group name="InfoGroup">
      <xs:sequence>
        <xs:group ref="InfoGroup"/>
        <xs:element name="extra" type="xs:string"/>
      </xs:sequence>
    </xs:group>
  </xs:redefine>
  <xs:element name="person" type="PersonType"/>
  <xs:element name="size" type="SizeType"/>
  <xs:element name="record">
    <xs:complexType>
      <xs:group ref="InfoGroup"/>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

fn schema() -> Schema {
    Schema::builder()
        .add("file:///test/base.xsd", BASE_XSD.as_bytes().to_vec())
        .add("file:///test/entry.xsd", REDEFINING_XSD.as_bytes().to_vec())
        .resolve()
        .expect("schema with redefine should compile")
}

fn validate(schema: &Schema, xml: &str) -> Vec<String> {
    let doc = Parser::from(xml).parse().unwrap();
    Validator::from(&doc)
        .schema(schema.clone())
        .run()
        .unwrap()
        .errors()
        .iter()
        .map(|e| e.message.to_string())
        .collect()
}

#[test]
fn redefined_complex_type_extends_original() {
    let schema = schema();

    // The redefined PersonType = original (name) + extension (age).
    let valid = "<person><name>taro</name><age>10</age></person>";
    let errors = validate(&schema, valid);
    assert!(errors.is_empty(), "expected valid, got: {errors:?}");

    // Missing the extension's element is invalid.
    let missing_age = "<person><name>taro</name></person>";
    let errors = validate(&schema, missing_age);
    assert!(!errors.is_empty(), "expected missing-age error");

    // Missing the original's element is invalid.
    let missing_name = "<person><age>10</age></person>";
    let errors = validate(&schema, missing_name);
    assert!(!errors.is_empty(), "expected missing-name error");
}

#[test]
fn redefined_simple_type_restricts_original() {
    let schema = schema();

    assert!(validate(&schema, "<size>5</size>").is_empty());
    // 50 satisfies the ORIGINAL bound (100) but not the redefined one (10).
    assert!(
        !validate(&schema, "<size>50</size>").is_empty(),
        "redefined SizeType must enforce the narrower bound"
    );
}

#[test]
fn redefined_group_wraps_original() {
    let schema = schema();

    let valid = "<record><info>i</info><extra>e</extra></record>";
    let errors = validate(&schema, valid);
    assert!(errors.is_empty(), "expected valid, got: {errors:?}");

    let missing_extra = "<record><info>i</info></record>";
    assert!(
        !validate(&schema, missing_extra).is_empty(),
        "redefined group requires the extra element"
    );
}

#[test]
fn non_redefined_components_still_resolve() {
    // A schema that redefines one type still sees everything else from
    // the redefined document.
    let schema = schema();
    // InfoGroup's original 'info' element came from base.xsd via redefine.
    assert!(validate(&schema, "<record><info>i</info><extra>e</extra></record>").is_empty());
}
