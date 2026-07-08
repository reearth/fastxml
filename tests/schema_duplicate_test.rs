//! Tests for duplicate schema handling and default namespace.

/// Test that duplicate schemas (same schema resolved multiple times) don't break compilation.
/// This reproduces the issue in compile_schema_for_streaming where resolve_all is called
/// for each schema location, potentially adding the same dependency schemas multiple times.
#[test]
fn test_duplicate_schemas_in_compile_schemas() {
    use fastxml::schema::Validator;
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast};

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
        compiled.types_ns.keys().collect::<Vec<_>>()
    );
    if let Some(flattened) = compiled
        .resolve_type_ref_to_ns("tran:RoadType")
        .and_then(|n| compiled.ns_type_children_cache.get(&n))
    {
        eprintln!(
            "RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    }
    if let Some(flattened) = compiled
        .resolve_type_ref_to_ns("tran:RoadType")
        .and_then(|n| compiled.ns_type_children_cache.get(&n))
    {
        eprintln!(
            "tran:RoadType children: {:?}",
            flattened.constraints.keys().collect::<Vec<_>>()
        );
    }

    let xml = r#"<tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0" xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:gml="http://www.opengis.net/gml"><core:creationDate>2024-01-01</core:creationDate><tran:class>9999</tran:class><tran:function>9020</tran:function><tran:lod1MultiSurface>surface</tran:lod1MultiSurface></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(compiled)
        .run()
        .expect("validation failed");

    let errors: Vec<_> = report
        .entries()
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
    use fastxml::schema::Validator;
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};

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
    for type_name in compiled.types_ns.keys() {
        if type_name.local_name.contains("Road")
            || type_name.local_name.contains("Transportation")
            || type_name.local_name.contains("CityObject")
        {
            eprintln!("  {:?}", type_name);
        }
    }

    // Debug: Check the ns-keyed type children cache
    eprintln!("Type children cache:");
    for (type_name, flattened) in &compiled.ns_type_children_cache {
        if type_name.local_name.contains("Road") {
            eprintln!(
                "  {:?}: {:?}",
                type_name,
                flattened.constraints.keys().collect::<Vec<_>>()
            );
        }
    }

    // Debug: Check elements
    eprintln!("Elements:");
    for (elem_name, elem_def) in &compiled.elements_ns {
        if elem_name.local_name.contains("Road") {
            eprintln!("  {:?} -> type_ref: {:?}", elem_name, elem_def.type_ref);
        }
    }

    let xml = r#"<tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0" xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:gml="http://www.opengis.net/gml"><core:creationDate>2024-01-01</core:creationDate><tran:class>9999</tran:class><tran:function>9020</tran:function><tran:lod1MultiSurface>surface</tran:lod1MultiSurface></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(compiled)
        .run()
        .expect("validation failed");

    let errors: Vec<_> = report
        .entries()
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
    let road_type_cache = compiled.get_ns_type_children(
        "http://www.opengis.net/citygml/transportation/2.0",
        "RoadType",
    );
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

    // Local names must be clean identifiers: keys carry the namespace in
    // the NsName, so no prefix can ever leak into the local part (the old
    // double-prefix "gml:tran:RoadType" bug class).
    let bad_local_keys: Vec<_> = compiled
        .ns_type_children_cache
        .keys()
        .filter(|k| k.local_name.contains(':'))
        .collect();
    assert!(
        bad_local_keys.is_empty(),
        "No local name in ns_type_children_cache may contain ':', found: {:?}",
        bad_local_keys
    );

    // Also verify that types are correctly registered under their namespaces
    assert!(
        compiled
            .get_ns_type_children(
                "http://www.opengis.net/citygml/transportation/2.0",
                "TransportationComplexType"
            )
            .is_some(),
        "Should have TransportationComplexType in cache"
    );
    assert!(
        compiled
            .get_ns_type_children(
                "http://www.opengis.net/citygml/2.0",
                "AbstractCityObjectType"
            )
            .is_some(),
        "Should have AbstractCityObjectType in cache"
    );
    assert!(
        compiled
            .get_ns_type_children("http://www.opengis.net/gml", "AbstractFeatureType")
            .is_some(),
        "Should have AbstractFeatureType in cache"
    );
}
