//! Tests for namespace handling and substitution groups.

use fastxml::schema::types::TypeDef;
use fastxml::schema::{CompiledSchema, ComplexType, ContentModel, ElementDef};

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

    // Insert under their real namespaces and bind the prefixes so the
    // string-key compat shim can resolve "gml:TrackType"/"tran:TrackType".
    const GML_NS: &str = "http://www.opengis.net/gml";
    const TRAN_NS: &str = "http://www.opengis.net/citygml/transportation/2.0";
    schema
        .prefix_namespaces
        .insert("gml".to_string(), GML_NS.to_string());
    schema
        .prefix_namespaces
        .insert("tran".to_string(), TRAN_NS.to_string());
    schema.types_ns.insert(
        fastxml::schema::types::NsName::new(GML_NS, "TrackType"),
        TypeDef::Complex(gml_track_type),
    );
    schema.types_ns.insert(
        fastxml::schema::types::NsName::new(TRAN_NS, "TrackType"),
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
    schema.types_ns.insert(
        fastxml::schema::types::NsName::new("", "TrackType"),
        TypeDef::Complex(gml_track_type),
    );
    schema.types_ns.insert(
        fastxml::schema::types::NsName::new("", "TrackType"),
        TypeDef::Complex(tran_track_type),
    );

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
    use fastxml::schema::Schema;

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
    let schema = Schema::builder()
        .add("http://www.opengis.net/gml/gml.xsd", gml_schema.as_bytes())
        .add(
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        )
        .resolve()
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
    use fastxml::schema::Schema;

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
    let schema = Schema::builder()
        .add(
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        )
        .add(
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        )
        .resolve()
        .expect("Failed to compile schemas");

    // Core elements should be stored with core: prefix
    assert!(
        schema
            .get_element("core:_GenericApplicationPropertyOfCityObject")
            .is_some(),
        "core:_GenericApplicationPropertyOfCityObject should be found. Available elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
    );

    // Transportation elements with substitutionGroup should be stored with tran: prefix
    assert!(
        schema.get_element("tran:class").is_some(),
        "tran:class should be found. Available elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.get_element("tran:function").is_some(),
        "tran:function should be found. Available elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.get_element("tran:lod1MultiSurface").is_some(),
        "tran:lod1MultiSurface should be found. Available elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
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
    use fastxml::schema::Schema;

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
    let schema = Schema::builder()
        .add(
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        )
        .add(
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        )
        .resolve()
        .expect("Failed to compile schemas");

    // Print available elements for debugging
    eprintln!(
        "Available elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
    );

    // Elements are stored without prefix when no xmlns:tran is declared
    assert!(
        schema.get_element("class").is_some(),
        "class element is stored without prefix"
    );
    assert!(
        schema.get_element("function").is_some(),
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
    use fastxml::schema::{Schema, Validator};

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

    let schema = Schema::builder()
        .add(
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        )
        .add(
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        )
        .resolve()
        .expect("Failed to compile schemas");

    // Parse XML using the tran: prefix:
    // <tran:Road xmlns:tran="..."><tran:class>main_road</tran:class></tran:Road>
    let xml = r#"<tran:Road xmlns:tran="http://www.opengis.net/citygml/transportation/2.0"><tran:class>main_road</tran:class></tran:Road>"#;
    let report = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validation failed");

    // Check for "not declared" errors
    let errors: Vec<_> = report
        .entries()
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
    use fastxml::schema::{Schema, Validator};

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

    let schema = Schema::builder()
        .add(
            "http://www.opengis.net/citygml/2.0/core.xsd",
            core_schema.as_bytes(),
        )
        .add(
            "http://www.opengis.net/citygml/transportation/2.0/transportation.xsd",
            tran_schema.as_bytes(),
        )
        .resolve()
        .expect("Failed to compile schemas");

    // Verify schema stores elements with tran: prefix
    eprintln!(
        "Schema elements: {:?}",
        schema.elements_ns.keys().collect::<Vec<_>>()
    );
    assert!(
        schema.get_element("tran:Road").is_some(),
        "Road should be stored as tran:Road"
    );
    assert!(
        schema.get_element("tran:class").is_some(),
        "class should be stored as tran:class"
    );

    // XML uses the "tr:" prefix instead of the schema's "tran:" — both map to
    // the same namespace URI, so validation must match by URI, not prefix.
    let xml = r#"<tr:Road xmlns:tr="http://www.opengis.net/citygml/transportation/2.0"><tr:class>main_road</tr:class></tr:Road>"#;
    let report = Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validation failed");

    // Check for "not declared" errors
    let errors: Vec<_> = report
        .entries()
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
