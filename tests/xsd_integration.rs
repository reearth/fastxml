//! Integration tests for XSD parsing with PLATEAU CityGML data.

use std::sync::Arc;

use fastxml::event::{XmlEvent, XmlEventHandler};
use fastxml::schema::validator::OnePassSchemaValidator;
use fastxml::schema::xsd;

/// Test parsing a simple CityGML-like schema
#[test]
fn test_citygml_schema_parsing() {
    let citygml_like_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml/3.2"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/relief/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/gml/3.2"/>
        <xs:import namespace="http://www.opengis.net/citygml/2.0"/>

        <xs:element name="ReliefFeature" type="ReliefFeatureType" substitutionGroup="core:_CityObject"/>

        <xs:complexType name="ReliefFeatureType">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <xs:element name="lod" type="xs:integer"/>
                        <xs:element name="reliefComponent" type="ReliefComponentPropertyType" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:complexType name="ReliefComponentPropertyType">
            <xs:sequence minOccurs="0">
                <xs:element ref="TINRelief"/>
            </xs:sequence>
            <xs:attributeGroup ref="gml:AssociationAttributeGroup"/>
        </xs:complexType>

        <xs:element name="TINRelief" type="TINReliefType" substitutionGroup="core:_CityObject"/>

        <xs:complexType name="TINReliefType">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <xs:element name="lod" type="xs:integer"/>
                        <xs:element name="tin" type="gml:TriangulatedSurfacePropertyType"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

    </xs:schema>"#;

    let schema = xsd::parse_xsd(citygml_like_xsd.as_bytes()).unwrap();

    // Check that elements are parsed
    assert!(schema.elements.contains_key("ReliefFeature"));
    assert!(schema.elements.contains_key("TINRelief"));

    // Check that types are parsed
    assert!(schema.types.contains_key("ReliefFeatureType"));
    assert!(schema.types.contains_key("TINReliefType"));
    assert!(schema.types.contains_key("ReliefComponentPropertyType"));

    // Check that built-in types are available
    assert!(schema.types.contains_key("xs:integer"));
    assert!(schema.types.contains_key("gml:CodeType"));
}

/// Test creating a validation context with built-in types
#[test]
fn test_builtin_schema_validation() {
    let schema = xsd::create_builtin_schema();
    let validator = OnePassSchemaValidator::new(Arc::new(schema));

    // The validator should be valid initially
    assert!(validator.is_valid());
}

/// Test parsing and validating a simple GML document
#[test]
fn test_gml_document_validation() {
    let schema = xsd::create_builtin_schema();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Simulate parsing a GML document
    validator
        .handle(&XmlEvent::StartElement {
            name: "Envelope".into(),
            prefix: Some("gml".into()),
            namespace: Some("http://www.opengis.net/gml/3.2".into()),
            attributes: vec![
                ("srsName".into(), "EPSG:6697".into()),
                ("srsDimension".into(), "3".into()),
            ],
            namespace_decls: vec![],
            line: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::StartElement {
            name: "lowerCorner".into(),
            prefix: Some("gml".into()),
            namespace: Some("http://www.opengis.net/gml/3.2".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::Text("35.0 135.0 0.0".into()))
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "lowerCorner".into(),
            prefix: Some("gml".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Envelope".into(),
            prefix: Some("gml".into()),
        })
        .unwrap();

    validator.handle(&XmlEvent::Eof).unwrap();
    validator.finish().unwrap();

    // Validation should pass
    assert!(validator.is_valid());
}

/// Test parsing PLATEAU building schema patterns
#[test]
fn test_plateau_building_pattern() {
    let building_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml/3.2"
               xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
               targetNamespace="http://www.opengis.net/citygml/building/2.0"
               elementFormDefault="qualified">

        <xs:element name="Building" type="BuildingType"/>

        <xs:complexType name="BuildingType">
            <xs:sequence>
                <xs:element name="class" type="gml:CodeType" minOccurs="0"/>
                <xs:element name="function" type="gml:CodeType" minOccurs="0" maxOccurs="unbounded"/>
                <xs:element name="usage" type="gml:CodeType" minOccurs="0" maxOccurs="unbounded"/>
                <xs:element name="yearOfConstruction" type="xs:gYear" minOccurs="0"/>
                <xs:element name="yearOfDemolition" type="xs:gYear" minOccurs="0"/>
                <xs:element name="roofType" type="gml:CodeType" minOccurs="0"/>
                <xs:element name="measuredHeight" type="gml:LengthType" minOccurs="0"/>
                <xs:element name="storeysAboveGround" type="xs:nonNegativeInteger" minOccurs="0"/>
                <xs:element name="storeysBelowGround" type="xs:nonNegativeInteger" minOccurs="0"/>
                <xs:element name="storeyHeightsAboveGround" type="gml:MeasureOrNullListType" minOccurs="0"/>
                <xs:element name="storeyHeightsBelowGround" type="gml:MeasureOrNullListType" minOccurs="0"/>
                <xs:element name="lod0FootPrint" type="gml:MultiSurfacePropertyType" minOccurs="0"/>
                <xs:element name="lod0RoofEdge" type="gml:MultiSurfacePropertyType" minOccurs="0"/>
                <xs:element name="lod1Solid" type="gml:SolidPropertyType" minOccurs="0"/>
                <xs:element name="lod2Solid" type="gml:SolidPropertyType" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

    </xs:schema>"#;

    let schema = xsd::parse_xsd(building_xsd.as_bytes()).unwrap();

    // Check that Building element exists
    assert!(schema.elements.contains_key("Building"));

    // Check that BuildingType exists and has correct structure
    assert!(schema.types.contains_key("BuildingType"));

    if let Some(fastxml::schema::types::TypeDef::Complex(ct)) = schema.types.get("BuildingType") {
        // Check that it's a sequence with elements
        if let fastxml::schema::types::ContentModel::Sequence(elements) = &ct.content {
            // Find specific elements
            assert!(elements.iter().any(|e| e.name == "class"));
            assert!(elements.iter().any(|e| e.name == "measuredHeight"));
            assert!(elements.iter().any(|e| e.name == "storeysAboveGround"));

            // Check that optional elements have min_occurs = 0
            let class_elem = elements.iter().find(|e| e.name == "class").unwrap();
            assert_eq!(class_elem.min_occurs, 0);

            // Check unbounded elements
            let function_elem = elements.iter().find(|e| e.name == "function").unwrap();
            assert_eq!(function_elem.min_occurs, 0);
            assert_eq!(function_elem.max_occurs, None); // unbounded
        } else {
            panic!("Expected Sequence content model");
        }
    } else {
        panic!("Expected Complex type");
    }
}

/// Test parsing i-UR extension patterns used in PLATEAU
#[test]
fn test_plateau_iur_extension_pattern() {
    let uro_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml/3.2"
               xmlns:uro="https://www.geospatial.jp/iur/uro/3.2"
               targetNamespace="https://www.geospatial.jp/iur/uro/3.2"
               elementFormDefault="qualified">

        <xs:element name="buildingDetails" type="BuildingDetailsPropertyType"/>

        <xs:complexType name="BuildingDetailsPropertyType">
            <xs:sequence minOccurs="0">
                <xs:element ref="BuildingDetails"/>
            </xs:sequence>
        </xs:complexType>

        <xs:element name="BuildingDetails" type="BuildingDetailsType"/>

        <xs:complexType name="BuildingDetailsType">
            <xs:sequence>
                <xs:element name="serialNumberOfBuildingCertification" type="xs:string" minOccurs="0"/>
                <xs:element name="siteArea" type="gml:MeasureType" minOccurs="0"/>
                <xs:element name="totalFloorArea" type="gml:MeasureType" minOccurs="0"/>
                <xs:element name="buildingFootprintArea" type="gml:MeasureType" minOccurs="0"/>
                <xs:element name="buildingRoofEdgeArea" type="gml:MeasureType" minOccurs="0"/>
                <xs:element name="buildingStructureType" type="gml:CodeType" minOccurs="0"/>
                <xs:element name="fireproofStructureType" type="gml:CodeType" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:simpleType name="BuildingIDAttributeType">
            <xs:restriction base="xs:string">
                <xs:pattern value="[0-9]{13}"/>
            </xs:restriction>
        </xs:simpleType>

    </xs:schema>"#;

    let schema = xsd::parse_xsd(uro_xsd.as_bytes()).unwrap();

    // Check elements
    assert!(schema.elements.contains_key("buildingDetails"));
    assert!(schema.elements.contains_key("BuildingDetails"));

    // Check types
    assert!(schema.types.contains_key("BuildingDetailsType"));
    assert!(schema.types.contains_key("BuildingDetailsPropertyType"));
    assert!(schema.types.contains_key("BuildingIDAttributeType"));

    // Check simple type restriction
    if let Some(fastxml::schema::types::TypeDef::Simple(st)) =
        schema.types.get("BuildingIDAttributeType")
    {
        assert_eq!(st.base_type.as_deref(), Some("xs:string"));
        assert!(st.pattern.is_some());
        assert_eq!(st.pattern.as_deref(), Some("[0-9]{13}"));
    } else {
        panic!("Expected Simple type");
    }
}

/// Test that we can parse a GML file and use the streaming validator
#[test]
fn test_parse_real_gml_structure() {
    // A minimal valid CityGML structure
    let citygml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                    xmlns:gml="http://www.opengis.net/gml"
                    xmlns:dem="http://www.opengis.net/citygml/relief/2.0">
        <gml:boundedBy>
            <gml:Envelope srsName="http://www.opengis.net/def/crs/EPSG/0/6697" srsDimension="3">
                <gml:lowerCorner>35.0 135.0 0.0</gml:lowerCorner>
                <gml:upperCorner>36.0 136.0 100.0</gml:upperCorner>
            </gml:Envelope>
        </gml:boundedBy>
        <core:cityObjectMember>
            <dem:ReliefFeature gml:id="dem_test_001">
                <dem:lod>1</dem:lod>
            </dem:ReliefFeature>
        </core:cityObjectMember>
    </core:CityModel>"#;

    // Parse the document
    let doc = fastxml::parse(citygml).unwrap();
    let root = doc.get_root_element().unwrap();

    assert_eq!(root.get_name(), "CityModel");
    assert_eq!(root.get_prefix(), Some("core".into()));

    // Get child elements
    let children = root.get_child_elements();
    assert!(children.iter().any(|c| c.get_name() == "boundedBy"));
    assert!(children.iter().any(|c| c.get_name() == "cityObjectMember"));
}

/// Test streaming parsing with validator
#[test]
fn test_streaming_parse_with_validator() {
    let citygml = r#"<?xml version="1.0" encoding="UTF-8"?>
    <gml:Envelope xmlns:gml="http://www.opengis.net/gml/3.2" srsName="EPSG:6697">
        <gml:lowerCorner>35.0 135.0</gml:lowerCorner>
        <gml:upperCorner>36.0 136.0</gml:upperCorner>
    </gml:Envelope>"#;

    // Parse the document
    let doc = fastxml::parse(citygml).unwrap();

    // Create schema with built-in types and use it for validation
    let schema = xsd::create_builtin_schema();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Simulate validation by handling events
    validator
        .handle(&XmlEvent::StartElement {
            name: "Envelope".into(),
            prefix: Some("gml".into()),
            namespace: Some("http://www.opengis.net/gml/3.2".into()),
            attributes: vec![("srsName".into(), "EPSG:6697".into())],
            namespace_decls: vec![],
            line: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Envelope".into(),
            prefix: Some("gml".into()),
        })
        .unwrap();

    validator.finish().unwrap();

    // Validator should be valid
    assert!(validator.is_valid());

    // Verify we can parse the document
    let root = doc.get_root_element().unwrap();
    assert_eq!(root.get_name(), "Envelope");
}
