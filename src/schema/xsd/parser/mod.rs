//! XSD Parser using XmlEventHandler.
//!
//! This parser converts XSD XML into AST types using a stack-based approach.

mod finishers;
mod handlers;
mod helpers;
mod stack_frame;

use std::collections::HashMap;

use crate::error::Result;
use crate::event::{XmlEvent, XmlEventHandler};

use super::types::*;

pub use helpers::XSD_NAMESPACE;
use helpers::convert_start_event;
use stack_frame::StackFrame;

/// XSD Parser that implements XmlEventHandler.
pub struct XsdParser {
    /// Stack of parsing states
    stack: Vec<StackFrame>,
    /// The schema being built
    schema: XsdSchema,
    /// Detected XSD namespace prefix.
    /// - `None` = not yet detected
    /// - `Some(None)` = default namespace (no prefix)
    /// - `Some(Some("xs"))` = prefix "xs"
    xsd_prefix: Option<Option<String>>,
    /// Current text content being collected
    current_text: String,
    /// Depth counter for skipping annotation content
    skip_depth: usize,
}

impl XsdParser {
    /// Creates a new XSD parser.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            schema: XsdSchema::new(),
            xsd_prefix: None, // Not yet detected
            current_text: String::new(),
            skip_depth: 0,
        }
    }

    /// Consumes the parser and returns the parsed schema.
    pub fn into_schema(self) -> XsdSchema {
        self.schema
    }

    /// Returns a reference to the parsed schema.
    pub fn schema(&self) -> &XsdSchema {
        &self.schema
    }

    /// Checks if the element name matches an XSD element.
    fn is_xsd_element(&self, _name: &str, prefix: Option<&str>) -> bool {
        match (&self.xsd_prefix, prefix) {
            // XSD prefix detected as "xs" or "xsd", element has same prefix
            (Some(Some(xsd_prefix)), Some(p)) => p == xsd_prefix,
            // XSD uses default namespace (no prefix), element has no prefix
            (Some(None), None) => true,
            // XSD not yet detected - accept elements with no prefix (schema element)
            // This allows the initial schema element to be processed
            (None, None) => true,
            // Mismatched prefixes
            (Some(Some(_)), None) | (Some(None), Some(_)) | (None, Some(_)) => false,
        }
    }

    /// Gets the local name of an XSD element.
    fn xsd_local_name<'a>(&self, name: &'a str) -> &'a str {
        name
    }

    /// Parses attributes from a start element event.
    /// Wrapper for test compatibility.
    pub fn parse_attributes(attrs: &[(&str, &str)]) -> HashMap<String, String> {
        helpers::parse_attributes(attrs)
    }

    /// Parses and validates minOccurs/maxOccurs from attributes.
    /// Wrapper for test compatibility.
    pub fn parse_occurs(attrs: &HashMap<String, String>) -> Result<(Occurs, Occurs)> {
        helpers::parse_occurs(attrs)
    }
}

impl Default for XsdParser {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlEventHandler for XsdParser {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                attributes,
                namespace_decls,
                ..
            } => {
                let attrs: Vec<(&str, &str)> = attributes
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.handle_start(name, prefix.as_deref(), &attrs, namespace_decls)?;
            }
            XmlEvent::EndElement { name, prefix } => {
                self.handle_end(name, prefix.as_deref())?;
            }
            XmlEvent::Text(text) => {
                self.current_text.push_str(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if !self.stack.is_empty() {
            return Err(
                crate::schema::xsd::error::XsdParseError::UnexpectedEndOfSchema {
                    remaining_frames: self.stack.len(),
                }
                .into(),
            );
        }
        Ok(())
    }
}

/// Parses XSD content into an AST.
pub fn parse_xsd_ast(content: &[u8]) -> Result<XsdSchema> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_reader(content);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;

    let mut xsd_parser = XsdParser::new();
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        let event_result = reader.read_event_into(&mut buf);
        let position = reader.buffer_position();

        match event_result {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let is_empty = matches!(event_result, Ok(Event::Empty(_)));
                let xml_event = convert_start_event(e, position)?;
                xsd_parser.handle(&xml_event)?;

                if is_empty {
                    if let XmlEvent::StartElement { name, prefix, .. } = &xml_event {
                        let end_event = XmlEvent::EndElement {
                            name: name.clone(),
                            prefix: prefix.clone(),
                        };
                        xsd_parser.handle(&end_event)?;
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let full_name = std::str::from_utf8(&name_bytes)?;
                let (prefix, name) = crate::namespace::split_qname(full_name);
                let event = XmlEvent::EndElement {
                    name: name.into(),
                    prefix: prefix.map(|p| p.into()),
                };
                xsd_parser.handle(&event)?;
            }
            Ok(Event::Text(ref e)) => {
                let text =
                    e.unescape()
                        .map_err(|e| crate::parse_error::ParseError::TextDecodeError {
                            message: e.to_string(),
                        })?;
                if !text.is_empty() {
                    let event = XmlEvent::Text(text.into_owned());
                    xsd_parser.handle(&event)?;
                }
            }
            Ok(Event::Eof) => {
                xsd_parser.handle(&XmlEvent::Eof)?;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                return Err(crate::schema::xsd::error::XsdParseError::ParseError {
                    position: position as usize,
                    message: e.to_string(),
                }
                .into());
            }
        }
        buf.clear();
    }

    xsd_parser.finish()?;
    Ok(xsd_parser.into_schema())
}

#[cfg(test)]
mod tests {
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
}
