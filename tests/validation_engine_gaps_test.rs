//! Instance-validation cases where the DOM and/or streaming engine used to
//! accept invalid documents (or reject valid ones). Every case runs through
//! both engines via the public `Validator` API and asserts the same verdict.

use std::sync::Arc;

use fastxml::schema::{Schema, Validator};
use fastxml::{Parser, StructuredError, ValidationErrorType};

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

/// Asserts both engines report the instance valid.
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

/// Asserts both engines report the instance invalid, with at least one error
/// message containing `needle` in each engine.
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
// Undeclared root element against a schema with no global elements
// ---------------------------------------------------------------------------

const TYPES_ONLY: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:complexType name="T"><xs:sequence><xs:element name="a" type="xs:int"/></xs:sequence></xs:complexType>
</xs:schema>"#;

#[test]
fn types_only_schema_rejects_undeclared_root() {
    let (dom, streaming) = run_both(TYPES_ONLY, "<anything><a>notint</a></anything>");
    for (engine, errors) in [("DOM", &dom), ("streaming", &streaming)] {
        assert!(
            errors
                .iter()
                .any(|e| e.error_type == ValidationErrorType::UnknownElement
                    && e.message.contains("'anything'")),
            "{engine}: expected UnknownElement for the root, got {:?}",
            messages(errors)
        );
    }
}

// ---------------------------------------------------------------------------
// nillable without xsi:nil
// ---------------------------------------------------------------------------

const NILLABLE: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="ni" type="xs:int" nillable="true"/>
<xs:element name="np" nillable="true"><xs:simpleType><xs:restriction base="xs:string"><xs:minLength value="1"/></xs:restriction></xs:simpleType></xs:element>
</xs:schema>"#;

#[test]
fn nillable_without_xsi_nil_still_validates_empty_content() {
    assert_invalid(NILLABLE, "<ni/>", "xs:int");
    assert_invalid(NILLABLE, "<np/>", "less than minimum");
}

#[test]
fn nilled_element_skips_value_validation() {
    assert_valid(
        NILLABLE,
        r#"<ni xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#,
    );
    assert_valid(
        NILLABLE,
        r#"<np xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#,
    );
}

// ---------------------------------------------------------------------------
// xsi:type on an element whose declared type is anonymous
// ---------------------------------------------------------------------------

const XSI_ANON: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:complexType name="Loose"><xs:sequence><xs:any processContents="skip" minOccurs="0" maxOccurs="unbounded"/></xs:sequence></xs:complexType>
<xs:element name="strict"><xs:complexType><xs:sequence><xs:element name="n" type="xs:int"/></xs:sequence></xs:complexType></xs:element>
<xs:element name="si"><xs:simpleType><xs:restriction base="xs:int"><xs:maxInclusive value="5"/></xs:restriction></xs:simpleType></xs:element>
<xs:element name="free"/>
</xs:schema>"#;

#[test]
fn xsi_type_cannot_replace_anonymous_complex_type() {
    assert_invalid(
        XSI_ANON,
        r#"<strict xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Loose"><junk/><n>notint</n></strict>"#,
        "is not derived from",
    );
}

#[test]
fn xsi_type_cannot_replace_anonymous_simple_type() {
    assert_invalid(
        XSI_ANON,
        r#"<si xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xs="http://www.w3.org/2001/XMLSchema" xsi:type="xs:string">hello</si>"#,
        "is not derived from",
    );
}

#[test]
fn xsi_type_on_untyped_element_is_allowed() {
    // A declaration without a type is xs:anyType, from which every type derives.
    assert_valid(
        XSI_ANON,
        r#"<free xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Loose"><junk/></free>"#,
    );
}

// ---------------------------------------------------------------------------
// Content-model automata for anonymous / xsi:type-substituted types
// ---------------------------------------------------------------------------

const CONTENT: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:complexType name="Named"><xs:sequence><xs:choice><xs:element name="a"/><xs:element name="b"/></xs:choice><xs:element name="c"/></xs:sequence></xs:complexType>
<xs:element name="named" type="Named"/>
<xs:element name="anon"><xs:complexType><xs:sequence><xs:choice><xs:element name="a"/><xs:element name="b"/></xs:choice><xs:element name="c"/></xs:sequence></xs:complexType></xs:element>
<xs:element name="outer"><xs:complexType><xs:sequence>
  <xs:element name="inner"><xs:complexType><xs:choice><xs:element name="p"/><xs:element name="q"/></xs:choice></xs:complexType></xs:element>
</xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;

#[test]
fn anonymous_type_choice_violation_is_reported() {
    assert_invalid(CONTENT, "<named><a/><b/><c/></named>", "not expected here");
    assert_invalid(CONTENT, "<anon><a/><b/><c/></anon>", "not expected here");
    assert_valid(CONTENT, "<anon><b/><c/></anon>");
}

#[test]
fn local_anonymous_type_choice_violation_is_reported() {
    assert_invalid(
        CONTENT,
        "<outer><inner><p/><q/></inner></outer>",
        "not expected here",
    );
    assert_valid(CONTENT, "<outer><inner><q/></inner></outer>");
}

const XSI_DERIVED: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:complexType name="Base"><xs:sequence><xs:element name="x" type="xs:int"/></xs:sequence></xs:complexType>
<xs:complexType name="Der"><xs:complexContent><xs:extension base="Base"><xs:sequence><xs:element name="y" type="xs:int"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
<xs:element name="b" type="Base"/>
</xs:schema>"#;

#[test]
fn xsi_type_substituted_type_uses_its_automaton() {
    assert_invalid(
        XSI_DERIVED,
        r#"<b xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Der"><x>1</x><y>2</y><x>3</x></b>"#,
        "not expected here",
    );
    assert_valid(
        XSI_DERIVED,
        r#"<b xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Der"><x>1</x><y>2</y></b>"#,
    );
}

// ---------------------------------------------------------------------------
// Children of anonymous local elements (streaming used the global table)
// ---------------------------------------------------------------------------

const NESTED: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="r"><xs:complexType><xs:sequence>
  <xs:element name="i"><xs:complexType><xs:sequence><xs:element name="w"><xs:complexType><xs:sequence><xs:element name="v" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
 </xs:sequence></xs:complexType></xs:element>
<xs:element name="r2"><xs:complexType><xs:sequence>
  <xs:element name="w"><xs:complexType><xs:sequence><xs:element name="v" type="xs:int"/></xs:sequence></xs:complexType></xs:element>
 </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;

#[test]
fn child_of_anonymous_local_element_is_validated() {
    assert_invalid(NESTED, "<r2><w><v>x</v></w></r2>", "xs:int");
}

#[test]
fn deeply_nested_anonymous_elements_are_resolved() {
    assert_valid(NESTED, "<r><i><w><v>1</v></w></i></r>");
    assert_invalid(NESTED, "<r><i><w><v>x</v></w></i></r>", "xs:int");
}

const SHADOW: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="w"><xs:complexType><xs:sequence><xs:element name="v" type="xs:string"/></xs:sequence></xs:complexType></xs:element>
<xs:element name="r"><xs:complexType><xs:sequence>
  <xs:element name="w"><xs:complexType><xs:sequence><xs:element name="v" type="xs:int"/></xs:sequence></xs:complexType></xs:element>
 </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;

#[test]
fn local_declaration_shadows_same_named_global() {
    assert_invalid(SHADOW, "<r><w><v>notint</v></w></r>", "xs:int");
    assert_valid(SHADOW, "<w><v>notint</v></w>");
}

// ---------------------------------------------------------------------------
// Elements in a namespace the declaration does not belong to
// ---------------------------------------------------------------------------

const NO_NS: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="age" type="xs:int"/>
</xs:schema>"#;

#[test]
fn element_in_foreign_namespace_is_not_matched_by_local_name() {
    assert_invalid(
        NO_NS,
        r#"<x:age xmlns:x="urn:other">5</x:age>"#,
        "is not declared",
    );
    let (dom, streaming) = run_both(NO_NS, r#"<age xmlns="urn:other">notanint</age>"#);
    for (engine, errors) in [("DOM", &dom), ("streaming", &streaming)] {
        assert!(
            errors
                .iter()
                .all(|e| e.error_type == ValidationErrorType::UnknownElement),
            "{engine}: expected only UnknownElement, got {:?}",
            messages(errors)
        );
        assert!(!errors.is_empty(), "{engine}: expected an error");
    }
}

#[test]
fn prefix_spelling_differences_still_resolve_by_namespace() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" elementFormDefault="qualified">
<xs:element name="age" type="xs:int"/>
</xs:schema>"#;
    assert_valid(xsd, r#"<other:age xmlns:other="urn:t">5</other:age>"#);
    assert_invalid(
        xsd,
        r#"<other:age xmlns:other="urn:t">x</other:age>"#,
        "xs:int",
    );
}

// ---------------------------------------------------------------------------
// Attributes on simple-typed elements; strict anyAttribute namespaces
// ---------------------------------------------------------------------------

const ATTRS: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="s" type="xs:int"/>
<xs:element name="w"><xs:complexType><xs:anyAttribute namespace="##other" processContents="strict"/></xs:complexType></xs:element>
<xs:attribute name="g" type="xs:int"/>
</xs:schema>"###;

#[test]
fn attribute_on_simple_typed_element_is_rejected() {
    assert_invalid(
        ATTRS,
        r#"<s foo="bar">1</s>"#,
        "attribute 'foo' is not allowed",
    );
    assert_valid(
        ATTRS,
        r#"<s xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="xs:int" xmlns:xs="http://www.w3.org/2001/XMLSchema">1</s>"#,
    );
}

#[test]
fn strict_attribute_wildcard_does_not_bind_across_namespaces() {
    assert_invalid(
        ATTRS,
        r#"<w xmlns:p="urn:p" p:g="notint"/>"#,
        "matched a strict wildcard but is not declared",
    );
}

// ---------------------------------------------------------------------------
// Identity constraints: descendant fields and empty-string values
// ---------------------------------------------------------------------------

const IDENTITY: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="r"><xs:complexType><xs:sequence>
  <xs:element name="i" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="w" minOccurs="0"><xs:complexType><xs:sequence><xs:element name="v" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element>
 </xs:sequence></xs:complexType>
 <xs:key name="kDeep"><xs:selector xpath="i"/><xs:field xpath=".//v"/></xs:key>
</xs:element>
<xs:element name="s"><xs:complexType><xs:sequence>
  <xs:element name="k" type="xs:string" maxOccurs="unbounded"/>
 </xs:sequence></xs:complexType>
 <xs:key name="kSelf"><xs:selector xpath="./k"/><xs:field xpath="."/></xs:key>
</xs:element>
<xs:element name="u"><xs:complexType><xs:sequence>
  <xs:element name="k" maxOccurs="unbounded"><xs:complexType><xs:attribute name="a" type="xs:string"/><xs:attribute name="b" type="xs:string"/></xs:complexType></xs:element>
 </xs:sequence></xs:complexType>
 <xs:unique name="uAlt"><xs:selector xpath="k"/><xs:field xpath="@a | @b"/></xs:unique>
</xs:element>
</xs:schema>"#;

#[test]
fn descendant_field_is_enforced() {
    assert_invalid(IDENTITY, "<r><i><w/></i></r>", "kDeep");
    assert_invalid(
        IDENTITY,
        "<r><i><w><v>1</v></w></i><i><w><v>1</v></w></i></r>",
        "kDeep",
    );
    assert_valid(
        IDENTITY,
        "<r><i><w><v>1</v></w></i><i><w><v>2</v></w></i></r>",
    );
}

#[test]
fn empty_string_field_is_a_value_not_null() {
    assert_valid(IDENTITY, "<s><k></k><k>a</k></s>");
    assert_invalid(IDENTITY, "<s><k></k><k></k></s>", "kSelf");
}

#[test]
fn field_selecting_element_only_content_is_an_error() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="root"><xs:complexType><xs:sequence>
  <xs:element name="uid" maxOccurs="unbounded"><xs:complexType><xs:sequence>
    <xs:element name="pid"><xs:complexType><xs:attribute name="p" type="xs:string"/></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
 </xs:sequence></xs:complexType>
 <xs:key name="uuid"><xs:selector xpath=".//uid"/><xs:field xpath="pid"/></xs:key>
</xs:element>
</xs:schema>"#;
    assert_invalid(
        xsd,
        r#"<root><uid><pid p="1"/></uid></root>"#,
        "without a simple value",
    );
}

#[test]
fn identity_constraint_on_local_element_is_enforced() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="root"><xs:complexType><xs:sequence>
  <xs:element name="list"><xs:complexType><xs:sequence>
    <xs:element name="item" type="xs:string" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType>
   <xs:unique name="uItem"><xs:selector xpath="item"/><xs:field xpath="."/></xs:unique>
  </xs:element>
 </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#;
    assert_valid(
        xsd,
        "<root><list><item>a</item><item>b</item></list></root>",
    );
    assert_invalid(
        xsd,
        "<root><list><item>a</item><item>a</item></list></root>",
        "uItem",
    );
}

#[test]
fn union_field_is_enforced() {
    assert_valid(IDENTITY, r#"<u><k a="1"/><k b="2"/></u>"#);
    assert_invalid(IDENTITY, r#"<u><k a="1"/><k b="1"/></u>"#, "uAlt");
    assert_invalid(IDENTITY, r#"<u><k a="1" b="2"/></u>"#, "more than one");
}
