//! Streaming identity constraints resolve prefixed element name tests by
//! namespace, as the DOM engine does (#63).
//!
//! Each case runs both engines and requires them to agree with the expected
//! verdict. Unprefixed steps and attribute steps keep matching by local name
//! in both engines.

use std::sync::Arc;

use fastxml::schema::{Schema, Validator};

/// A `urn:t` schema whose `root` admits `t:item` children (each with an `id`
/// attribute and an optional `t:code` child) plus foreign-namespace elements,
/// and declares `constraint` (an `xs:unique` / `xs:key` element, attributes
/// included) on `root`.
fn schema(constraint: &str) -> Arc<Schema> {
    let xsd = format!(
        r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns:t="urn:t" elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" minOccurs="0" maxOccurs="unbounded">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="code" type="xs:string" minOccurs="0"/>
              <xs:any namespace="##other" processContents="skip" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
            <xs:attribute name="id" type="xs:string"/>
          </xs:complexType>
        </xs:element>
        <xs:any namespace="##other" processContents="skip" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
    {constraint}
  </xs:element>
</xs:schema>"###
    );
    Arc::new(Schema::from_xsd(xsd.as_bytes()).expect("compile"))
}

/// Asserts both engines return `expect_valid` for `xml`.
fn check(schema: &Arc<Schema>, xml: &str, expect_valid: bool) {
    let doc = fastxml::Parser::from(xml).parse().expect("parse");
    let dom = Validator::from(&doc)
        .schema(Arc::clone(schema))
        .run()
        .expect("validate");
    let streaming = Validator::from(xml)
        .schema(Arc::clone(schema))
        .run()
        .expect("validate");
    assert_eq!(
        dom.is_valid(),
        expect_valid,
        "DOM on {xml}: {:?}",
        dom.errors()
    );
    assert_eq!(
        streaming.is_valid(),
        expect_valid,
        "streaming on {xml}: {:?}",
        streaming.errors()
    );
}

fn unique(selector: &str, field: &str) -> String {
    format!(
        r#"<xs:unique name="k"><xs:selector xpath="{selector}"/><xs:field xpath="{field}"/></xs:unique>"#
    )
}

/// Two items with the same id, the second in a foreign namespace.
const FOREIGN_DUP: &str =
    r#"<t:root xmlns:t="urn:t" xmlns:o="urn:o"><t:item id="a"/><o:item id="a"/></t:root>"#;
/// Two `t:item`s with the same id.
const REAL_DUP: &str = r#"<t:root xmlns:t="urn:t"><t:item id="a"/><t:item id="a"/></t:root>"#;

#[test]
fn prefixed_selector_does_not_select_a_foreign_element() {
    let s = schema(&unique("t:item", "@id"));
    check(&s, FOREIGN_DUP, true);
    check(&s, REAL_DUP, false);
}

#[test]
fn prefixed_wildcard_selector_is_limited_to_its_namespace() {
    let s = schema(&unique("t:*", "@id"));
    check(&s, FOREIGN_DUP, true);
    check(&s, REAL_DUP, false);
}

#[test]
fn descendant_selector_is_limited_to_its_namespace() {
    let s = schema(&unique(".//t:item", "@id"));
    check(&s, FOREIGN_DUP, true);
    check(&s, REAL_DUP, false);
}

#[test]
fn instance_prefix_spelling_does_not_matter() {
    let s = schema(&unique("t:item", "@id"));
    check(
        &s,
        r#"<x:root xmlns:x="urn:t"><x:item id="a"/><item xmlns="urn:t" id="a"/></x:root>"#,
        false,
    );
}

#[test]
fn prefix_declared_on_the_selector_element() {
    // `q` is bound only on xs:selector, not on xs:schema.
    let s = schema(
        r#"<xs:unique name="k"><xs:selector xpath="q:item" xmlns:q="urn:t"/><xs:field xpath="@id"/></xs:unique>"#,
    );
    check(&s, FOREIGN_DUP, true);
    check(&s, REAL_DUP, false);
}

#[test]
fn prefixed_field_element_step_ignores_a_foreign_child() {
    // The second item has no t:code, only a foreign o:code with the same
    // value: its field is absent, so there is no duplicate.
    let s = schema(&unique("t:item", "t:code"));
    check(
        &s,
        r#"<t:root xmlns:t="urn:t" xmlns:o="urn:o"><t:item><t:code>a</t:code></t:item><t:item><o:code>a</o:code></t:item></t:root>"#,
        true,
    );
    check(
        &s,
        r#"<t:root xmlns:t="urn:t"><t:item><t:code>a</t:code></t:item><t:item><t:code>a</t:code></t:item></t:root>"#,
        false,
    );
}

#[test]
fn unprefixed_steps_still_match_by_local_name() {
    // Unchanged in both engines: the DOM XPath engine matches unprefixed
    // name tests by local name.
    let s = schema(&unique("item", "@id"));
    check(&s, REAL_DUP, false);
}
