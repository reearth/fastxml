//! Integration tests for schema validation.

use fastxml::schema::{
    CompiledSchema, ComplexType, ContentModel, ElementDef, InMemoryStore, SchemaStore,
    TempDirStore, types::TypeDef,
};

#[test]
fn test_tempdir_store() {
    let store = TempDirStore::new().unwrap();

    let uri = "http://example.com/schema.xsd";
    let content = b"<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"/>";

    // Store schema
    store.put(uri, content).unwrap();
    assert!(store.contains(uri));

    // Retrieve schema
    let retrieved = store.get(uri).unwrap().unwrap();
    assert_eq!(retrieved, content);

    // Get path
    let path = store.resolve_path(uri).unwrap();
    assert!(path.exists());

    // List schemas
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);

    // Remove schema
    assert!(store.remove(uri).unwrap());
    assert!(!store.contains(uri));
}

#[test]
fn test_memory_store() {
    let store = InMemoryStore::new();

    let uri = "http://example.com/schema.xsd";
    let content = b"<schema/>";

    store.put(uri, content).unwrap();
    assert!(store.contains(uri));
    assert_eq!(store.len(), 1);

    let retrieved = store.get(uri).unwrap().unwrap();
    assert_eq!(retrieved, content);

    store.clear().unwrap();
    assert!(store.is_empty());
}

#[test]
fn test_compiled_schema() {
    let mut schema = CompiledSchema::new();

    // Add an element
    let elem = ElementDef::new("Building")
        .with_type("BuildingType")
        .optional();
    schema.elements.insert("Building".to_string(), elem);

    // Verify lookup
    let found = schema.get_element("Building");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Building");
    assert_eq!(found.unwrap().min_occurs, 0);
}

#[test]
fn test_element_def() {
    let elem = ElementDef::new("TestElement")
        .with_type("xs:string")
        .with_occurs(0, None)
        .optional()
        .unbounded();

    assert_eq!(elem.name, "TestElement");
    assert_eq!(elem.type_ref, Some("xs:string".to_string()));
    assert_eq!(elem.min_occurs, 0);
    assert_eq!(elem.max_occurs, None); // unbounded
}

#[test]
fn test_complex_type() {
    let child1 = ElementDef::new("child1").with_type("xs:string");
    let child2 = ElementDef::new("child2").with_type("xs:integer");

    let complex = ComplexType::sequence("MyType", vec![child1, child2]);

    assert_eq!(complex.name, "MyType");
    if let ContentModel::Sequence(elements) = &complex.content {
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].name, "child1");
        assert_eq!(elements[1].name, "child2");
    } else {
        panic!("expected sequence content");
    }
}

#[test]
fn test_schema_with_namespace() {
    let schema = CompiledSchema::with_namespace("http://example.com/ns");
    assert_eq!(
        schema.target_namespace,
        Some("http://example.com/ns".to_string())
    );
}

#[test]
fn test_store_multiple_schemas() {
    let store = TempDirStore::new().unwrap();

    let schemas = vec![
        ("http://a.com/1.xsd", "schema1"),
        ("http://b.com/2.xsd", "schema2"),
        ("http://c.com/3.xsd", "schema3"),
    ];

    for (uri, content) in &schemas {
        store.put(uri, content.as_bytes()).unwrap();
    }

    assert_eq!(store.list().unwrap().len(), 3);

    for (uri, content) in &schemas {
        let retrieved = store.get(uri).unwrap().unwrap();
        assert_eq!(String::from_utf8(retrieved).unwrap(), *content);
    }
}

/// Tests that types with the same local name but different namespace prefixes
/// are correctly distinguished when resolved.
///
/// This tests the scenario where:
/// - gml:TrackType (from GML dynamicFeature.xsd) requires MovingObjectStatus
/// - tran:TrackType (from CityGML transportation.xsd) extends TransportationComplexType
///
/// Both are named "TrackType" but should be stored and retrieved separately.
#[test]
fn test_namespace_qualified_type_resolution() {
    let mut schema = CompiledSchema::new();

    // Create gml:TrackType - requires MovingObjectStatus child
    let gml_track_type = ComplexType::sequence(
        "gml:TrackType",
        vec![ElementDef::new("MovingObjectStatus").with_type("gml:MovingObjectStatusType")],
    );

    // Create tran:TrackType - extends TransportationComplexType, no MovingObjectStatus
    let tran_track_type = ComplexType::sequence(
        "tran:TrackType",
        vec![
            ElementDef::new("class")
                .with_type("gml:CodeType")
                .optional(),
            ElementDef::new("function")
                .with_type("gml:CodeType")
                .optional(),
        ],
    );

    // Insert with namespace-qualified names
    schema.types.insert(
        "gml:TrackType".to_string(),
        TypeDef::Complex(gml_track_type),
    );
    schema.types.insert(
        "tran:TrackType".to_string(),
        TypeDef::Complex(tran_track_type),
    );

    // Verify gml:TrackType is retrieved correctly
    let gml_type = schema.get_type("gml:TrackType");
    assert!(gml_type.is_some(), "gml:TrackType should be found");
    if let Some(TypeDef::Complex(complex)) = gml_type {
        if let ContentModel::Sequence(elements) = &complex.content {
            assert_eq!(elements.len(), 1);
            assert_eq!(elements[0].name, "MovingObjectStatus");
        } else {
            panic!("expected sequence content for gml:TrackType");
        }
    }

    // Verify tran:TrackType is retrieved correctly
    let tran_type = schema.get_type("tran:TrackType");
    assert!(tran_type.is_some(), "tran:TrackType should be found");
    if let Some(TypeDef::Complex(complex)) = tran_type {
        if let ContentModel::Sequence(elements) = &complex.content {
            assert_eq!(elements.len(), 2);
            assert_eq!(elements[0].name, "class");
            assert_eq!(elements[1].name, "function");
        } else {
            panic!("expected sequence content for tran:TrackType");
        }
    }

    // The two types should NOT be confused
    assert_ne!(
        schema.get_type("gml:TrackType").map(|t| format!("{:?}", t)),
        schema
            .get_type("tran:TrackType")
            .map(|t| format!("{:?}", t)),
        "gml:TrackType and tran:TrackType should be different"
    );
}

/// Tests that when types with the same local name exist, get_type falls back
/// correctly and doesn't return the wrong type.
///
/// This simulates a bug where both gml:TrackType and tran:TrackType were stored
/// as just "TrackType", and the wrong one was returned.
#[test]
fn test_type_fallback_with_same_local_name() {
    let mut schema = CompiledSchema::new();

    // Simulate what happens when types are stored without namespace prefix
    // (which might happen in some scenarios)
    let gml_track_type = ComplexType::sequence(
        "TrackType", // stored without gml: prefix
        vec![ElementDef::new("MovingObjectStatus").with_type("MovingObjectStatusType")],
    );

    let tran_track_type = ComplexType::sequence(
        "TrackType", // same local name!
        vec![ElementDef::new("class").with_type("CodeType").optional()],
    );

    // If both are stored with same key "TrackType", last one wins
    schema
        .types
        .insert("TrackType".to_string(), TypeDef::Complex(gml_track_type));
    schema
        .types
        .insert("TrackType".to_string(), TypeDef::Complex(tran_track_type));

    // Only tran:TrackType should exist now (it was inserted last)
    let found = schema.get_type("TrackType");
    assert!(found.is_some());
    if let Some(TypeDef::Complex(complex)) = found
        && let ContentModel::Sequence(elements) = &complex.content
    {
        // Should be tran's "class", not gml's "MovingObjectStatus"
        assert_eq!(elements[0].name, "class");
    }

    // This demonstrates the problem - if we want gml:TrackType, we can't get it!
    // The solution is to always store with namespace-qualified names.
}

/// Tests that the XSD compiler stores types with namespace-qualified names
/// when compiling multiple schemas with types that have the same local name.
///
/// This is the actual bug test: when GML and CityGML both define "TrackType",
/// they should be stored as "gml:TrackType" and "tran:TrackType" respectively.
#[test]
fn test_xsd_compiler_namespace_qualified_types() {
    use fastxml::schema::xsd::parse_xsd_multiple;

    // Schema 1: GML-like schema with TrackType requiring MovingObjectStatus
    let gml_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:complexType name="TrackType">
            <xs:sequence>
                <xs:element name="MovingObjectStatus" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>

        <xs:element name="track" type="gml:TrackType"/>
    </xs:schema>"#;

    // Schema 2: CityGML transportation-like schema with TrackType having class/function
    let tran_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/gml"/>

        <xs:complexType name="TrackType">
            <xs:sequence>
                <xs:element name="class" type="xs:string" minOccurs="0"/>
                <xs:element name="function" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:element name="Track" type="tran:TrackType"/>
    </xs:schema>"#;

    // Compile both schemas together
    let schema = parse_xsd_multiple(&[
        ("http://www.opengis.net/gml/gml.xsd", gml_schema.as_bytes()),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        ),
    ])
    .expect("Failed to compile schemas");

    // Both types should be accessible with their namespace-qualified names
    let gml_type = schema.get_type("gml:TrackType");
    let tran_type = schema.get_type("tran:TrackType");

    assert!(
        gml_type.is_some(),
        "gml:TrackType should be found in compiled schema"
    );
    assert!(
        tran_type.is_some(),
        "tran:TrackType should be found in compiled schema"
    );

    // Verify gml:TrackType has MovingObjectStatus child
    if let Some(TypeDef::Complex(complex)) = gml_type {
        if let ContentModel::Sequence(elements) = &complex.content {
            assert!(
                elements.iter().any(|e| e.name == "MovingObjectStatus"),
                "gml:TrackType should have MovingObjectStatus child, got: {:?}",
                elements.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
        } else {
            panic!(
                "gml:TrackType should have sequence content, got: {:?}",
                complex.content
            );
        }
    } else {
        panic!(
            "gml:TrackType should be a complex type, got: {:?}",
            gml_type
        );
    }

    // Verify tran:TrackType has class/function children
    if let Some(TypeDef::Complex(complex)) = tran_type {
        if let ContentModel::Sequence(elements) = &complex.content {
            assert!(
                elements.iter().any(|e| e.name == "class"),
                "tran:TrackType should have class child, got: {:?}",
                elements.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
        } else {
            panic!(
                "tran:TrackType should have sequence content, got: {:?}",
                complex.content
            );
        }
    } else {
        panic!(
            "tran:TrackType should be a complex type, got: {:?}",
            tran_type
        );
    }

    // The two types should be different
    assert_ne!(
        format!("{:?}", gml_type),
        format!("{:?}", tran_type),
        "gml:TrackType and tran:TrackType should be different types"
    );
}

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
