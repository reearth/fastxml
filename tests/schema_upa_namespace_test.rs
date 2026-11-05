//! Unique Particle Attribution compares expanded names: two local element
//! declarations with the same local name in different target namespaces never
//! compete, whether they sit in one content model through type derivation or
//! not. A genuine same-namespace clash is still rejected. Validation keeps
//! them apart too: each child matches only the declaration in its own
//! namespace, for its position in the content model and for its type.

use std::sync::Arc;

use fastxml::Parser;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::{SchemaBuilder, Validator};

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
/// namespace, beside the inherited one. Its type (`xs:int`) differs from the
/// inherited `label`'s (`xs:string`), so the type applied to a child shows
/// which declaration it matched.
const DERIVED: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:base="urn:base" targetNamespace="urn:derived" elementFormDefault="qualified">
  <xs:import namespace="urn:base"/>
  <xs:element name="DerivedThing" type="DerivedType"/>
  <xs:complexType name="DerivedType">
    <xs:complexContent>
      <xs:extension base="base:BaseType">
        <xs:sequence>
          <xs:element name="label" minOccurs="0" type="xs:int"/>
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

fn derived_schema() -> Arc<CompiledSchema> {
    Arc::new(
        SchemaBuilder::new()
            .add("urn:base", BASE)
            .add("urn:derived", DERIVED)
            .resolve()
            .expect("the schema compiles"),
    )
}

/// Error messages from both engines for a `DerivedThing` with `children`.
fn validate(children: &str) -> (Vec<String>, Vec<String>) {
    let xml = format!(
        r#"<d:DerivedThing xmlns:d="urn:derived" xmlns:b="urn:base">{children}</d:DerivedThing>"#
    );
    let schema = derived_schema();
    let errors = |report: fastxml::schema::Report| -> Vec<String> {
        report
            .into_entries()
            .into_iter()
            .filter(|e| e.is_error())
            .map(|e| e.to_string())
            .collect()
    };
    let doc = Parser::from(xml.as_str()).parse().expect("well-formed");
    let dom = errors(
        Validator::from(&doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("DOM validation runs"),
    );
    let streaming = errors(
        Validator::from(xml.as_str())
            .schema(schema)
            .run()
            .expect("streaming validation runs"),
    );
    (dom, streaming)
}

fn assert_valid(children: &str) {
    let (dom, streaming) = validate(children);
    assert!(dom.is_empty(), "DOM rejected {children}: {dom:?}");
    assert!(
        streaming.is_empty(),
        "streaming rejected {children}: {streaming:?}"
    );
}

fn assert_invalid(children: &str) {
    let (dom, streaming) = validate(children);
    assert!(!dom.is_empty(), "DOM accepted {children}");
    assert!(!streaming.is_empty(), "streaming accepted {children}");
}

#[test]
fn each_label_takes_the_type_of_its_own_namespace() {
    assert_valid("<b:label>text</b:label>");
    assert_valid("<b:label>text</b:label><d:label>1</d:label>");
    assert_invalid("<d:label>text</d:label>");
}

#[test]
fn each_label_keeps_its_own_position() {
    assert_valid("<b:label>a</b:label><b:other>b</b:other><d:label>1</d:label>");
    assert_invalid("<b:label>a</b:label><b:label>b</b:label>");
    assert_invalid("<d:label>1</d:label><b:other>b</b:other>");
    assert_invalid("<b:other>b</b:other><b:label>a</b:label>");
}
