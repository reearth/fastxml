//! Tests for type inheritance and extension chains.

use fastxml::schema::types::TypeDef;
use fastxml::schema::{CompiledSchema, ComplexType, ContentModel, ElementDef};

/// Tests that element type resolution uses namespace-qualified type references.
///
/// When tran:Track element has type_ref="tran:TrackType", it should resolve to
/// tran:TrackType, not gml:TrackType.
#[test]
fn test_element_type_ref_with_namespace() {
    let mut schema = CompiledSchema::new();

    // Create both TrackTypes with different structures
    let gml_track_type = ComplexType::sequence(
        "gml:TrackType",
        vec![ElementDef::new("MovingObjectStatus").with_type("gml:MovingObjectStatusType")],
    );

    let tran_track_type = ComplexType::sequence(
        "tran:TrackType",
        vec![
            ElementDef::new("class")
                .with_type("gml:CodeType")
                .optional(),
        ],
    );

    schema.types.insert(
        "gml:TrackType".to_string(),
        TypeDef::Complex(gml_track_type),
    );
    schema.types.insert(
        "tran:TrackType".to_string(),
        TypeDef::Complex(tran_track_type),
    );

    // Create tran:Track element with namespace-qualified type reference
    let track_element = ElementDef::new("Track").with_type("tran:TrackType");

    schema
        .elements
        .insert("tran:Track".to_string(), track_element);

    // Lookup the element
    let elem = schema.get_element("tran:Track");
    assert!(elem.is_some(), "tran:Track element should be found");

    // Get the type reference and resolve it
    let type_ref = elem.unwrap().type_ref.as_ref().unwrap();
    assert_eq!(type_ref, "tran:TrackType");

    // Resolve the type - should get tran:TrackType, not gml:TrackType
    let resolved_type = schema.get_type(type_ref);
    assert!(resolved_type.is_some(), "tran:TrackType should be resolved");

    if let Some(TypeDef::Complex(complex)) = resolved_type {
        if let ContentModel::Sequence(elements) = &complex.content {
            // Should have "class" element, not "MovingObjectStatus"
            assert_eq!(
                elements[0].name, "class",
                "tran:Track should use tran:TrackType (with 'class'), not gml:TrackType (with 'MovingObjectStatus')"
            );
        } else {
            panic!("expected sequence content");
        }
    }
}

/// Tests that elements defined in a base type via xs:extension are visible in derived types.
///
/// This reproduces a bug where:
/// - TransportationComplexType defines `class`, `function`, `lod1MultiSurface`
/// - RoadType extends TransportationComplexType via xs:extension
/// - Road element uses RoadType
/// - When validating <tran:Road><tran:class>...</tran:class></tran:Road>,
///   the validator fails to find `class` because it doesn't traverse the inheritance chain
#[test]
fn test_inherited_elements_from_base_type_extension() {
    use fastxml::schema::{Schema, Validator};

    // Schema mimicking CityGML Transportation module inheritance chain:
    // AbstractCityObjectType -> AbstractTransportationObjectType -> TransportationComplexType -> RoadType
    let schema_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <!-- Base type with some elements -->
        <xs:complexType name="AbstractTransportationObjectType" abstract="true">
            <xs:sequence>
                <xs:element name="description" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <!-- TransportationComplexType extends base and adds class, function, lod1MultiSurface -->
        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="tran:AbstractTransportationObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                        <xs:element name="lod1MultiSurface" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- RoadType extends TransportationComplexType -->
        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="tran:TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- Road element uses RoadType -->
        <xs:element name="Road" type="tran:RoadType"/>
    </xs:schema>"#;

    let schema = Schema::builder().add("http://www.opengis.net/citygml/transportation/2.0/transportation.xsd", schema_xsd.as_bytes()).resolve()
    .expect("Failed to compile schema");

    // Debug: Print the type structure
    if let Some(TypeDef::Complex(road_type)) = schema.get_type("tran:RoadType") {
        eprintln!("RoadType content: {:?}", road_type.content);
    }

    // Validate XML: <tran:Road><tran:class>道路</tran:class></tran:Road>
    // `class` is defined in TransportationComplexType, the base of RoadType;
    // the validator should find it via inheritance.
    let xml = r#"<tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"><tran:class>道路</tran:class></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validation failed");

    // Check for errors
    let errors: Vec<_> = report
        .entries()
        .iter()
        .filter(|e| e.message.contains("not declared") || e.message.contains("not expected"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    // After the fix, this should pass - class element should be found via inheritance
    assert!(
        errors.is_empty(),
        "Elements from base type (class) should be visible in derived type (RoadType). Errors: {:?}",
        errors
    );
}

/// Test that elements inherited from a base type in a DIFFERENT namespace (via xs:import)
/// are visible in derived types. This reproduces the CityGML issue where:
/// - core:AbstractCityObjectType (defines creationDate) in core namespace
/// - tran:TransportationComplexType extends core:AbstractTransportationObjectType
/// - tran:Road uses tran:RoadType which extends tran:TransportationComplexType
#[test]
fn test_inherited_elements_across_namespaces_via_import() {
    use fastxml::schema::{Schema, Validator};

    // Core schema with AbstractCityObjectType defining creationDate
    let core_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractCityObjectType" abstract="true">
            <xs:sequence>
                <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
                <xs:element name="terminationDate" type="xs:date" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // Transportation schema that imports core and extends AbstractCityObjectType
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"
                   schemaLocation="http://www.opengis.net/citygml/2.0/cityGMLBase.xsd"/>

        <!-- Extends core:AbstractCityObjectType to inherit creationDate -->
        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                        <xs:element name="lod1MultiSurface" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- RoadType extends TransportationComplexType -->
        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="tran:TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- Road element uses RoadType -->
        <xs:element name="Road" type="tran:RoadType"/>
    </xs:schema>"#;

    let schema = Schema::builder().add("http://www.opengis.net/citygml/2.0/cityGMLBase.xsd", core_xsd.as_bytes()).add("http://www.opengis.net/citygml/transportation/2.0/transportation.xsd", tran_xsd.as_bytes()).resolve()
    .expect("Failed to compile schema");

    // Debug: Check what's in the type_children_cache for RoadType
    eprintln!("=== Type children cache contents ===");
    for (type_name, flattened) in &schema.type_children_cache {
        if type_name.contains("Road") || type_name.contains("Transportation") {
            eprintln!(
                "{}: {:?}",
                type_name,
                flattened.constraints.keys().collect::<Vec<_>>()
            );
        }
    }

    // Debug: Check the type structure
    if let Some(fastxml::schema::types::TypeDef::Complex(road_type)) = schema.get_type("RoadType") {
        eprintln!("RoadType content: {:?}", road_type.content);
    }
    if let Some(fastxml::schema::types::TypeDef::Complex(trans_type)) =
        schema.get_type("TransportationComplexType")
    {
        eprintln!(
            "TransportationComplexType content: {:?}",
            trans_type.content
        );
    }

    // Validate a doc where creationDate (from core:AbstractCityObjectType) and
    // class (from TransportationComplexType) are reached via inheritance.
    let xml = r#"<tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0" xmlns:core="http://www.opengis.net/citygml/2.0"><core:creationDate>2024-01-01</core:creationDate><tran:class>9999</tran:class></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validation failed");

    let errors: Vec<_> = report
        .entries()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    // The test should pass - both creationDate (from core namespace) and class should be found
    assert!(
        errors.is_empty(),
        "Elements inherited across namespaces should be visible. Errors: {:?}",
        errors
    );
}

/// Test deep inheritance chain across multiple namespaces (mimics actual CityGML structure)
/// gml:AbstractGMLType -> gml:AbstractFeatureType -> core:AbstractCityObjectType
///   -> tran:AbstractTransportationObjectType -> tran:TransportationComplexType -> tran:RoadType
#[test]
fn test_deep_inheritance_chain_across_namespaces() {
    use fastxml::schema::{Schema, Validator};

    // GML base schema
    let gml_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractGMLType" abstract="true">
            <xs:sequence>
                <xs:element name="description" type="xs:string" minOccurs="0"/>
                <xs:element name="name" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
            <xs:attribute ref="gml:id"/>
        </xs:complexType>

        <xs:attribute name="id" type="xs:ID"/>

        <xs:complexType name="AbstractFeatureType" abstract="true">
            <xs:complexContent>
                <xs:extension base="gml:AbstractGMLType">
                    <xs:sequence>
                        <xs:element name="boundedBy" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;

    // Core CityGML schema
    let core_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/gml"
                   schemaLocation="http://schemas.opengis.net/gml/3.1.1/base/gml.xsd"/>

        <xs:complexType name="AbstractCityObjectType" abstract="true">
            <xs:complexContent>
                <xs:extension base="gml:AbstractFeatureType">
                    <xs:sequence>
                        <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
                        <xs:element name="terminationDate" type="xs:date" minOccurs="0"/>
                        <xs:element name="externalReference" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="cityObjectMember" type="xs:string"/>
    </xs:schema>"#;

    // Transportation schema with deep inheritance
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"
                   schemaLocation="http://www.opengis.net/citygml/2.0/cityGMLBase.xsd"/>
        <xs:import namespace="http://www.opengis.net/gml"
                   schemaLocation="http://schemas.opengis.net/gml/3.1.1/base/gml.xsd"/>

        <!-- Intermediate abstract type -->
        <xs:complexType name="AbstractTransportationObjectType" abstract="true">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <!-- No additional elements here -->
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- TransportationComplexType adds class, function, etc. -->
        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="tran:AbstractTransportationObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                        <xs:element name="usage" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                        <xs:element name="lod1MultiSurface" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <!-- RoadType extends TransportationComplexType -->
        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="tran:TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                        <xs:element name="auxiliaryTrafficArea" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="Road" type="tran:RoadType" substitutionGroup="core:cityObjectMember"/>
    </xs:schema>"#;

    let schema = Schema::builder().add("http://schemas.opengis.net/gml/3.1.1/base/gml.xsd", gml_xsd.as_bytes()).add("http://www.opengis.net/citygml/2.0/cityGMLBase.xsd", core_xsd.as_bytes()).add("http://www.opengis.net/citygml/transportation/2.0/transportation.xsd", tran_xsd.as_bytes()).resolve()
    .expect("Failed to compile schema");

    // Debug: Check type_children_cache contents for RoadType
    eprintln!("=== Deep inheritance test: type_children_cache ===");
    if let Some(flattened) = schema.type_children_cache.get("RoadType") {
        eprintln!(
            "RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    } else {
        eprintln!("RoadType not found in cache!");
    }
    if let Some(flattened) = schema.type_children_cache.get("tran:RoadType") {
        eprintln!(
            "tran:RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    }

    // A Road instance exercising elements reached through the full inheritance
    // chain: boundedBy (gml:AbstractFeatureType, 3 up), creationDate
    // (core:AbstractCityObjectType, 2 up), and class/function/lod1MultiSurface
    // (tran:TransportationComplexType, 1 up).
    let xml = r#"<tran:Road gml:id="road_1" xmlns:tran="http://www.opengis.net/citygml/transportation/2.0" xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:gml="http://www.opengis.net/gml"><gml:boundedBy>envelope</gml:boundedBy><core:creationDate>2024-01-01</core:creationDate><tran:class>9999</tran:class><tran:function>9020</tran:function><tran:lod1MultiSurface>surface</tran:lod1MultiSurface></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validation failed");

    let errors: Vec<_> = report
        .entries()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    // All elements from the deep inheritance chain should be visible
    assert!(
        errors.is_empty(),
        "Elements from deep inheritance chain should be visible. Errors: {:?}",
        errors
    );
}

/// Test that same-namespace inheritance works when base type is referenced without prefix.
///
/// In CityGML schemas, types in the same namespace extend each other without prefix:
/// - RoadType extends TransportationComplexType (not tran:TransportationComplexType)
///
/// This test verifies that the inheritance chain is resolved correctly even when
/// the base type reference has no namespace prefix.
#[test]
fn test_same_namespace_inheritance_without_prefix() {
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};

    // Schema where types extend other types in the same namespace WITHOUT prefix
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <!-- Base type with elements -->
        <xs:complexType name="TransportationComplexType">
            <xs:sequence>
                <xs:element name="class" type="xs:string" minOccurs="0"/>
                <xs:element name="function" type="xs:string" minOccurs="0"/>
                <xs:element name="usage" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <!-- Derived type extends base WITHOUT prefix (same namespace) -->
        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="Road" type="tran:RoadType"/>
    </xs:schema>"#;

    let tran_ast = parse_xsd_ast(tran_xsd.as_bytes()).unwrap();
    let mut compiled = compile_schemas(vec![tran_ast]).expect("Failed to compile schemas");
    register_builtin_types(&mut compiled);

    eprintln!("=== Same-namespace inheritance test ===");

    // Check that types are stored
    eprintln!(
        "Types in schema: {:?}",
        compiled.types.keys().collect::<Vec<_>>()
    );

    // RoadType should inherit elements from TransportationComplexType
    let road_type_cache = compiled.type_children_cache.get("RoadType");
    eprintln!(
        "RoadType cache: {:?}",
        road_type_cache.map(|f| f.constraints.keys().collect::<Vec<_>>())
    );

    let road_type_cache = road_type_cache.expect("RoadType should be in cache");

    // Own element
    assert!(
        road_type_cache.constraints.contains_key("trafficArea"),
        "RoadType should have trafficArea (own element)"
    );

    // Inherited elements from TransportationComplexType (extended WITHOUT prefix)
    assert!(
        road_type_cache.constraints.contains_key("class"),
        "RoadType should have class (inherited from TransportationComplexType)"
    );
    assert!(
        road_type_cache.constraints.contains_key("function"),
        "RoadType should have function (inherited from TransportationComplexType)"
    );
    assert!(
        road_type_cache.constraints.contains_key("usage"),
        "RoadType should have usage (inherited from TransportationComplexType)"
    );
}
