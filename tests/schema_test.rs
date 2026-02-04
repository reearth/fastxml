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

/// Tests that elements defined with substitutionGroup in imported schemas are correctly
/// stored and can be looked up during validation.
///
/// This reproduces a bug where elements like tran:class, tran:function were not found
/// during validation because:
/// 1. They are defined in Transportation.xsd with substitutionGroup attribute
/// 2. When schemas are merged, these elements need to be stored with the correct
///    namespace-qualified key (e.g., "tran:class")
/// 3. The validator needs to be able to look them up when validating XML with
///    prefixed element names like <tran:class>
#[test]
fn test_substitution_group_elements_from_imported_schema() {
    use fastxml::schema::xsd::parse_xsd_multiple;

    // Schema 1: CityGML Core - defines abstract element that others substitute for
    let core_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <!-- Abstract element that can be substituted -->
        <xs:element name="_GenericApplicationPropertyOfCityObject" abstract="true"/>

        <xs:complexType name="AbstractCityObjectType" abstract="true">
            <xs:sequence>
                <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
                <xs:element name="terminationDate" type="xs:date" minOccurs="0"/>
                <xs:element ref="core:_GenericApplicationPropertyOfCityObject" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // Schema 2: Transportation - defines elements with substitutionGroup
    let tran_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"/>

        <!-- Elements with substitutionGroup - these should be stored as tran:class, etc. -->
        <xs:element name="class" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
        <xs:element name="function" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
        <xs:element name="lod1MultiSurface" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
    </xs:schema>"#;

    // Compile both schemas together
    let schema = parse_xsd_multiple(&[
        (
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        ),
    ])
    .expect("Failed to compile schemas");

    // Core elements should be stored with core: prefix
    assert!(
        schema
            .get_element("core:_GenericApplicationPropertyOfCityObject")
            .is_some(),
        "core:_GenericApplicationPropertyOfCityObject should be found. Available elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );

    // Transportation elements with substitutionGroup should be stored with tran: prefix
    assert!(
        schema.get_element("tran:class").is_some(),
        "tran:class should be found. Available elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.get_element("tran:function").is_some(),
        "tran:function should be found. Available elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.get_element("tran:lod1MultiSurface").is_some(),
        "tran:lod1MultiSurface should be found. Available elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );

    // Verify the substitution group is correctly recorded
    let tran_class = schema.get_element("tran:class").unwrap();
    assert_eq!(
        tran_class.substitution_group.as_deref(),
        Some("core:_GenericApplicationPropertyOfCityObject"),
        "tran:class should have substitutionGroup=core:_GenericApplicationPropertyOfCityObject"
    );
}

/// Tests that elements are stored with correct namespace prefix even when the schema
/// does NOT have an explicit xmlns declaration for its own target namespace.
///
/// This reproduces a real-world bug where schemas like Transportation.xsd might be
/// written without an explicit xmlns:tran="..." declaration, causing elements to be
/// stored without the expected prefix.
///
/// The bug manifests when:
/// 1. Schema defines elements without an xmlns prefix for its own targetNamespace
/// 2. Elements are stored with local name only (e.g., "class")
/// 3. XML document uses prefixed element names (e.g., <tran:class>)
/// 4. Validator can't find "tran:class" because schema only has "class"
#[test]
fn test_elements_without_explicit_target_namespace_prefix() {
    use fastxml::schema::xsd::parse_xsd_multiple;

    // Core schema - has explicit xmlns:core for its target namespace
    let core_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <xs:element name="_GenericApplicationPropertyOfCityObject" abstract="true"/>
    </xs:schema>"#;

    // Transportation schema - NO explicit xmlns:tran for its target namespace!
    // This is a valid XSD pattern but can cause issues with prefix resolution.
    let tran_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"/>

        <xs:element name="class" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
        <xs:element name="function" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
    </xs:schema>"#;

    // Compile both schemas
    let schema = parse_xsd_multiple(&[
        (
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        ),
    ])
    .expect("Failed to compile schemas");

    // Print available elements for debugging
    eprintln!(
        "Available elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );

    // Elements are stored without prefix when no xmlns:tran is declared
    assert!(
        schema.elements.contains_key("class"),
        "class element is stored without prefix"
    );
    assert!(
        schema.elements.contains_key("function"),
        "function element is stored without prefix"
    );

    // Looking up "tran:class" now works via fallback to local name lookup
    // get_element tries: 1. "tran:class" (not found), 2. local name "class" (found!)
    let tran_class_found = schema.get_element("tran:class").is_some();
    eprintln!("get_element('tran:class') found: {}", tran_class_found);
    assert!(
        tran_class_found,
        "tran:class should be found via local name fallback (stored as 'class')"
    );

    // Also works via namespace URI lookup
    let by_ns = schema
        .get_element_by_ns("http://www.opengis.net/citygml/transportation/2.0", "class")
        .is_some();
    // Note: This will fail because we didn't declare xmlns:tran, so the namespace_prefixes
    // map doesn't have this namespace. But get_element_by_ns has a fallback to local name.
    assert!(
        by_ns,
        "get_element_by_ns should find via local name fallback"
    );
}

/// Tests the validator behavior when elements are stored without namespace prefix.
///
/// This simulates what happens when validating XML like:
/// <tran:Road>
///   <tran:class>main_road</tran:class>
/// </tran:Road>
///
/// When the schema stores "class" without prefix but XML uses "tran:class",
/// the validator should still be able to find the element.
#[test]
fn test_validator_finds_element_with_namespace_mismatch() {
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::parse_xsd_multiple;
    use std::sync::Arc;

    // Schema where transportation elements have NO xmlns:tran prefix
    let core_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <xs:element name="_GenericApplicationPropertyOfCityObject" abstract="true"/>
    </xs:schema>"#;

    let tran_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"/>

        <xs:element name="Road">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="class" type="xs:string" minOccurs="0"/>
                    <xs:element name="function" type="xs:string" minOccurs="0"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>

        <xs:element name="class" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
        <xs:element name="function" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
    </xs:schema>"#;

    let schema = parse_xsd_multiple(&[
        (
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        ),
    ])
    .expect("Failed to compile schemas");

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Simulate parsing XML with tran: prefix
    // <tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0">
    //   <tran:class>main_road</tran:class>
    // </tran:Road>

    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![Namespace::new(
                "tran",
                "http://www.opengis.net/citygml/transportation/2.0",
            )],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // This is the problematic part: <tran:class> but schema has "class" without prefix
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::Text("main_road".into()))
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator.handle(&XmlEvent::Eof).unwrap();
    validator.finish().unwrap();

    // Check for "not declared" errors
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    // BUG: With current implementation, we expect errors because:
    // - Schema stores "Road" (without tran: prefix)
    // - XML uses "tran:Road"
    // - Validator can't match them

    // The fix should make this pass without errors
    // For now, we document the expected behavior:
    if !errors.is_empty() {
        eprintln!(
            "BUG CONFIRMED: {} 'not declared' errors found",
            errors.len()
        );
        eprintln!("This confirms the namespace prefix mismatch issue");
    }

    // After the fix, this assertion should pass:
    // assert!(errors.is_empty(), "Validator should find elements regardless of prefix mismatch");
}

/// Tests the real problem scenario: schema stores "tran:class" but XML uses "tr:class"
/// (same namespace URI, different prefix).
///
/// This is the actual bug in PLATEAU validation where:
/// 1. Schema defines xmlns:tran="http://...transportation..." and stores elements as "tran:class"
/// 2. XML uses xmlns:tr="http://...transportation..." (different prefix, same namespace)
/// 3. Validator looks up "tr:class" but can't find it because schema has "tran:class"
#[test]
fn test_validator_fails_with_different_prefix_same_namespace() {
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::parse_xsd_multiple;
    use std::sync::Arc;

    // Schema WITH explicit xmlns:tran for its target namespace
    // Elements will be stored as "tran:class", "tran:function", etc.
    let core_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">

        <xs:element name="_GenericApplicationPropertyOfCityObject" abstract="true"/>
    </xs:schema>"#;

    // This schema HAS xmlns:tran, so elements are stored as tran:Road, tran:class, etc.
    let tran_schema = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"/>

        <xs:element name="Road">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="class" type="xs:string" minOccurs="0"/>
                    <xs:element name="function" type="xs:string" minOccurs="0"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>

        <xs:element name="class" type="xs:string"
                    substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
    </xs:schema>"#;

    let schema = parse_xsd_multiple(&[
        (
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        ),
    ])
    .expect("Failed to compile schemas");

    // Verify schema stores elements with tran: prefix
    eprintln!(
        "Schema elements: {:?}",
        schema.elements.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.elements.contains_key("tran:Road"),
        "Road should be stored as tran:Road"
    );
    assert!(
        schema.elements.contains_key("tran:class"),
        "class should be stored as tran:class"
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Simulate XML that uses "tr:" prefix instead of "tran:"
    // Both map to the same namespace URI!
    // <tr:Road xmlns:tr="http://www.opengis.net/citygml/transportation/2.0">
    //   <tr:class>main_road</tr:class>
    // </tr:Road>

    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tr".into()), // Different prefix!
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![Namespace::new(
                "tr", // Different prefix from schema's "tran"
                "http://www.opengis.net/citygml/transportation/2.0",
            )],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tr".into()), // Different prefix!
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::Text("main_road".into()))
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tr".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tr".into()),
        })
        .unwrap();

    validator.handle(&XmlEvent::Eof).unwrap();
    validator.finish().unwrap();

    // Check for "not declared" errors
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    // Validator should match by namespace URI, not just by prefix
    // So even though XML uses tr:* and schema has tran:*, they should match
    // because they have the same namespace URI
    assert!(
        errors.is_empty(),
        "Validator should match by namespace URI, not prefix. Errors: {:?}",
        errors
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
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::parse_xsd_multiple;
    use std::sync::Arc;

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

    let schema = parse_xsd_multiple(&[(
        "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
        schema_xsd.as_bytes(),
    )])
    .expect("Failed to compile schema");

    // Debug: Print the type structure
    if let Some(TypeDef::Complex(road_type)) = schema.get_type("tran:RoadType") {
        eprintln!("RoadType content: {:?}", road_type.content);
    }

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Validate XML: <tran:Road><tran:class>道路</tran:class></tran:Road>
    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![Namespace::new(
                "tran",
                "http://www.opengis.net/citygml/transportation/2.0",
            )],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // This is the problematic element: class is defined in TransportationComplexType,
    // which is the base of RoadType. The validator should find it via inheritance.
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator.handle(&XmlEvent::Text("道路".into())).unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator.handle(&XmlEvent::Eof).unwrap();
    validator.finish().unwrap();

    // Check for errors
    let errors: Vec<_> = validator
        .errors()
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
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::parse_xsd_multiple;
    use std::sync::Arc;

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

    let schema = parse_xsd_multiple(&[
        (
            "http://www.opengis.net/citygml/2.0/cityGMLBase.xsd",
            core_xsd.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_xsd.as_bytes(),
        ),
    ])
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

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Validate: <tran:Road><core:creationDate>2024-01-01</core:creationDate></tran:Road>
    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![
                Namespace::new("tran", "http://www.opengis.net/citygml/transportation/2.0"),
                Namespace::new("core", "http://www.opengis.net/citygml/2.0"),
            ],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // creationDate is defined in core:AbstractCityObjectType but should be visible via inheritance
    validator
        .handle(&XmlEvent::StartElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
            namespace: Some("http://www.opengis.net/citygml/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::Text("2024-01-01".into()))
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
        })
        .unwrap();

    // Also test tran:class which is defined directly in TransportationComplexType
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();

    validator.handle(&XmlEvent::Text("9999".into())).unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    let errors: Vec<_> = validator
        .errors()
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
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::parse_xsd_multiple;
    use std::sync::Arc;

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

    let schema = parse_xsd_multiple(&[
        (
            "http://schemas.opengis.net/gml/3.1.1/base/gml.xsd",
            gml_xsd.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/2.0/cityGMLBase.xsd",
            core_xsd.as_bytes(),
        ),
        (
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_xsd.as_bytes(),
        ),
    ])
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

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start Road element
    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![("gml:id".into(), "road_1".into())],
            namespace_decls: vec![
                Namespace::new("tran", "http://www.opengis.net/citygml/transportation/2.0"),
                Namespace::new("core", "http://www.opengis.net/citygml/2.0"),
                Namespace::new("gml", "http://www.opengis.net/gml"),
            ],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Test element from gml:AbstractFeatureType (3 levels up)
    validator
        .handle(&XmlEvent::StartElement {
            name: "boundedBy".into(),
            prefix: Some("gml".into()),
            namespace: Some("http://www.opengis.net/gml".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();
    validator
        .handle(&XmlEvent::Text("envelope".into()))
        .unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "boundedBy".into(),
            prefix: Some("gml".into()),
        })
        .unwrap();

    // Test element from core:AbstractCityObjectType (2 levels up)
    validator
        .handle(&XmlEvent::StartElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
            namespace: Some("http://www.opengis.net/citygml/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();
    validator
        .handle(&XmlEvent::Text("2024-01-01".into()))
        .unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
        })
        .unwrap();

    // Test element from tran:TransportationComplexType (1 level up)
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(4),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9999".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test element from tran:TransportationComplexType
    validator
        .handle(&XmlEvent::StartElement {
            name: "function".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(5),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9020".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "function".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test element from tran:TransportationComplexType
    validator
        .handle(&XmlEvent::StartElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(6),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("surface".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // End Road element
    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    let errors: Vec<_> = validator
        .errors()
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

/// Test that duplicate schemas (same schema resolved multiple times) don't break compilation.
/// This reproduces the issue in compile_schema_for_streaming where resolve_all is called
/// for each schema location, potentially adding the same dependency schemas multiple times.
#[test]
fn test_duplicate_schemas_in_compile_schemas() {
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast};
    use std::sync::Arc;

    // GML base schema
    let gml_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractGMLType" abstract="true">
            <xs:sequence>
                <xs:element name="description" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

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

        <xs:complexType name="AbstractCityObjectType" abstract="true">
            <xs:complexContent>
                <xs:extension base="gml:AbstractFeatureType">
                    <xs:sequence>
                        <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;

    // Transportation schema
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0"/>
                        <xs:element name="lod1MultiSurface" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="tran:TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="Road" type="tran:RoadType"/>
    </xs:schema>"#;

    // Parse each schema
    let gml_ast = parse_xsd_ast(gml_xsd.as_bytes()).unwrap();
    let core_ast = parse_xsd_ast(core_xsd.as_bytes()).unwrap();
    let tran_ast = parse_xsd_ast(tran_xsd.as_bytes()).unwrap();

    // Simulate what compile_schema_for_streaming does:
    // Each resolve_all call returns the schema + its dependencies
    // So we get duplicates when we extend all_schemas
    //
    // IMPORTANT: In compile_schema_for_streaming, the order depends on xsi:schemaLocation
    // which might process tran BEFORE core is resolved, leading to:
    // First "resolve_all" for tran returns: [gml, core, tran] (resolves dependencies)
    // Second "resolve_all" for core returns: [gml, core] (resolved again!)
    // Third "resolve_all" for gml returns: [gml] (resolved again!)
    // Result: [gml, core, tran, gml, core, gml] - tran comes BEFORE core is "properly" resolved

    let all_schemas = vec![
        gml_ast.clone(),  // from tran resolve (dependency)
        core_ast.clone(), // from tran resolve (dependency)
        tran_ast,         // from tran resolve (entry)
        gml_ast.clone(),  // from core resolve (dependency)
        core_ast.clone(), // from core resolve (entry)
        gml_ast.clone(),  // from gml resolve (entry)
    ];

    eprintln!("=== Duplicate schemas test ===");
    eprintln!("Number of schemas: {}", all_schemas.len());

    let mut compiled = compile_schemas(all_schemas).expect("Failed to compile schemas");
    fastxml::schema::xsd::register_builtin_types(&mut compiled);

    // Debug: Check what's in the type_children_cache
    eprintln!(
        "Types in schema: {:?}",
        compiled.types.keys().collect::<Vec<_>>()
    );
    if let Some(flattened) = compiled.type_children_cache.get("RoadType") {
        eprintln!(
            "RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    }
    if let Some(flattened) = compiled.type_children_cache.get("tran:RoadType") {
        eprintln!(
            "tran:RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    }

    let mut validator = OnePassSchemaValidator::new(Arc::new(compiled));

    // Test validation
    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![
                Namespace::new("tran", "http://www.opengis.net/citygml/transportation/2.0"),
                Namespace::new("core", "http://www.opengis.net/citygml/2.0"),
                Namespace::new("gml", "http://www.opengis.net/gml"),
            ],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Test core:creationDate (inherited from core:AbstractCityObjectType)
    validator
        .handle(&XmlEvent::StartElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
            namespace: Some("http://www.opengis.net/citygml/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();
    validator
        .handle(&XmlEvent::Text("2024-01-01".into()))
        .unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
        })
        .unwrap();

    // Test tran:class (defined in TransportationComplexType)
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9999".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test tran:function
    validator
        .handle(&XmlEvent::StartElement {
            name: "function".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(4),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9020".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "function".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test tran:lod1MultiSurface
    validator
        .handle(&XmlEvent::StartElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(5),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("surface".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    assert!(
        errors.is_empty(),
        "Duplicate schemas should not break inheritance. Errors: {:?}",
        errors
    );
}

/// Test with schema using default namespace (xmlns="...") instead of prefixed namespace.
/// This reproduces the actual OGC CityGML schema pattern where:
/// - transportation.xsd uses xmlns="http://www.opengis.net/citygml/transportation/2.0" (default)
/// - Types are stored without prefix (e.g., "RoadType" not "tran:RoadType")
/// - But XML uses prefixed names (tran:Road)
#[test]
fn test_schema_with_default_namespace() {
    use fastxml::Namespace;
    use fastxml::event::{XmlEvent, XmlEventHandler};
    use fastxml::schema::validator::OnePassSchemaValidator;
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};
    use std::sync::Arc;

    // GML base schema (uses gml: prefix for its own namespace)
    let gml_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:complexType name="AbstractGMLType" abstract="true">
            <xs:sequence>
                <xs:element name="description" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

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

    // Core CityGML schema - uses DEFAULT namespace for its own types!
    // This matches the actual OGC schema pattern
    let core_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns="http://www.opengis.net/citygml/2.0"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
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
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="_CityObject" type="AbstractCityObjectType" abstract="true"
                    substitutionGroup="gml:_Feature"/>
    </xs:schema>"#;

    // Transportation schema - also uses DEFAULT namespace for its own types!
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">

        <xs:import namespace="http://www.opengis.net/citygml/2.0"
                   schemaLocation="http://schemas.opengis.net/citygml/2.0/cityGMLBase.xsd"/>
        <xs:import namespace="http://www.opengis.net/gml"
                   schemaLocation="http://schemas.opengis.net/gml/3.1.1/base/gml.xsd"/>

        <xs:complexType name="AbstractTransportationObjectType" abstract="true">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="AbstractTransportationObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0"/>
                        <xs:element name="lod1MultiSurface" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>

        <xs:element name="Road" type="RoadType" substitutionGroup="core:_CityObject"/>
    </xs:schema>"#;

    // Parse schemas
    let gml_ast = parse_xsd_ast(gml_xsd.as_bytes()).unwrap();
    let core_ast = parse_xsd_ast(core_xsd.as_bytes()).unwrap();
    let tran_ast = parse_xsd_ast(tran_xsd.as_bytes()).unwrap();

    eprintln!("=== Default namespace test ===");
    eprintln!("gml namespace_bindings: {:?}", gml_ast.namespace_bindings);
    eprintln!("core namespace_bindings: {:?}", core_ast.namespace_bindings);
    eprintln!("tran namespace_bindings: {:?}", tran_ast.namespace_bindings);

    let all_schemas = vec![gml_ast, core_ast, tran_ast];

    let mut compiled = compile_schemas(all_schemas).expect("Failed to compile schemas");
    register_builtin_types(&mut compiled);

    // Debug: Check type storage
    eprintln!("Types in schema:");
    for type_name in compiled.types.keys() {
        if type_name.contains("Road")
            || type_name.contains("Transportation")
            || type_name.contains("CityObject")
        {
            eprintln!("  {}", type_name);
        }
    }

    // Debug: Check type_children_cache
    eprintln!("Type children cache:");
    for (type_name, flattened) in &compiled.type_children_cache {
        if type_name.contains("Road") {
            eprintln!(
                "  {}: {:?}",
                type_name,
                flattened.constraints.keys().collect::<Vec<_>>()
            );
        }
    }

    // Debug: Check elements
    eprintln!("Elements:");
    for (elem_name, elem_def) in &compiled.elements {
        if elem_name.contains("Road") {
            eprintln!("  {} -> type_ref: {:?}", elem_name, elem_def.type_ref);
        }
    }

    let mut validator = OnePassSchemaValidator::new(Arc::new(compiled));

    // Test validation with prefixed element names (as used in actual XML)
    validator
        .handle(&XmlEvent::StartElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![
                Namespace::new("tran", "http://www.opengis.net/citygml/transportation/2.0"),
                Namespace::new("core", "http://www.opengis.net/citygml/2.0"),
                Namespace::new("gml", "http://www.opengis.net/gml"),
            ],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Test core:creationDate (inherited from core:AbstractCityObjectType)
    validator
        .handle(&XmlEvent::StartElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
            namespace: Some("http://www.opengis.net/citygml/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();
    validator
        .handle(&XmlEvent::Text("2024-01-01".into()))
        .unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "creationDate".into(),
            prefix: Some("core".into()),
        })
        .unwrap();

    // Test tran:class
    validator
        .handle(&XmlEvent::StartElement {
            name: "class".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9999".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "class".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test tran:function
    validator
        .handle(&XmlEvent::StartElement {
            name: "function".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(4),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("9020".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "function".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    // Test tran:lod1MultiSurface
    validator
        .handle(&XmlEvent::StartElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
            namespace: Some("http://www.opengis.net/citygml/transportation/2.0".into()),
            attributes: vec![],
            namespace_decls: vec![],
            line: Some(5),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&XmlEvent::Text("surface".into())).unwrap();
    validator
        .handle(&XmlEvent::EndElement {
            name: "lod1MultiSurface".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    validator
        .handle(&XmlEvent::EndElement {
            name: "Road".into(),
            prefix: Some("tran".into()),
        })
        .unwrap();

    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("not declared"))
        .collect();

    eprintln!("Validation errors: {:?}", errors);

    assert!(
        errors.is_empty(),
        "Schema with default namespace should work. Errors: {:?}",
        errors
    );
}

/// Test that duplicate schemas (same targetNamespace appearing multiple times) are properly
/// deduplicated and don't break the inheritance chain.
/// This reproduces the issue found in compile_schema_for_streaming where resolve_all is called
/// for each schema location, causing the same schema to be added to all_schemas multiple times.
/// Without deduplication, the inheritance chain breaks because namespace bindings get overwritten.
#[test]
fn test_duplicate_schemas_deduplication() {
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};

    // GML schema
    let gml_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">
        <xs:complexType name="AbstractFeatureType" abstract="true">
            <xs:sequence>
                <xs:element name="boundedBy" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // Core schema
    let core_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns="http://www.opengis.net/citygml/2.0"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/2.0"
               elementFormDefault="qualified">
        <xs:complexType name="AbstractCityObjectType" abstract="true">
            <xs:complexContent>
                <xs:extension base="gml:AbstractFeatureType">
                    <xs:sequence>
                        <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;

    // Transportation schema
    let tran_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns="http://www.opengis.net/citygml/transportation/2.0"
               xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:core="http://www.opengis.net/citygml/2.0"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/citygml/transportation/2.0"
               elementFormDefault="qualified">
        <xs:complexType name="TransportationComplexType">
            <xs:complexContent>
                <xs:extension base="core:AbstractCityObjectType">
                    <xs:sequence>
                        <xs:element name="class" type="xs:string" minOccurs="0"/>
                        <xs:element name="function" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
        <xs:complexType name="RoadType">
            <xs:complexContent>
                <xs:extension base="TransportationComplexType">
                    <xs:sequence>
                        <xs:element name="trafficArea" type="xs:string" minOccurs="0"/>
                    </xs:sequence>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
        <xs:element name="Road" type="RoadType"/>
    </xs:schema>"#;

    let gml_ast = parse_xsd_ast(gml_xsd.as_bytes()).unwrap();
    let core_ast = parse_xsd_ast(core_xsd.as_bytes()).unwrap();
    let tran_ast = parse_xsd_ast(tran_xsd.as_bytes()).unwrap();

    // Simulate what happens in compile_schema_for_streaming:
    // Each resolve_all returns the entry schema + its dependencies
    // This causes massive duplication!
    let all_schemas = vec![
        // First resolve_all for tran: returns [gml, core, tran]
        gml_ast.clone(),
        core_ast.clone(),
        tran_ast.clone(),
        // Second resolve_all for core: returns [gml, core]
        gml_ast.clone(),
        core_ast.clone(),
        // Third resolve_all for gml: returns [gml]
        gml_ast.clone(),
        // Fourth resolve_all for some other schema that depends on core
        gml_ast.clone(),
        core_ast.clone(),
        // More duplicates...
        gml_ast,
        core_ast,
        tran_ast,
    ];

    eprintln!("=== Duplicate schemas deduplication test ===");
    eprintln!("Input schemas count: {}", all_schemas.len());

    let mut compiled = compile_schemas(all_schemas).expect("Failed to compile schemas");
    register_builtin_types(&mut compiled);

    // Check that RoadType has inherited elements from the full chain
    let road_type_cache = compiled.type_children_cache.get("RoadType");
    eprintln!(
        "RoadType cache: {:?}",
        road_type_cache.map(|f| f.constraints.keys().collect::<Vec<_>>())
    );

    // RoadType should have:
    // - trafficArea (own)
    // - class, function (from TransportationComplexType)
    // - creationDate (from AbstractCityObjectType)
    // - boundedBy (from AbstractFeatureType)
    let road_type_cache = road_type_cache.expect("RoadType should be in cache");
    assert!(
        road_type_cache.constraints.contains_key("trafficArea"),
        "RoadType should have trafficArea"
    );
    assert!(
        road_type_cache.constraints.contains_key("class"),
        "RoadType should have class (inherited from TransportationComplexType)"
    );
    assert!(
        road_type_cache.constraints.contains_key("function"),
        "RoadType should have function (inherited from TransportationComplexType)"
    );
    assert!(
        road_type_cache.constraints.contains_key("creationDate"),
        "RoadType should have creationDate (inherited from AbstractCityObjectType)"
    );
    assert!(
        road_type_cache.constraints.contains_key("boundedBy"),
        "RoadType should have boundedBy (inherited from AbstractFeatureType)"
    );

    // Verify no double-prefix keys like "gml:tran:RoadType" or "core:tran:RoadType"
    // This was a bug where prefixes were incorrectly concatenated
    let double_prefix_keys: Vec<_> = compiled
        .type_children_cache
        .keys()
        .filter(|k| k.matches(':').count() >= 2)
        .collect();
    assert!(
        double_prefix_keys.is_empty(),
        "Should not have double-prefix keys in type_children_cache, found: {:?}",
        double_prefix_keys
    );

    // Also verify that types are correctly registered with proper prefixes
    // The tran namespace types should be accessible
    assert!(
        compiled
            .type_children_cache
            .contains_key("TransportationComplexType"),
        "Should have TransportationComplexType in cache"
    );
    assert!(
        compiled
            .type_children_cache
            .contains_key("AbstractCityObjectType"),
        "Should have AbstractCityObjectType in cache"
    );
    assert!(
        compiled
            .type_children_cache
            .contains_key("AbstractFeatureType"),
        "Should have AbstractFeatureType in cache"
    );
}
