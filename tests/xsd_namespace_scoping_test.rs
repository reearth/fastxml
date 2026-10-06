//! QName-valued XSD attributes resolve against the namespace declarations in
//! scope on the element that carries them (its own over its ancestors',
//! innermost wins), not against one schema-wide prefix table.
//!
//! Two imported namespaces each declare a simple type `T` with a different
//! value space (`urn:a` = xs:int, `urn:b` = xs:boolean), so which `T` a
//! reference resolved to is visible in what the element accepts.

use std::sync::Arc;

use fastxml::schema::Schema;

const XS: &str = "http://www.w3.org/2001/XMLSchema";

const SCHEMA_A: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a">
  <xs:simpleType name="T"><xs:restriction base="xs:int"/></xs:simpleType>
</xs:schema>"#;

const SCHEMA_B: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:b">
  <xs:simpleType name="T"><xs:restriction base="xs:boolean"/></xs:simpleType>
</xs:schema>"#;

/// Wraps declarations in a `urn:m` schema importing `urn:a` and `urn:b`.
fn main_schema(root_attrs: &str, body: &str) -> String {
    format!(
        r#"<xs:schema xmlns:xs="{XS}" targetNamespace="urn:m" xmlns:m="urn:m" elementFormDefault="qualified" {root_attrs}>
  <xs:import namespace="urn:a" schemaLocation="a.xsd"/>
  <xs:import namespace="urn:b" schemaLocation="b.xsd"/>
  {body}
</xs:schema>"#
    )
}

fn compile(main: &str) -> fastxml::Result<Schema> {
    Schema::builder()
        .add("main.xsd", main.as_bytes().to_vec())
        .add("a.xsd", SCHEMA_A.as_bytes().to_vec())
        .add("b.xsd", SCHEMA_B.as_bytes().to_vec())
        .resolve()
}

/// Validity of `<m:{elem}>{text}</m:{elem}>` in both engines (which must
/// agree).
fn accepts(schema: &Arc<Schema>, elem: &str, text: &str) -> bool {
    let xml = format!(r#"<m:{elem} xmlns:m="urn:m">{text}</m:{elem}>"#);
    let doc = fastxml::Parser::from(xml.as_str()).parse().expect("parse");
    let dom = fastxml::schema::Validator::from(&doc)
        .schema(Arc::clone(schema))
        .run()
        .expect("validate")
        .is_valid();
    let streaming = fastxml::schema::Validator::from(xml.as_str())
        .schema(Arc::clone(schema))
        .run()
        .expect("validate")
        .is_valid();
    assert_eq!(dom, streaming, "engines disagree on {xml}");
    dom
}

/// Asserts that `elem` has `urn:a`'s `T` (int) or `urn:b`'s `T` (boolean).
fn assert_type(schema: &Arc<Schema>, elem: &str, ns: &str) {
    let (ok, bad) = match ns {
        "urn:a" => ("5", "true"),
        "urn:b" => ("true", "5"),
        _ => unreachable!(),
    };
    assert!(accepts(schema, elem, ok), "{elem} should be {ns}'s T");
    assert!(!accepts(schema, elem, bad), "{elem} should be {ns}'s T");
}

#[test]
fn prefix_declared_on_nested_element_resolves_there() {
    let main = main_schema("", r#"<xs:element name="e" xmlns:p="urn:a" type="p:T"/>"#);
    let schema = Arc::new(compile(&main).expect("compile"));
    assert_type(&schema, "e", "urn:a");
}

#[test]
fn same_prefix_in_sibling_subtrees_resolves_per_subtree() {
    let main = main_schema(
        "",
        r#"<xs:element name="first" xmlns:p="urn:a" type="p:T"/>
  <xs:element name="second" xmlns:p="urn:b" type="p:T"/>
  <xs:complexType name="Wrapper" xmlns:p="urn:a">
    <xs:sequence>
      <xs:element name="inner" type="p:T"/>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="third" type="m:Wrapper"/>"#,
    );
    let schema = Arc::new(compile(&main).expect("compile"));
    assert_type(&schema, "first", "urn:a");
    assert_type(&schema, "second", "urn:b");

    let wrapped = |text: &str| {
        let xml = format!(r#"<m:third xmlns:m="urn:m"><m:inner>{text}</m:inner></m:third>"#);
        let doc = fastxml::Parser::from(xml.as_str()).parse().expect("parse");
        fastxml::schema::Validator::from(&doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("validate")
            .is_valid()
    };
    assert!(wrapped("5"), "inner element inherits the ancestor's p");
    assert!(!wrapped("true"), "inner element inherits the ancestor's p");
}

#[test]
fn nested_binding_does_not_leak_to_later_siblings() {
    let main = main_schema(
        "",
        r#"<xs:element name="first" xmlns:p="urn:a" type="p:T"/>
  <xs:element name="second" type="p:T"/>"#,
    );
    let err = compile(&main).expect_err("p is not in scope on 'second'");
    assert!(
        err.to_string().contains("p:T"),
        "error should name the unresolved reference: {err}"
    );
}

#[test]
fn inner_default_namespace_overrides_outer_for_unprefixed_refs() {
    let main = main_schema(
        r#"xmlns="urn:a""#,
        r#"<xs:element name="outer" type="T"/>
  <xs:element name="inner" xmlns="urn:b" type="T"/>
  <xs:element name="after" type="T"/>"#,
    );
    let schema = Arc::new(compile(&main).expect("compile"));
    assert_type(&schema, "outer", "urn:a");
    assert_type(&schema, "inner", "urn:b");
    assert_type(&schema, "after", "urn:a");
}

#[test]
fn declarations_inside_documentation_are_ignored() {
    let main = main_schema(
        r#"xmlns:p="urn:a""#,
        r#"<xs:annotation>
    <xs:documentation>
      Example:
      <xs:schema xmlns:xs="http://example.com/not-xsd" xmlns:p="urn:b" xmlns="urn:b">
        <xs:element name="x" type="p:T"/>
      </xs:schema>
    </xs:documentation>
  </xs:annotation>
  <xs:element name="e" type="p:T"/>
  <xs:element name="n" type="xs:int"/>"#,
    );
    let schema = Arc::new(compile(&main).expect("compile"));
    assert_type(&schema, "e", "urn:a");
    assert!(accepts(&schema, "n", "7"));
    assert!(!accepts(&schema, "n", "seven"));
}

#[test]
fn ast_records_root_bindings_and_in_scope_qname_namespaces() {
    let main = main_schema(
        r#"xmlns:p="urn:a""#,
        r#"<xs:annotation><xs:documentation><x xmlns:q="urn:doc"/></xs:documentation></xs:annotation>
  <xs:element name="e" xmlns:p="urn:b" type="p:T"/>
  <xs:element name="f" type="p:T"/>
  <xs:element name="g" type="T"/>"#,
    );
    let ast = fastxml::schema::xsd::parse_xsd_ast(main.as_bytes()).expect("parse");

    // Document-level bindings are the root element's declarations only.
    assert_eq!(
        ast.namespace_bindings.get("p").map(String::as_str),
        Some("urn:a")
    );
    assert!(!ast.namespace_bindings.contains_key("q"));

    let type_ns = |name: &str| {
        ast.elements
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.type_ref.as_ref())
            .and_then(|q| q.namespace.clone())
    };
    assert_eq!(type_ns("e").as_deref(), Some("urn:b"));
    assert_eq!(type_ns("f").as_deref(), Some("urn:a"));
    // Unprefixed with no default namespace in scope.
    assert_eq!(type_ns("g").as_deref(), Some(""));
}

#[test]
fn identity_xpath_prefix_declared_on_nested_element() {
    // `t` is declared on the element carrying the key, not on xs:schema.
    let xsd = format!(
        r#"<xs:schema xmlns:xs="{XS}" targetNamespace="urn:t" elementFormDefault="qualified">
  <xs:element name="root" xmlns:t="urn:t">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="item" maxOccurs="unbounded">
          <xs:complexType><xs:attribute name="id" type="xs:string"/></xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="k">
      <xs:selector xpath="t:item"/>
      <xs:field xpath="@id"/>
    </xs:key>
  </xs:element>
</xs:schema>"#
    );
    let schema = Arc::new(Schema::from_xsd(xsd.as_bytes()).expect("compile"));
    // The instance uses a default namespace, so only the schema binds `t`.
    let xml = r#"<root xmlns="urn:t"><item id="a"/><item id="a"/></root>"#;
    let doc = fastxml::Parser::from(xml).parse().expect("parse");
    let dom = fastxml::schema::Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .expect("validate");
    assert!(!dom.is_valid(), "DOM must report the duplicate key");
    let streaming = fastxml::schema::Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validate");
    assert!(
        !streaming.is_valid(),
        "streaming must report the duplicate key"
    );
}
