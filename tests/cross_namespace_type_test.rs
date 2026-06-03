//! Tests that the type children cache correctly isolates namespaces when
//! a schema uses only a default namespace (no explicit prefix) and another
//! schema defines types with the same local name but different children.

use fastxml::schema::types::CompiledSchema;
use fastxml::schema::{Schema, Validator};

/// Schema A uses only a default namespace (`xmlns="..."`, no explicit prefix).
/// Schema B defines types with the same local name but different children,
/// and provides the prefix for A's namespace via `xmlns:bldg="..."`.
///
/// Validates that child elements declared in A are not lost due to
/// cache collisions with B's same-named types.
#[test]
fn test_validates_correctly_when_schema_uses_default_namespace_with_conflicting_type_names() {
    let schema = build_cross_namespace_schema();

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <bldg:WallSurface
        xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
        xmlns:other="http://example.com/other">
        <bldg:opening>window-1</bldg:opening>
    </bldg:WallSurface>"#;

    let errors = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("Validation should not fail")
        .into_entries();

    let error_messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();

    assert!(
        error_messages.is_empty(),
        "Child elements from schema A should not be lost due to \
         cache collisions with schema B's same-named types. Errors: {:?}",
        error_messages
    );
}

/// Reverse direction: validates that `other:WallSurface` accepts its own
/// children (`lining`, `otherMaterial`) and does NOT accept `bldg:WallSurface`'s
/// child (`opening`).
#[test]
fn test_other_namespace_wall_surface_has_own_children_not_bldg_children() {
    let schema = build_cross_namespace_schema();

    // -- valid other:WallSurface with its own children --

    let valid_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <other:WallSurface
        xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
        xmlns:other="http://example.com/other">
        <other:lining>ceramic</other:lining>
        <other:otherMaterial>concrete</other:otherMaterial>
    </other:WallSurface>"#;

    let errors = Validator::from(valid_xml)
        .schema(schema.clone())
        .run()
        .expect("Validation should not fail")
        .into_entries();

    let error_messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
    assert!(
        error_messages.is_empty(),
        "other:WallSurface should accept its own children (lining, otherMaterial). Errors: {:?}",
        error_messages
    );

    // -- invalid other:WallSurface using bldg's child (opening) --

    let invalid_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <other:WallSurface
        xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
        xmlns:other="http://example.com/other">
        <other:opening>should-not-be-allowed</other:opening>
    </other:WallSurface>"#;

    let errors = Validator::from(invalid_xml)
        .schema(schema)
        .run()
        .expect("Validation should not fail")
        .into_entries();

    assert!(
        !errors.is_empty(),
        "other:WallSurface should NOT accept 'opening' (which belongs to bldg namespace)"
    );
}

/// Build the two-schema fixture:
/// - Schema A (bldg): default namespace, WallSurfaceType with `opening` + `wallMaterial`
/// - Schema B (other): same-named types with `lining` + `otherMaterial`
fn build_cross_namespace_schema() -> CompiledSchema {
    let bldg_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns="http://www.opengis.net/citygml/building/2.0"
               targetNamespace="http://www.opengis.net/citygml/building/2.0"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractBoundarySurfaceType" abstract="true">
            <xs:sequence>
                <xs:element name="opening" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="WallSurfaceType">
            <xs:complexContent>
                <xs:extension base="AbstractBoundarySurfaceType">
                    <xs:sequence>
                        <xs:element name="wallMaterial" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="WallSurface" type="WallSurfaceType"/>
    </xs:schema>"#;

    let other_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:other="http://example.com/other"
               xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
               targetNamespace="http://example.com/other"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractBoundarySurfaceType" abstract="true">
            <xs:sequence>
                <xs:element name="lining" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="WallSurfaceType">
            <xs:complexContent>
                <xs:extension base="other:AbstractBoundarySurfaceType">
                    <xs:sequence>
                        <xs:element name="otherMaterial" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="WallSurface" type="other:WallSurfaceType"/>
    </xs:schema>"#;

    Schema::builder()
        .add(
            "http://www.opengis.net/citygml/building/2.0/building.xsd",
            bldg_schema.as_bytes(),
        )
        .add("http://example.com/other/other.xsd", other_schema.as_bytes())
        .resolve()
        .expect("Failed to compile schemas")
}
