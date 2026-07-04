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
    assert_eq!(compiled.elements.len(), 1);
    assert!(compiled.elements.contains_key("root"));
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

    assert!(compiled.types.contains_key("PersonType"));
    if let Some(TypeDef::Complex(ct)) = compiled.types.get("PersonType") {
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

    if let Some(TypeDef::Simple(st)) = compiled.types.get("StatusType") {
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

    if let Some(TypeDef::Complex(ct)) = compiled.types.get("ExtendedType") {
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
