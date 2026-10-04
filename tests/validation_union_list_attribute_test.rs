//! Union member types, list item-type facets, attribute namespaces and
//! prohibited attributes, checked through both engines.

use std::sync::Arc;

use fastxml::schema::{Schema, Validator};
use fastxml::{Parser, StructuredError};

/// Validates `xml` against `xsd` with both engines and returns the
/// error-level entries of each, as `(dom, streaming)`.
fn run_both(xsd: &str, xml: &str) -> (Vec<StructuredError>, Vec<StructuredError>) {
    let schema = Arc::new(Schema::from_xsd(xsd).expect("schema compiles"));
    let doc = Parser::from(xml).parse().expect("instance parses");
    let dom = Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .expect("dom validation runs");
    let streaming = Validator::from(xml)
        .schema(Arc::clone(&schema))
        .run()
        .expect("streaming validation runs");
    let errors = |r: fastxml::schema::Report| -> Vec<StructuredError> {
        r.into_entries()
            .into_iter()
            .filter(|e| e.is_error())
            .collect()
    };
    (errors(dom), errors(streaming))
}

fn messages(errors: &[StructuredError]) -> Vec<String> {
    errors.iter().map(|e| e.message.to_string()).collect()
}

fn assert_valid(xsd: &str, xml: &str) {
    let (dom, streaming) = run_both(xsd, xml);
    assert!(
        dom.is_empty(),
        "DOM: expected valid, got {:?}",
        messages(&dom)
    );
    assert!(
        streaming.is_empty(),
        "streaming: expected valid, got {:?}",
        messages(&streaming)
    );
}

fn assert_invalid(xsd: &str, xml: &str, needle: &str) {
    let (dom, streaming) = run_both(xsd, xml);
    for (engine, errors) in [("DOM", &dom), ("streaming", &streaming)] {
        assert!(
            errors.iter().any(|e| e.message.contains(needle)),
            "{engine}: expected an error containing {needle:?}, got {:?}",
            messages(errors)
        );
    }
}

// ---------------------------------------------------------------------------
// Unions
// ---------------------------------------------------------------------------

const UNION: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:simpleType name="U"><xs:union memberTypes="xs:int xs:boolean"/></xs:simpleType>
<xs:simpleType name="Small"><xs:restriction base="xs:int"><xs:maxInclusive value="5"/></xs:restriction></xs:simpleType>
<xs:simpleType name="UF"><xs:union memberTypes="Small"><xs:simpleType><xs:restriction base="xs:string"><xs:enumeration value="none"/></xs:restriction></xs:simpleType></xs:union></xs:simpleType>
<xs:simpleType name="UR"><xs:restriction base="U"><xs:pattern value="[0-9]+"/></xs:restriction></xs:simpleType>
<xs:element name="r"><xs:complexType><xs:sequence>
 <xs:element name="u" type="U" minOccurs="0" maxOccurs="unbounded"/>
 <xs:element name="uf" type="UF" minOccurs="0" maxOccurs="unbounded"/>
 <xs:element name="ur" type="UR" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence><xs:attribute name="a" type="U"/></xs:complexType></xs:element>
</xs:schema>"#;

#[test]
fn union_value_must_match_a_member_type() {
    assert_valid(UNION, "<r><u>1</u><u>true</u><u> 42 </u></r>");
    assert_invalid(UNION, "<r><u>hello</u></r>", "union");
    assert_invalid(UNION, r#"<r a="hello"/>"#, "union");
    assert_valid(UNION, r#"<r a="false"/>"#);
}

#[test]
fn union_member_facets_and_anonymous_members_apply() {
    assert_valid(UNION, "<r><uf>3</uf><uf>none</uf></r>");
    assert_invalid(UNION, "<r><uf>9</uf></r>", "union");
    assert_invalid(UNION, "<r><uf>other</uf></r>", "union");
}

#[test]
fn restriction_of_union_keeps_member_check() {
    assert_valid(UNION, "<r><ur>12</ur></r>");
    assert_invalid(UNION, "<r><ur>99999999999</ur></r>", "union");
}

// ---------------------------------------------------------------------------
// List item-type facets
// ---------------------------------------------------------------------------

const LIST: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:simpleType name="small"><xs:restriction base="xs:double"><xs:maxInclusive value="10"/></xs:restriction></xs:simpleType>
<xs:simpleType name="smallList"><xs:list itemType="small"/></xs:simpleType>
<xs:simpleType name="dl"><xs:list itemType="xs:double"/></xs:simpleType>
<xs:simpleType name="codeList"><xs:list><xs:simpleType><xs:restriction base="xs:string"><xs:pattern value="[A-Z]{2}"/></xs:restriction></xs:simpleType></xs:list></xs:simpleType>
<xs:element name="r"><xs:complexType><xs:sequence>
 <xs:element name="s" type="smallList" minOccurs="0" maxOccurs="unbounded"/>
 <xs:element name="d" type="dl" minOccurs="0" maxOccurs="unbounded"/>
 <xs:element name="c" type="codeList" minOccurs="0" maxOccurs="unbounded"/>
</xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;

#[test]
fn list_item_facets_are_enforced() {
    assert_valid(LIST, "<r><s>1 2 10</s><d>1e300 2</d><c>AB CD</c></r>");
    assert_invalid(LIST, "<r><s>1 2 11</s></r>", "list item '11'");
    assert_invalid(LIST, "<r><c>AB c</c></r>", "list item 'c'");
    assert_invalid(LIST, "<r><d>1 x</d></r>", "list item 'x'");
}

// ---------------------------------------------------------------------------
// Attribute namespaces and prohibited attributes
// ---------------------------------------------------------------------------

const ATTRS: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" elementFormDefault="qualified">
<xs:attribute name="g" type="xs:int"/>
<xs:complexType name="B"><xs:attribute name="a" type="xs:int"/><xs:attribute name="req" type="xs:string" use="required"/><xs:attribute ref="t:g"/></xs:complexType>
<xs:complexType name="R"><xs:complexContent><xs:restriction base="t:B"><xs:attribute name="a" use="prohibited"/><xs:attribute name="req" type="xs:string" use="required"/></xs:restriction></xs:complexContent></xs:complexType>
<xs:complexType name="Q"><xs:attribute name="q" type="xs:int" form="qualified"/></xs:complexType>
<xs:element name="b" type="t:B"/>
<xs:element name="r" type="t:R"/>
<xs:element name="q" type="t:Q"/>
</xs:schema>"#;

#[test]
fn unqualified_attribute_in_another_namespace_is_not_the_declared_one() {
    assert_valid(ATTRS, r#"<t:b xmlns:t="urn:t" req="x" a="1" t:g="2"/>"#);
    assert_invalid(
        ATTRS,
        r#"<t:b xmlns:t="urn:t" xmlns:p="urn:p" p:req="x"/>"#,
        "required attribute 'req' is missing",
    );
    assert_invalid(
        ATTRS,
        r#"<t:b xmlns:t="urn:t" xmlns:p="urn:p" req="x" p:a="notint"/>"#,
        "a' is not allowed", // DOM reports the local name, streaming the qualified one
    );
}

#[test]
fn referenced_and_qualified_attributes_must_be_namespaced() {
    assert_invalid(
        ATTRS,
        r#"<t:b xmlns:t="urn:t" req="x" g="2"/>"#,
        "attribute 'g' is not allowed",
    );
    assert_valid(ATTRS, r#"<t:q xmlns:t="urn:t" t:q="1"/>"#);
    assert_invalid(
        ATTRS,
        r#"<t:q xmlns:t="urn:t" q="1"/>"#,
        "attribute 'q' is not allowed",
    );
}

#[test]
fn prohibited_attribute_is_removed_by_restriction() {
    assert_valid(ATTRS, r#"<t:r xmlns:t="urn:t" req="x"/>"#);
    assert_invalid(
        ATTRS,
        r#"<t:r xmlns:t="urn:t" req="x" a="1"/>"#,
        "attribute 'a' is not allowed",
    );
}

#[test]
fn prohibited_use_inside_attribute_group_has_no_effect() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:complexType name="base"><xs:attribute name="a"/></xs:complexType>
<xs:complexType name="derived"><xs:complexContent><xs:restriction base="base"><xs:attributeGroup ref="g"/></xs:restriction></xs:complexContent></xs:complexType>
<xs:attributeGroup name="g"><xs:attribute name="a" use="prohibited"/></xs:attributeGroup>
<xs:element name="doc" type="derived"/>
</xs:schema>"#;
    assert_valid(xsd, r#"<doc a="a"/>"#);
}

#[test]
fn same_local_name_attributes_in_different_namespaces_are_distinct() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" attributeFormDefault="qualified">
<xs:attribute name="id" type="xs:int"/>
<xs:element name="e"><xs:complexType>
  <xs:attribute name="code" type="xs:string"/>
  <xs:attribute name="id" type="xs:string" form="unqualified"/>
  <xs:attribute ref="t:id"/>
</xs:complexType></xs:element>
</xs:schema>"#;
    // The DOM keeps only one attribute per local name, so the two `id`
    // attributes are checked through the streaming engine only.
    let schema = Arc::new(Schema::from_xsd(xsd).expect("schema compiles"));
    let run = |xml: &str| {
        Validator::from(xml)
            .schema(Arc::clone(&schema))
            .run()
            .expect("streaming validation runs")
    };
    let ok = run(r#"<t:e xmlns:t="urn:t" id="abc" t:id="1" t:code="x"/>"#);
    assert!(ok.is_valid(), "{:?}", messages(ok.entries()));
    let bad = run(r#"<t:e xmlns:t="urn:t" id="1" t:id="abc"/>"#);
    assert!(
        bad.entries().iter().any(|e| e.message.contains("xs:int")),
        "{:?}",
        messages(bad.entries())
    );
}

#[test]
fn chameleon_included_attribute_reference_adopts_the_includer_namespace() {
    let dir = std::env::temp_dir().join(format!("fastxml-chameleon-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.xsd");
    std::fs::write(
        &main,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a" xmlns="urn:a"><xs:include schemaLocation="inc.xsd"/></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("inc.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="doc"><xs:complexType><xs:attribute ref="att"/></xs:complexType></xs:element>
<xs:attribute name="att" type="xs:int"/>
</xs:schema>"#,
    )
    .unwrap();
    let schema = Arc::new(
        Schema::builder()
            .add(
                format!("file://{}", main.display()),
                std::fs::read(&main).unwrap(),
            )
            .resolve_with(&fastxml::schema::FileFetcher::new())
            .expect("schema resolves"),
    );
    let xml = r#"<a:doc xmlns:a="urn:a" a:att="1"/>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let dom = Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .unwrap();
    let streaming = Validator::from(xml).schema(schema).run().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(dom.is_valid(), "DOM: {:?}", messages(dom.entries()));
    assert!(
        streaming.is_valid(),
        "streaming: {:?}",
        messages(streaming.entries())
    );
}

#[test]
fn xml_namespace_attribute_reference_is_matched() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:import namespace="http://www.w3.org/XML/1998/namespace"/>
<xs:element name="root"><xs:complexType><xs:attribute ref="xml:lang" use="required"/></xs:complexType></xs:element>
</xs:schema>"#;
    assert_valid(xsd, r#"<root xml:lang="en"/>"#);
    assert_invalid(xsd, "<root/>", "required attribute 'lang' is missing");
}
