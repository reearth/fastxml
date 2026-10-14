//! Tests for the XSD compiler.

use super::*;
use crate::schema::types::{ContentModel, TypeDef};
use crate::schema::xsd::parser::parse_xsd_ast;

#[test]
fn test_compile_simple_schema() {
    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               targetNamespace="http://example.com/test">
        <xs:element name="root" type="xs:string"/>
    </xs:schema>"#;

    let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
    let compiled = compile_schemas(vec![ast]).unwrap();

    assert_eq!(
        compiled.target_namespace,
        Some("http://example.com/test".to_string())
    );
    assert_eq!(compiled.elements_ns.len(), 1);
    assert!(compiled.get_element("root").is_some());
}

#[test]
fn test_compile_complex_type() {
    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="PersonType">
            <xs:sequence>
                <xs:element name="name" type="xs:string"/>
                <xs:element name="age" type="xs:integer" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
    let compiled = compile_schemas(vec![ast]).unwrap();

    assert!(compiled.get_type("PersonType").is_some());
    if let Some(TypeDef::Complex(ct)) = compiled.get_type("PersonType") {
        if let ContentModel::Sequence(elems) = &ct.content {
            assert_eq!(elems.len(), 2);
            assert_eq!(elems[0].name, "name");
            assert_eq!(elems[1].name, "age");
            assert_eq!(elems[1].min_occurs, 0);
        } else {
            panic!("Expected sequence content");
        }
    } else {
        panic!("Expected complex type");
    }
}

#[test]
fn test_compile_simple_type_enumeration() {
    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="StatusType">
            <xs:restriction base="xs:string">
                <xs:enumeration value="active"/>
                <xs:enumeration value="inactive"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
    let compiled = compile_schemas(vec![ast]).unwrap();

    if let Some(TypeDef::Simple(st)) = compiled.get_type("StatusType") {
        assert_eq!(st.enumeration.len(), 2);
        assert!(st.enumeration.contains(&"active".to_string()));
        assert!(st.enumeration.contains(&"inactive".to_string()));
    } else {
        panic!("Expected simple type");
    }
}

#[test]
fn test_compile_extension() {
    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="BaseType">
            <xs:sequence>
                <xs:element name="core" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="ExtendedType">
            <xs:complexContent>
                <xs:extension base="BaseType">
                    <xs:sequence>
                        <xs:element name="extra" type="xs:string"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;

    let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
    let compiled = compile_schemas(vec![ast]).unwrap();

    if let Some(TypeDef::Complex(ct)) = compiled.get_type("ExtendedType") {
        if let ContentModel::ComplexExtension {
            base_type,
            elements,
        } = &ct.content
        {
            assert_eq!(base_type, "BaseType");
            assert_eq!(elements.len(), 1);
            assert_eq!(elements[0].name, "extra");
        } else {
            panic!("Expected complex extension");
        }
    } else {
        panic!("Expected complex type");
    }
}

/// Compiles a schema in namespace `http://t` with `body` as its content.
fn compile_body(body: &str) -> crate::error::Result<CompiledSchema> {
    let xsd = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">{body}</xs:schema>"#
    );
    compile_schemas(vec![parse_xsd_ast(xsd.as_bytes())?])
}

#[test]
fn attribute_uses_under_restriction() {
    let base = r###"<xs:complexType name="B">
        <xs:attribute name="req" use="required"/>
        <xs:attribute name="fix" fixed="1" type="xs:int"/>
        <xs:attribute name="opt"/>
    </xs:complexType>
    <xs:complexType name="W"><xs:anyAttribute namespace="##local"/></xs:complexType>"###;
    let restrict = |base_type: &str, body: &str| {
        format!(
            r#"{base}<xs:complexType name="R"><xs:complexContent>
               <xs:restriction base="x:{base_type}">{body}</xs:restriction>
               </xs:complexContent></xs:complexType>"#
        )
    };
    for (what, base_type, body, ok) in [
        (
            "keeps required",
            "B",
            r#"<xs:attribute name="req" use="required"/>"#,
            true,
        ),
        (
            "loosens required",
            "B",
            r#"<xs:attribute name="req"/>"#,
            false,
        ),
        (
            "prohibits required",
            "B",
            r#"<xs:attribute name="req" use="prohibited"/>"#,
            false,
        ),
        (
            "same fixed value",
            "B",
            r#"<xs:attribute name="fix" fixed="01" type="xs:int"/>"#,
            true,
        ),
        (
            "other fixed value",
            "B",
            r#"<xs:attribute name="fix" fixed="2" type="xs:int"/>"#,
            false,
        ),
        (
            "prohibits optional",
            "B",
            r#"<xs:attribute name="opt" use="prohibited"/>"#,
            true,
        ),
        (
            "adds an attribute",
            "B",
            r#"<xs:attribute name="new"/>"#,
            false,
        ),
        ("adds a wildcard", "B", "<xs:anyAttribute/>", false),
        (
            "attribute within the wildcard",
            "W",
            r#"<xs:attribute name="new"/>"#,
            true,
        ),
        (
            "narrower wildcard",
            "W",
            r###"<xs:anyAttribute namespace="##local"/>"###,
            true,
        ),
        ("wider wildcard", "W", "<xs:anyAttribute/>", false),
    ] {
        let result = compile_body(&restrict(base_type, body));
        assert_eq!(result.is_ok(), ok, "{what}: {:?}", result.err());
    }
}

#[test]
fn value_constraints_meet_every_facet() {
    let colors = r#"<xs:simpleType name="Color"><xs:restriction base="xs:string">
        <xs:enumeration value="red"/><xs:enumeration value="blue"/>
    </xs:restriction></xs:simpleType>
    <xs:attribute name="g" type="xs:int" fixed="7"/>"#;
    for (what, body, ok) in [
        (
            "enumerated default",
            r#"<xs:element name="e" type="x:Color" default="red"/>"#,
            true,
        ),
        (
            "default outside the enumeration",
            r#"<xs:element name="e" type="x:Color" default="green"/>"#,
            false,
        ),
        (
            "nested attribute outside the enumeration",
            r#"<xs:complexType name="c"><xs:attribute name="a" type="x:Color" fixed="green"/></xs:complexType>"#,
            false,
        ),
        (
            "use repeating the fixed value",
            r#"<xs:complexType name="c"><xs:attribute ref="x:g" fixed="07"/></xs:complexType>"#,
            true,
        ),
        (
            "use changing the fixed value",
            r#"<xs:complexType name="c"><xs:attribute ref="x:g" fixed="8"/></xs:complexType>"#,
            false,
        ),
        (
            "attribute twice through a group",
            r#"<xs:attributeGroup name="ag"><xs:attribute name="a"/></xs:attributeGroup>
               <xs:complexType name="c"><xs:attribute name="a"/><xs:attributeGroup ref="x:ag"/></xs:complexType>"#,
            false,
        ),
    ] {
        let result = compile_body(&format!("{colors}{body}"));
        assert_eq!(result.is_ok(), ok, "{what}: {:?}", result.err());
    }
}

#[test]
fn simple_content_extension_bases() {
    let element_only = r#"<xs:complexType name="E"><xs:sequence><xs:element name="i"/></xs:sequence></xs:complexType>
        <xs:complexType name="S"><xs:simpleContent><xs:extension base="xs:string"/></xs:simpleContent></xs:complexType>"#;
    for (what, body, ok) in [
        (
            "simple content from simple content",
            r#"<xs:complexType name="c"><xs:simpleContent><xs:extension base="x:S"/></xs:simpleContent></xs:complexType>"#,
            true,
        ),
        (
            "simple content from element-only content",
            r#"<xs:complexType name="c"><xs:simpleContent><xs:extension base="x:E"/></xs:simpleContent></xs:complexType>"#,
            false,
        ),
        (
            "elements added to simple content",
            r#"<xs:complexType name="c"><xs:complexContent><xs:extension base="x:S"><xs:sequence><xs:element name="i"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>"#,
            false,
        ),
    ] {
        let result = compile_body(&format!("{element_only}{body}"));
        assert_eq!(result.is_ok(), ok, "{what}: {:?}", result.err());
    }
}
