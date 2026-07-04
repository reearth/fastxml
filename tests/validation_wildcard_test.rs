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

/// Regression for wildG031: two schema documents declare the same local name
/// (`foo`) in different target namespaces — one with a target namespace and a
/// lax wildcard content model, one with no namespace and no type (anyType).
/// The instance's root element and a lax-wildcard-matched child both use the
/// local name `foo` in different namespaces. Element resolution and the
/// multi-document merge must be deterministic so the verdict does not flip
/// between runs (previously it did, because schemas were merged in HashMap
/// iteration order). The W3C-expected verdict is `valid`.
#[test]
fn wildcard_same_local_name_cross_namespace_is_deterministically_valid() {
    const NS_SCHEMA: &[u8] = br#"<?xml version="1.0"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="http://foobar">
  <xsd:element name="foo">
    <xsd:complexType>
      <xsd:sequence>
        <xsd:any namespace="A B C D E ##targetNamespace ##local" maxOccurs="unbounded" processContents="lax"/>
      </xsd:sequence>
    </xsd:complexType>
  </xsd:element>
  <xsd:element name="bar" type="xsd:string"/>
</xsd:schema>"#;
    const NO_NS_SCHEMA: &[u8] = br#"<?xml version="1.0"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <xsd:element name="foo"/>
  <xsd:element name="bar" type="xsd:string"/>
</xsd:schema>"#;
    const INSTANCE: &str = r#"<?xml version="1.0"?>
<a:foo xmlns:a="http://foobar">
  <a:bar>foo bar</a:bar>
  <bar>foo bar</bar>
  <foo/>
  <A:foo xmlns:A="A"/>
  <A:foo xmlns:A="B"/>
  <foo xmlns="C"/>
  <D:foo xmlns:D="D"/>
  <e:foo xmlns:e="E"/>
</a:foo>"#;

    // Build the combined schema many times: an unstable merge order would make
    // some of these builds pick the wrong `foo` declaration and report errors.
    for _ in 0..8 {
        let schema = Arc::new(
            Schema::builder()
                .add("ns.xsd", NS_SCHEMA)
                .add("nons.xsd", NO_NS_SCHEMA)
                .resolve()
                .expect("resolve combined schema"),
        );

        let doc = fastxml::Parser::from(INSTANCE).parse().expect("parse");
        let dom = Validator::from(&doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("dom validate")
            .into_entries();
        let stream = Validator::from(INSTANCE.as_bytes())
            .schema(Arc::clone(&schema))
            .run()
            .expect("stream validate")
            .into_entries();

        assert!(dom.is_empty(), "DOM should be valid, got: {dom:?}");
        assert!(
            stream.is_empty(),
            "streaming should be valid, got: {stream:?}"
        );
    }
}
