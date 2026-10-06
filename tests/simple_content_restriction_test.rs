//! Regression (#61): a complex type defined as an empty `simpleContent`
//! restriction of another simpleContent type inherits the base's value type.
//!
//! This is the shape of every GML 3.1.1 measure type (`gml:LengthType` ->
//! `gml:MeasureType` -> `xs:double`), so `<bldg:measuredHeight uom="m">tall
//! </bldg:measuredHeight>` must be rejected. Both engines used to check only
//! one derivation step and silently accepted the restricted type's text.

mod common;

use common::validate_all;

const XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" elementFormDefault="qualified">
  <xs:complexType name="MeasureType">
    <xs:simpleContent>
      <xs:extension base="xs:double">
        <xs:attribute name="uom" type="xs:string" use="required"/>
      </xs:extension>
    </xs:simpleContent>
  </xs:complexType>
  <xs:complexType name="LengthType">
    <xs:simpleContent>
      <xs:restriction base="t:MeasureType"/>
    </xs:simpleContent>
  </xs:complexType>
  <xs:complexType name="HeightType">
    <xs:simpleContent>
      <xs:restriction base="t:LengthType"/>
    </xs:simpleContent>
  </xs:complexType>
  <xs:element name="measure" type="t:MeasureType"/>
  <xs:element name="length" type="t:LengthType"/>
  <xs:element name="height" type="t:HeightType"/>
  <xs:element name="inline">
    <xs:complexType>
      <xs:simpleContent>
        <xs:restriction base="t:MeasureType"/>
      </xs:simpleContent>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

fn check(xml: &str, expect_valid: bool) {
    let (dom, streaming) = validate_all(xml, XSD);
    assert_eq!(
        dom.is_valid(),
        expect_valid,
        "DOM verdict for {xml}: {:?}",
        dom.errors()
    );
    assert_eq!(
        streaming.is_valid(),
        expect_valid,
        "streaming verdict for {xml}: {:?}",
        streaming.errors()
    );
}

#[test]
fn base_measure_type_checks_its_value() {
    check(
        r#"<t:measure xmlns:t="urn:t" uom="m">10.5</t:measure>"#,
        true,
    );
    check(
        r#"<t:measure xmlns:t="urn:t" uom="m">tall</t:measure>"#,
        false,
    );
}

#[test]
fn restriction_of_simple_content_type_checks_inherited_value_type() {
    check(r#"<t:length xmlns:t="urn:t" uom="m">10.5</t:length>"#, true);
    check(
        r#"<t:length xmlns:t="urn:t" uom="m">tall</t:length>"#,
        false,
    );
}

#[test]
fn chained_restrictions_check_inherited_value_type() {
    check(r#"<t:height xmlns:t="urn:t" uom="m">3</t:height>"#, true);
    check(
        r#"<t:height xmlns:t="urn:t" uom="m">tall</t:height>"#,
        false,
    );
}

#[test]
fn inline_restriction_checks_inherited_value_type() {
    check(r#"<t:inline xmlns:t="urn:t" uom="m">3</t:inline>"#, true);
    check(
        r#"<t:inline xmlns:t="urn:t" uom="m">tall</t:inline>"#,
        false,
    );
}
