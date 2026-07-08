//! Regression: namespace-aware base-type resolution on the ComplexExtension
//! inheritance hop (both engines' inline-type fallback path).
//!
//! Two namespaces each declare a global complex type named `Base` but with
//! DIFFERENT child element content. A no-namespace element `d` has an inline
//! (anonymous) complex type that extends the no-namespace `Base`. Anonymous
//! types are never in the compiled ns-keyed children cache, so both the DOM
//! and streaming validators fall back to `collect_elements_with_inheritance`,
//! which resolved the ComplexExtension `base_type` by its raw string
//! ("Base"). Because the merged schema's target namespace is `ns-a`, the
//! unprefixed `get_type("Base")` shim resolves to *ns-a*'s `Base` (children
//! `y:int`), not the no-namespace `Base` (children `x:string`) actually
//! referenced. The inline type inherits the wrong namespace's children and a
//! valid instance is rejected — identically in both engines.
//!
//! The correct base namespace is available on the compiled type as
//! `ComplexType.base_ns` ({"", "Base"}); the fix hops via that ns-first.
//!
//! The collision is target-namespace / merge-order dependent, so both schema
//! orderings are exercised to keep the repro deterministic.

use std::sync::Arc;

// no-namespace `Base` has child `x:string`; the inline type on `d` extends it
// and adds `extra:string`.
const SCHEMA_NO_NS: &[u8] = br###"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
    elementFormDefault="qualified">
  <xsd:complexType name="Base">
    <xsd:sequence>
      <xsd:element name="x" type="xsd:string"/>
    </xsd:sequence>
  </xsd:complexType>
  <xsd:element name="d">
    <xsd:complexType>
      <xsd:complexContent>
        <xsd:extension base="Base">
          <xsd:sequence>
            <xsd:element name="extra" type="xsd:string"/>
          </xsd:sequence>
        </xsd:extension>
      </xsd:complexContent>
    </xsd:complexType>
  </xsd:element>
</xsd:schema>"###;

// ns-a's `Base` has a DIFFERENT child `y:int`. If the no-ns inline type
// mis-resolves its base to this one, it would expect `y` and reject `x`.
const SCHEMA_A: &[u8] = br###"<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
    targetNamespace="ns-a" xmlns:a="ns-a" elementFormDefault="qualified">
  <xsd:import schemaLocation="noNs.xsd"/>
  <xsd:complexType name="Base">
    <xsd:sequence>
      <xsd:element name="y" type="xsd:int"/>
    </xsd:sequence>
  </xsd:complexType>
</xsd:schema>"###;

// Valid only against the no-namespace `Base`: inherited `x` then own `extra`.
const INSTANCE: &[u8] = br###"<d><x>hello</x><extra>world</extra></d>"###;

fn schema(a_first: bool) -> Arc<fastxml::schema::Schema> {
    let builder = fastxml::schema::Schema::builder();
    let builder = if a_first {
        builder.add("a.xsd", SCHEMA_A).add("noNs.xsd", SCHEMA_NO_NS)
    } else {
        builder.add("noNs.xsd", SCHEMA_NO_NS).add("a.xsd", SCHEMA_A)
    };
    Arc::new(builder.resolve().expect("compile schema set"))
}

fn dom_errors(a_first: bool) -> Vec<String> {
    let schema = schema(a_first);
    let doc = fastxml::Parser::from(INSTANCE).parse().expect("parse");
    fastxml::schema::Validator::from(&doc)
        .schema(schema)
        .run()
        .expect("validate")
        .into_entries()
        .iter()
        .map(|e| format!("{e:?}"))
        .collect()
}

fn streaming_errors(a_first: bool) -> Vec<String> {
    let schema = schema(a_first);
    fastxml::schema::Validator::from(INSTANCE)
        .schema(schema)
        .run()
        .expect("validate")
        .into_entries()
        .iter()
        .map(|e| format!("{e:?}"))
        .collect()
}

#[test]
fn dom_resolves_no_ns_base_a_first() {
    let errors = dom_errors(true);
    assert!(
        errors.is_empty(),
        "DOM (a first) should accept; got: {errors:?}"
    );
}

#[test]
fn dom_resolves_no_ns_base_no_ns_first() {
    let errors = dom_errors(false);
    assert!(
        errors.is_empty(),
        "DOM (no-ns first) should accept; got: {errors:?}"
    );
}

#[test]
fn streaming_resolves_no_ns_base_a_first() {
    let errors = streaming_errors(true);
    assert!(
        errors.is_empty(),
        "streaming (a first) should accept; got: {errors:?}"
    );
}

#[test]
fn streaming_resolves_no_ns_base_no_ns_first() {
    let errors = streaming_errors(false);
    assert!(
        errors.is_empty(),
        "streaming (no-ns first) should accept; got: {errors:?}"
    );
}
