//! Tests for the XSD parser (structural rules, occurs parsing, AST shapes).

use super::*;

// =============================================================================
// Basic Schema Tests
// =============================================================================

#[test]
fn test_parse_simple_schema() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               targetNamespace="http://example.com/test"
               elementFormDefault="qualified">
        <xs:element name="root" type="xs:string"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(
        schema.target_namespace,
        Some("http://example.com/test".to_string())
    );
    assert_eq!(schema.element_form_default, FormDefault::Qualified);
    assert_eq!(schema.elements.len(), 1);
    assert_eq!(schema.elements[0].name, "root");
}

#[test]
fn test_parse_schema_attribute_form_default() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               attributeFormDefault="qualified">
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.attribute_form_default, FormDefault::Qualified);
}

#[test]
fn test_xsd_parser_new() {
    let parser = XsdParser::new();
    assert!(parser.stack.is_empty());
    assert!(parser.schema.elements.is_empty());
}

#[test]
fn test_xsd_parser_into_schema() {
    let parser = XsdParser::new();
    let schema = parser.into_schema();
    assert!(schema.elements.is_empty());
}

#[test]
fn test_xsd_parser_schema() {
    let parser = XsdParser::new();
    let schema = parser.schema();
    assert!(schema.elements.is_empty());
}

// =============================================================================
// Complex Type Tests
// =============================================================================

#[test]
fn test_parse_complex_type() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="PersonType">
            <xs:sequence>
                <xs:element name="name" type="xs:string"/>
                <xs:element name="age" type="xs:integer" minOccurs="0"/>
            </xs:sequence>
            <xs:attribute name="id" type="xs:ID" use="required"/>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.types.len(), 1);

    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        assert_eq!(ct.name, Some("PersonType".to_string()));
        assert_eq!(ct.attributes.len(), 1);
        assert_eq!(ct.attributes[0].name, Some("id".to_string()));
        assert_eq!(ct.attributes[0].use_, AttributeUse::Required);

        if let XsdComplexContent::Particle(XsdParticle::Sequence(seq)) = &ct.content {
            assert_eq!(seq.particles.len(), 2);
        } else {
            panic!("Expected sequence");
        }
    } else {
        panic!("Expected complex type");
    }
}

#[test]
fn test_parse_complex_type_choice() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="ChoiceType">
            <xs:choice>
                <xs:element name="optionA" type="xs:string"/>
                <xs:element name="optionB" type="xs:integer"/>
            </xs:choice>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::Particle(XsdParticle::Choice(choice)) = &ct.content {
            assert_eq!(choice.particles.len(), 2);
        } else {
            panic!("Expected choice");
        }
    }
}

#[test]
fn test_parse_complex_type_all() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="AllType">
            <xs:all>
                <xs:element name="fieldA" type="xs:string"/>
                <xs:element name="fieldB" type="xs:string"/>
            </xs:all>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::Particle(XsdParticle::All(all)) = &ct.content {
            assert_eq!(all.elements.len(), 2);
        } else {
            panic!("Expected all");
        }
    }
}

#[test]
fn test_parse_complex_type_mixed() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="MixedType" mixed="true">
            <xs:sequence>
                <xs:element name="item" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        assert!(ct.mixed);
    }
}

#[test]
fn test_parse_complex_type_abstract() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="AbstractType" abstract="true">
            <xs:sequence/>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        assert!(ct.is_abstract);
    }
}

// =============================================================================
// Simple Type Tests
// =============================================================================

#[test]
fn test_parse_simple_type_restriction() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="StatusType">
            <xs:restriction base="xs:string">
                <xs:enumeration value="active"/>
                <xs:enumeration value="inactive"/>
                <xs:enumeration value="pending"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.types.len(), 1);

    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        assert_eq!(st.name, Some("StatusType".to_string()));
        if let XsdSimpleTypeContent::Restriction(r) = &st.content {
            assert_eq!(r.facets.len(), 3);
            assert!(matches!(&r.facets[0], XsdFacet::Enumeration(v) if v == "active"));
        } else {
            panic!("Expected restriction");
        }
    } else {
        panic!("Expected simple type");
    }
}

#[test]
fn test_parse_simple_type_pattern() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="PhoneType">
            <xs:restriction base="xs:string">
                <xs:pattern value="[0-9]{3}-[0-9]{4}"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        if let XsdSimpleTypeContent::Restriction(r) = &st.content {
            assert!(matches!(&r.facets[0], XsdFacet::Pattern(p) if p == "[0-9]{3}-[0-9]{4}"));
        }
    }
}

#[test]
fn test_parse_simple_type_length_facets() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="BoundedString">
            <xs:restriction base="xs:string">
                <xs:minLength value="1"/>
                <xs:maxLength value="100"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        if let XsdSimpleTypeContent::Restriction(r) = &st.content {
            assert_eq!(r.facets.len(), 2);
            assert!(matches!(&r.facets[0], XsdFacet::MinLength(1)));
            assert!(matches!(&r.facets[1], XsdFacet::MaxLength(100)));
        }
    }
}

#[test]
fn test_parse_simple_type_numeric_facets() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="RangedNumber">
            <xs:restriction base="xs:integer">
                <xs:minInclusive value="0"/>
                <xs:maxInclusive value="100"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        if let XsdSimpleTypeContent::Restriction(r) = &st.content {
            assert_eq!(r.facets.len(), 2);
        }
    }
}

#[test]
fn test_parse_simple_type_list() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="IntList">
            <xs:list itemType="xs:integer"/>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        assert!(matches!(&st.content, XsdSimpleTypeContent::List(_)));
    }
}

#[test]
fn test_parse_simple_type_union() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="StringOrInt">
            <xs:union memberTypes="xs:string xs:integer"/>
        </xs:simpleType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Simple(st) = &schema.types[0] {
        if let XsdSimpleTypeContent::Union(u) = &st.content {
            assert_eq!(u.member_types.len(), 2);
        } else {
            panic!("Expected union");
        }
    }
}

// =============================================================================
// Import/Include Tests
// =============================================================================

#[test]
fn test_parse_import() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:import namespace="http://www.opengis.net/gml/3.2"
                   schemaLocation="http://schemas.opengis.net/gml/3.2.1/gml.xsd"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.imports.len(), 1);
    assert_eq!(
        schema.imports[0].namespace,
        Some("http://www.opengis.net/gml/3.2".to_string())
    );
    assert_eq!(
        schema.imports[0].schema_location,
        Some("http://schemas.opengis.net/gml/3.2.1/gml.xsd".to_string())
    );
}

#[test]
fn test_parse_include() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:include schemaLocation="types.xsd"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.includes.len(), 1);
    assert_eq!(schema.includes[0].schema_location, "types.xsd");
}

// =============================================================================
// Extension/Restriction Tests
// =============================================================================

#[test]
fn test_parse_extension() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
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

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.types.len(), 1);

    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::ComplexContent(cc) = &ct.content {
            if let XsdComplexContentDerivation::Extension(ext) = &cc.derivation {
                assert_eq!(ext.base.local, "BaseType");
                assert!(ext.particle.is_some());
            } else {
                panic!("Expected extension");
            }
        } else {
            panic!("Expected complex content");
        }
    } else {
        panic!("Expected complex type");
    }
}

#[test]
fn test_parse_complex_content_restriction() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="RestrictedType">
            <xs:complexContent>
                <xs:restriction base="BaseType">
                    <xs:sequence>
                        <xs:element name="item" type="xs:string"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::ComplexContent(cc) = &ct.content {
            assert!(matches!(
                &cc.derivation,
                XsdComplexContentDerivation::Restriction(_)
            ));
        }
    }
}

#[test]
fn test_parse_simple_content_extension() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="TypeWithAttr">
            <xs:simpleContent>
                <xs:extension base="xs:string">
                    <xs:attribute name="lang" type="xs:string"/>
                </xs:extension>
            </xs:simpleContent>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::SimpleContent(sc) = &ct.content {
            if let XsdSimpleContentDerivation::Extension(ext) = &sc.derivation {
                assert_eq!(ext.base.local, "string");
                assert_eq!(ext.attributes.len(), 1);
            }
        }
    }
}

// =============================================================================
// Element Tests
// =============================================================================

#[test]
fn test_parse_element_with_inline_type() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="child" type="xs:string"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.elements.len(), 1);
    assert!(schema.elements[0].inline_type.is_some());
}

#[test]
fn test_parse_element_nillable() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="nullable" type="xs:string" nillable="true"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert!(schema.elements[0].nillable);
}

#[test]
fn test_parse_element_default_fixed() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="withDefault" type="xs:string" default="hello"/>
        <xs:element name="withFixed" type="xs:string" fixed="world"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.elements[0].default, Some("hello".to_string()));
    assert_eq!(schema.elements[1].fixed, Some("world".to_string()));
}

#[test]
fn test_parse_element_occurs() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="T">
            <xs:sequence>
                <xs:element name="optional" type="xs:string" minOccurs="0"/>
                <xs:element name="many" type="xs:string" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::Particle(XsdParticle::Sequence(seq)) = &ct.content {
            if let XsdParticleItem::Element(e) = &seq.particles[0] {
                assert_eq!(e.min_occurs, Occurs::Count(0));
            }
            if let XsdParticleItem::Element(e) = &seq.particles[1] {
                assert_eq!(e.max_occurs, Occurs::Unbounded);
            }
        }
    }
}

// =============================================================================
// Attribute Tests
// =============================================================================

#[test]
fn test_parse_attribute_use_optional() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="T">
            <xs:attribute name="opt" type="xs:string" use="optional"/>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        assert_eq!(ct.attributes[0].use_, AttributeUse::Optional);
    }
}

#[test]
fn test_parse_attribute_use_prohibited() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="T">
            <xs:attribute name="removed" type="xs:string" use="prohibited"/>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        assert_eq!(ct.attributes[0].use_, AttributeUse::Prohibited);
    }
}

// =============================================================================
// Group Tests
// =============================================================================

#[test]
fn test_parse_group_definition() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:group name="CommonElements">
            <xs:sequence>
                <xs:element name="name" type="xs:string"/>
                <xs:element name="description" type="xs:string"/>
            </xs:sequence>
        </xs:group>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.groups.len(), 1);
    assert_eq!(schema.groups[0].name, Some("CommonElements".to_string()));
}

#[test]
fn test_parse_attribute_group() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:attributeGroup name="CommonAttrs">
            <xs:attribute name="id" type="xs:ID"/>
            <xs:attribute name="class" type="xs:string"/>
        </xs:attributeGroup>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.attribute_groups.len(), 1);
    assert_eq!(
        schema.attribute_groups[0].name,
        Some("CommonAttrs".to_string())
    );
    assert_eq!(schema.attribute_groups[0].attributes.len(), 2);
}

// =============================================================================
// Any/AnyAttribute Tests
// =============================================================================

#[test]
fn test_parse_any() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="OpenType">
            <xs:sequence>
                <xs:any processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    if let XsdTypeDef::Complex(ct) = &schema.types[0] {
        if let XsdComplexContent::Particle(XsdParticle::Sequence(seq)) = &ct.content {
            assert!(matches!(&seq.particles[0], XsdParticleItem::Any(_)));
        }
    }
}

// =============================================================================
// Annotation Tests (should be skipped)
// =============================================================================

#[test]
fn test_parse_annotation_skipped() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:annotation>
            <xs:documentation>This is documentation</xs:documentation>
            <xs:appinfo>Some app info</xs:appinfo>
        </xs:annotation>
        <xs:element name="root" type="xs:string"/>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.elements.len(), 1);
}

// =============================================================================
// Identity Constraint Tests
// =============================================================================

#[test]
fn test_parse_unique_constraint() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="item" maxOccurs="unbounded">
                        <xs:complexType>
                            <xs:attribute name="id" type="xs:string"/>
                        </xs:complexType>
                    </xs:element>
                </xs:sequence>
            </xs:complexType>
            <xs:unique name="uniqueId">
                <xs:selector xpath="item"/>
                <xs:field xpath="@id"/>
            </xs:unique>
        </xs:element>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(schema.elements[0].identity_constraints.len(), 1);
    assert_eq!(
        schema.elements[0].identity_constraints[0].constraint_type,
        XsdConstraintType::Unique
    );
}

#[test]
fn test_parse_key_constraint() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="item" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
            <xs:key name="itemKey">
                <xs:selector xpath="item"/>
                <xs:field xpath="@id"/>
            </xs:key>
        </xs:element>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(
        schema.elements[0].identity_constraints[0].constraint_type,
        XsdConstraintType::Key
    );
}

#[test]
fn test_parse_keyref_constraint() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="ref" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
            <xs:keyref name="itemRef" refer="itemKey">
                <xs:selector xpath="ref"/>
                <xs:field xpath="@refId"/>
            </xs:keyref>
        </xs:element>
    </xs:schema>"#;

    let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
    assert_eq!(
        schema.elements[0].identity_constraints[0].constraint_type,
        XsdConstraintType::KeyRef
    );
    assert!(schema.elements[0].identity_constraints[0].refer.is_some());
}

// =============================================================================
// Helper Function Tests
// =============================================================================

#[test]
fn test_parse_occurs_valid() {
    let mut attrs = HashMap::new();
    attrs.insert("minOccurs".to_string(), "0".to_string());
    attrs.insert("maxOccurs".to_string(), "unbounded".to_string());

    let (min, max) = XsdParser::parse_occurs(&attrs).unwrap();
    assert_eq!(min, Occurs::Count(0));
    assert_eq!(max, Occurs::Unbounded);
}

#[test]
fn test_parse_occurs_default() {
    let attrs = HashMap::new();
    let (min, max) = XsdParser::parse_occurs(&attrs).unwrap();
    assert_eq!(min, Occurs::Count(1));
    assert_eq!(max, Occurs::Count(1));
}

#[test]
fn test_parse_occurs_min_greater_than_max() {
    let mut attrs = HashMap::new();
    attrs.insert("minOccurs".to_string(), "5".to_string());
    attrs.insert("maxOccurs".to_string(), "2".to_string());

    let result = XsdParser::parse_occurs(&attrs);
    assert!(result.is_err());
}

#[test]
fn test_parse_occurs_invalid_value() {
    let mut attrs = HashMap::new();
    attrs.insert("minOccurs".to_string(), "invalid".to_string());

    let result = XsdParser::parse_occurs(&attrs);
    assert!(result.is_err());
}

#[test]
fn test_parse_attributes() {
    let attrs = [("name", "test"), ("type", "xs:string")];
    let parsed = XsdParser::parse_attributes(&attrs);
    assert_eq!(parsed.get("name"), Some(&"test".to_string()));
    assert_eq!(parsed.get("type"), Some(&"xs:string".to_string()));
}

// cos-all-limited: xs:all placement and occurrence constraints.

#[test]
fn all_with_max_occurs_other_than_one_rejected() {
    for max in ["0", "2", "unbounded"] {
        let xsd = format!(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
                <xs:complexType name="t">
                    <xs:all minOccurs="0" maxOccurs="{max}">
                        <xs:element name="e" type="xs:string"/>
                    </xs:all>
                </xs:complexType>
            </xs:schema>"#
        );
        assert!(
            parse_xsd_ast(xsd.as_bytes()).is_err(),
            "all with maxOccurs={max} must be rejected"
        );
    }
}

#[test]
fn all_with_min_occurs_two_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:all minOccurs="2">
                <xs:element name="e" type="xs:string"/>
            </xs:all>
        </xs:complexType>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}

#[test]
fn all_with_valid_occurs_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:all minOccurs="0" maxOccurs="1">
                <xs:element name="e" type="xs:string" minOccurs="0" maxOccurs="1"/>
            </xs:all>
        </xs:complexType>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_ok());
}

#[test]
fn all_member_with_max_occurs_two_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:all>
                <xs:element name="e" type="xs:string" maxOccurs="2"/>
            </xs:all>
        </xs:complexType>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}

#[test]
fn all_nested_in_sequence_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:sequence>
                <xs:all>
                    <xs:element name="e" type="xs:string"/>
                </xs:all>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}

#[test]
fn sequence_nested_in_all_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:all>
                <xs:sequence>
                    <xs:element name="e" type="xs:string"/>
                </xs:sequence>
            </xs:all>
        </xs:complexType>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}

#[test]
fn nested_schema_element_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:group name="g">
            <xs:schema>
                <xs:all>
                    <xs:element name="e" type="xs:string"/>
                </xs:all>
            </xs:schema>
        </xs:group>
    </xs:schema>"#;
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}

#[test]
fn simple_type_inside_simple_content_keeps_its_restriction() {
    // An attribute's anonymous simpleType nested in a simpleContent
    // extension or restriction has a restriction of its own.
    for derivation in ["extension", "restriction"] {
        let xsd = format!(
            "<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'>\
             <xs:complexType name='c'><xs:simpleContent><xs:{derivation} base='xs:string'>\
             <xs:attribute name='a'><xs:simpleType><xs:restriction base='xs:int'>\
             <xs:maxInclusive value='5'/></xs:restriction></xs:simpleType></xs:attribute>\
             </xs:{derivation}></xs:simpleContent></xs:complexType></xs:schema>"
        );
        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        let XsdTypeDef::Complex(ct) = &schema.types[0] else {
            panic!("expected a complex type");
        };
        let XsdComplexContent::SimpleContent(sc) = &ct.content else {
            panic!(
                "{derivation}: expected simple content, got {:?}",
                ct.content
            );
        };
        let attributes = match &sc.derivation {
            XsdSimpleContentDerivation::Extension(e) => &e.attributes,
            XsdSimpleContentDerivation::Restriction(r) => &r.attributes,
        };
        let inline = attributes[0].inline_type.as_ref().unwrap();
        let XsdSimpleTypeContent::Restriction(r) = &inline.content else {
            panic!("{derivation}: expected a restriction");
        };
        assert_eq!(r.base.as_ref().unwrap().local, "int", "{derivation}");
        assert_eq!(r.facets.len(), 1, "{derivation}");
    }
}

#[test]
fn attributes_not_in_the_schema_for_schemas_are_rejected() {
    let schema = |body: &str| {
        format!(
            "<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema' xmlns:f='urn:f'>{body}</xs:schema>"
        )
    };
    for body in [
        "<xs:attribute name='a' value='string'/>",
        "<xs:attribute name='a' use='required'/>",
        "<xs:attribute name='a' form='qualified'/>",
        "<xs:element name='e' minOccurs='0'/>",
        "<xs:complexType name='c'><xs:sequence><xs:element name='e'><xs:complexType name='inner'/></xs:element></xs:sequence></xs:complexType>",
        "<xs:element name='e'><xs:simpleType name='s'><xs:restriction base='xs:string'/></xs:simpleType></xs:element>",
    ] {
        assert!(
            parse_xsd_ast(schema(body).as_bytes()).is_err(),
            "should reject: {body}"
        );
    }
    for body in [
        "<xs:attribute name='a' type='xs:string' f:note='x' xml:lang='en' id='a1'/>",
        "<xs:complexType name='c'><xs:attribute name='a' use='required' form='qualified'/></xs:complexType>",
        "<xs:element name='e'><xs:complexType><xs:sequence><xs:element name='i' minOccurs='0'/></xs:sequence></xs:complexType></xs:element>",
    ] {
        assert!(
            parse_xsd_ast(schema(body).as_bytes()).is_ok(),
            "should accept: {body}"
        );
    }
}

#[test]
fn content_order_and_placement_rules() {
    let schema = |body: &str| {
        format!("<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'>{body}</xs:schema>")
    };
    for body in [
        // attributes before the particle, attribute after anyAttribute
        "<xs:complexType name='c'><xs:attribute name='a'/><xs:sequence/></xs:complexType>",
        "<xs:complexType name='c'><xs:anyAttribute/><xs:attribute name='a'/></xs:complexType>",
        "<xs:complexType name='c'><xs:complexContent><xs:extension base='xs:anyType'><xs:attributeGroup ref='g'/><xs:choice/></xs:extension></xs:complexContent></xs:complexType>",
        // simpleContent admits no particle; its extension no facet
        "<xs:complexType name='c'><xs:simpleContent><xs:extension base='xs:string'><xs:sequence/></xs:extension></xs:simpleContent></xs:complexType>",
        "<xs:complexType name='c'><xs:simpleContent><xs:extension base='xs:string'><xs:length value='5'/></xs:extension></xs:simpleContent></xs:complexType>",
        // facets come before attributes
        "<xs:complexType name='c'><xs:simpleContent><xs:restriction base='x'><xs:attribute name='a'/><xs:length value='5'/></xs:restriction></xs:simpleContent></xs:complexType>",
        // simpleContent / complexContent need a derivation
        "<xs:complexType name='c'><xs:complexContent/></xs:complexType>",
        "<xs:complexType name='c'><xs:simpleContent><xs:annotation/></xs:simpleContent></xs:complexType>",
        // other syntax
        "<xs:complexType name='c'><xs:sequence><xs:element name='e' minOccurs='unbounded'/></xs:sequence></xs:complexType>",
        "<xs:complexType name='c' xs:type='xs:int'/>",
        "<xs:attribute name='xmlns'/>",
        // simple type structure
        "<xs:simpleType name='s'><xs:annotation/></xs:simpleType>",
        "<xs:simpleType name='s'><xs:union/></xs:simpleType>",
        "<xs:simpleType name='s'><xs:restriction base='xs:string'><xs:simpleType><xs:restriction base='xs:string'/></xs:simpleType></xs:restriction></xs:simpleType>",
        "<xs:simpleType name='s'><xs:list itemType='xs:int'><xs:simpleType><xs:restriction base='xs:int'/></xs:simpleType></xs:list></xs:simpleType>",
        "<xs:simpleType><xs:restriction base='xs:string'/></xs:simpleType>",
        // local elements, named group bodies
        "<xs:complexType name='c'><xs:sequence><xs:element name='e' abstract='true'/></xs:sequence></xs:complexType>",
        "<xs:group name='g'><xs:sequence minOccurs='0'/></xs:group>",
    ] {
        assert!(
            parse_xsd_ast(schema(body).as_bytes()).is_err(),
            "should reject: {body}"
        );
    }
    for body in [
        "<xs:complexType name='c'><xs:sequence/><xs:attribute name='a'/><xs:attributeGroup ref='g'/><xs:attribute name='b'/><xs:anyAttribute/></xs:complexType>",
        "<xs:complexType name='c'><xs:simpleContent><xs:restriction base='x'><xs:simpleType><xs:restriction base='xs:string'/></xs:simpleType><xs:length value='5'/><xs:pattern value='a*'/><xs:attribute name='a'/></xs:restriction></xs:simpleContent></xs:complexType>",
        "<xs:simpleType name='s'><xs:restriction base='xs:string'><xs:length value='5'/><xs:pattern value='a*'/></xs:restriction></xs:simpleType>",
        "<xs:simpleType name='s'><xs:union memberTypes='xs:int'><xs:simpleType><xs:restriction base='xs:string'/></xs:simpleType><xs:simpleType><xs:restriction base='xs:date'/></xs:simpleType></xs:union></xs:simpleType>",
        "<xs:simpleType name='s'><xs:list><xs:simpleType><xs:restriction base='xs:int'/></xs:simpleType></xs:list></xs:simpleType>",
        "<xs:group name='g'><xs:sequence><xs:element name='e' minOccurs='0'/></xs:sequence></xs:group>",
    ] {
        assert!(
            parse_xsd_ast(schema(body).as_bytes()).is_ok(),
            "should accept: {body}"
        );
    }
}

#[test]
fn empty_target_namespace_is_rejected() {
    let xsd = "<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema' targetNamespace=''/>";
    assert!(parse_xsd_ast(xsd.as_bytes()).is_err());
}
