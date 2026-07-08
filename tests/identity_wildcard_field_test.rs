//! Identity-constraint fields using wildcard XPath name tests.
//!
//! Two wildcard forms appear in the W3C identity-constraint suite and were
//! handled by only one engine each:
//!
//! * element namespace-wildcard field `prefix:*` — the DOM engine's XPath
//!   `QName { local: "*" }` never matched (fixed here);
//! * attribute-wildcard field `@*` / `attribute::*` — the streaming engine
//!   matched an attribute literally named `*` (fixed here).
//!
//! Both instances below are VALID (distinct key values) and must validate
//! clean on *both* engines.

mod common;

use common::validate_all;

/// `<xs:field xpath="myNS:*"/>` — element namespace wildcard selects the single
/// namespaced child of each selected node.
#[test]
fn element_namespace_wildcard_field() {
    let xsd = r#"<?xml version="1.0"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" elementFormDefault="qualified"
    targetNamespace="myNS.tempuri.org" xmlns:myNS="myNS.tempuri.org" xmlns="myNS.tempuri.org">
  <xsd:element name="root">
    <xsd:complexType>
      <xsd:sequence>
        <xsd:element ref="t" maxOccurs="unbounded"/>
      </xsd:sequence>
    </xsd:complexType>
    <xsd:key id="foo123" name="tableu">
      <xsd:selector xpath=".//myNS:t"/>
      <xsd:field xpath="myNS:*"/>
    </xsd:key>
  </xsd:element>
  <xsd:element name="t" type="ttype"/>
  <xsd:complexType name="ttype">
    <xsd:sequence>
      <xsd:element name="row" type="xsd:string"/>
    </xsd:sequence>
  </xsd:complexType>
</xsd:schema>"#;
    let xml = r#"<?xml version="1.0"?>
<myNS:root xmlns:myNS="myNS.tempuri.org">
  <myNS:t><myNS:row>1</myNS:row></myNS:t>
  <myNS:t><myNS:row>2</myNS:row></myNS:t>
  <myNS:t><myNS:row>11</myNS:row></myNS:t>
</myNS:root>"#;
    let (dom, stream) = validate_all(xml, xsd);
    assert!(
        dom.is_valid(),
        "DOM should accept element-wildcard field: {:?}",
        dom.errors()
    );
    assert!(
        stream.is_valid(),
        "streaming should accept element-wildcard field: {:?}",
        stream.errors()
    );
}

/// `<xs:field xpath="@*"/>` and `attribute::*` — attribute wildcard selects the
/// single attribute of each selected node.
#[test]
fn attribute_wildcard_field() {
    for field in ["@*", "attribute::*"] {
        let xsd = format!(
            r#"<?xml version="1.0"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" elementFormDefault="qualified"
    targetNamespace="myNS.tempuri.org" xmlns:myNS="myNS.tempuri.org" xmlns="myNS.tempuri.org">
  <xsd:element name="root">
    <xsd:complexType>
      <xsd:sequence>
        <xsd:element ref="t" maxOccurs="unbounded"/>
      </xsd:sequence>
    </xsd:complexType>
    <xsd:key id="foo123" name="tableu">
      <xsd:selector xpath=".//myNS:t/myNS:row"/>
      <xsd:field xpath="{field}"/>
    </xsd:key>
  </xsd:element>
  <xsd:element name="t" type="ttype"/>
  <xsd:complexType name="ttype">
    <xsd:sequence>
      <xsd:element name="row" maxOccurs="unbounded">
        <xsd:complexType>
          <xsd:simpleContent>
            <xsd:extension base="xsd:string">
              <xsd:attribute name="col" type="xsd:string"/>
            </xsd:extension>
          </xsd:simpleContent>
        </xsd:complexType>
      </xsd:element>
    </xsd:sequence>
  </xsd:complexType>
</xsd:schema>"#
        );
        let xml = r#"<?xml version="1.0"?>
<myNS:root xmlns:myNS="myNS.tempuri.org">
  <myNS:t><myNS:row col="1">1</myNS:row></myNS:t>
  <myNS:t><myNS:row col="2">2</myNS:row></myNS:t>
  <myNS:t><myNS:row col="11">11</myNS:row></myNS:t>
</myNS:root>"#;
        let (dom, stream) = validate_all(xml, &xsd);
        assert!(
            dom.is_valid(),
            "DOM should accept `{field}` field: {:?}",
            dom.errors()
        );
        assert!(
            stream.is_valid(),
            "streaming should accept `{field}` field: {:?}",
            stream.errors()
        );
    }
}
