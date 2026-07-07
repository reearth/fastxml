//! Tests for the redesigned `Schema` construction API
//! (`Schema::from_xsd` / `Schema::builtin` / `Schema::builder`).

use fastxml::schema::Schema;

const SIMPLE_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/test">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

#[test]
fn from_xsd_compiles_single_document() {
    let schema = Schema::from_xsd(SIMPLE_XSD.as_bytes()).unwrap();
    assert!(schema.get_element("root").is_some());
    assert_eq!(
        schema.target_namespace.as_deref(),
        Some("http://example.com/test")
    );
}

#[test]
fn from_xsd_accepts_owned_bytes() {
    // `impl AsRef<[u8]>` — a Vec<u8> must work as well as a &[u8].
    let owned: Vec<u8> = SIMPLE_XSD.as_bytes().to_vec();
    let schema = Schema::from_xsd(owned).unwrap();
    assert!(schema.get_element("root").is_some());
}

#[test]
fn from_xsd_rejects_malformed_input() {
    assert!(Schema::from_xsd(b"<xs:schema unclosed").is_err());
}

#[test]
fn builtin_has_types_but_no_user_elements() {
    let schema = Schema::builtin();
    assert!(
        schema.elements_ns.is_empty(),
        "builtin schema should declare no user elements"
    );
    assert!(
        !schema.types_ns.is_empty(),
        "builtin schema should register built-in types"
    );
}

#[test]
fn builder_single_entry_resolves() {
    let schema = Schema::builder()
        .add("http://example.com/test.xsd", SIMPLE_XSD.as_bytes())
        .resolve()
        .unwrap();
    assert!(schema.get_element("root").is_some());
}

#[test]
fn builder_combines_multiple_independent_entries() {
    let a = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/a">
    <xs:element name="alpha" type="xs:string"/>
</xs:schema>"#;
    let b = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="http://example.com/b">
    <xs:element name="beta" type="xs:string"/>
</xs:schema>"#;

    let schema = Schema::builder()
        .add("http://example.com/a.xsd", a.as_bytes())
        .add("http://example.com/b.xsd", b.as_bytes())
        .resolve()
        .unwrap();

    assert!(schema.get_element("alpha").is_some());
    assert!(schema.get_element("beta").is_some());
}
