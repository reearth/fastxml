//! Tests for schema import and include handling.

/// Test that GML elements from xs:include chain are properly compiled.
///
/// GML 3.1.1 splits elements across multiple XSD files:
/// - gml.xsd (root) includes feature.xsd, geometry*.xsd, etc.
/// - boundedBy → feature.xsd
/// - Envelope, lowerCorner, upperCorner, posList → geometryBasic0d1d.xsd
/// - Polygon, LinearRing, exterior → geometryBasic2d.xsd
/// - MultiSurface, surfaceMember → geometryAggregates.xsd
///
/// This test verifies that elements from included schemas are all available
/// after compilation.
#[test]
fn test_gml_elements_from_include_chain() {
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};

    // Simulating GML 3.1.1 structure with xs:include chain

    // geometryBasic0d1d.xsd - Envelope, lowerCorner, upperCorner, posList
    let geometry_basic_0d1d_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:element name="Envelope" type="gml:EnvelopeType"/>
        <xs:element name="lowerCorner" type="gml:DirectPositionType"/>
        <xs:element name="upperCorner" type="gml:DirectPositionType"/>
        <xs:element name="posList" type="gml:DirectPositionListType"/>

        <xs:complexType name="EnvelopeType">
            <xs:sequence>
                <xs:element ref="gml:lowerCorner"/>
                <xs:element ref="gml:upperCorner"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="DirectPositionType">
            <xs:simpleContent>
                <xs:extension base="xs:string"/>
            </xs:simpleContent>
        </xs:complexType>

        <xs:complexType name="DirectPositionListType">
            <xs:simpleContent>
                <xs:extension base="xs:string"/>
            </xs:simpleContent>
        </xs:complexType>
    </xs:schema>"#;

    // geometryBasic2d.xsd - Polygon, LinearRing, exterior
    let geometry_basic_2d_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:element name="Polygon" type="gml:PolygonType"/>
        <xs:element name="LinearRing" type="gml:LinearRingType"/>
        <xs:element name="exterior" type="gml:AbstractRingPropertyType"/>

        <xs:complexType name="PolygonType">
            <xs:sequence>
                <xs:element ref="gml:exterior" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="LinearRingType">
            <xs:sequence>
                <xs:element ref="gml:posList" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="AbstractRingPropertyType">
            <xs:sequence>
                <xs:element ref="gml:LinearRing" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // geometryAggregates.xsd - MultiSurface, surfaceMember
    let geometry_aggregates_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:element name="MultiSurface" type="gml:MultiSurfaceType"/>
        <xs:element name="surfaceMember" type="gml:SurfacePropertyType"/>

        <xs:complexType name="MultiSurfaceType">
            <xs:sequence>
                <xs:element ref="gml:surfaceMember" minOccurs="0" maxOccurs="unbounded"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="SurfacePropertyType">
            <xs:sequence>
                <xs:element ref="gml:Polygon" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // feature.xsd - boundedBy
    let feature_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:element name="boundedBy" type="gml:BoundingShapeType"/>

        <xs:complexType name="BoundingShapeType">
            <xs:sequence>
                <xs:element ref="gml:Envelope" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>

        <xs:complexType name="AbstractFeatureType" abstract="true">
            <xs:sequence>
                <xs:element ref="gml:boundedBy" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;

    // Parse all schemas (simulating include chain resolution)
    let geo_0d1d_ast = parse_xsd_ast(geometry_basic_0d1d_xsd.as_bytes()).unwrap();
    let geo_2d_ast = parse_xsd_ast(geometry_basic_2d_xsd.as_bytes()).unwrap();
    let geo_agg_ast = parse_xsd_ast(geometry_aggregates_xsd.as_bytes()).unwrap();
    let feature_ast = parse_xsd_ast(feature_xsd.as_bytes()).unwrap();

    // Compile all schemas together (as if gml.xsd included them all)
    let all_schemas = vec![geo_0d1d_ast, geo_2d_ast, geo_agg_ast, feature_ast];

    let mut compiled = compile_schemas(all_schemas).expect("Failed to compile schemas");
    register_builtin_types(&mut compiled);

    eprintln!("=== GML include chain test ===");
    eprintln!(
        "Elements in schema: {:?}",
        compiled.elements.keys().collect::<Vec<_>>()
    );

    // All GML elements should be accessible
    let expected_elements = [
        "boundedBy",
        "Envelope",
        "lowerCorner",
        "upperCorner",
        "posList",
        "Polygon",
        "LinearRing",
        "exterior",
        "MultiSurface",
        "surfaceMember",
    ];

    for elem_name in &expected_elements {
        // Should be accessible by local name
        assert!(
            compiled.get_element(elem_name).is_some(),
            "Should find {} by local name",
            elem_name
        );
        // Should also be accessible by prefixed name
        let prefixed = format!("gml:{}", elem_name);
        assert!(
            compiled.get_element(&prefixed).is_some(),
            "Should find {} with prefix",
            prefixed
        );
    }

    // All GML types should also be accessible
    let expected_types = [
        "EnvelopeType",
        "DirectPositionType",
        "DirectPositionListType",
        "PolygonType",
        "LinearRingType",
        "AbstractRingPropertyType",
        "MultiSurfaceType",
        "SurfacePropertyType",
        "BoundingShapeType",
        "AbstractFeatureType",
    ];

    for type_name in &expected_types {
        assert!(
            compiled.get_type(type_name).is_some(),
            "Should find {} type by local name",
            type_name
        );
        let prefixed = format!("gml:{}", type_name);
        assert!(
            compiled.get_type(&prefixed).is_some(),
            "Should find {} type with prefix",
            prefixed
        );
    }
}

/// Test that GML elements are accessible with both prefixed and local names.
///
/// GML elements like boundedBy, Envelope, MultiSurface should be accessible via:
/// - "gml:boundedBy" (prefixed)
/// - "boundedBy" (local name)
///
/// This is needed because type children references may use local names without prefix
/// when referring to elements in the same namespace.
#[test]
fn test_gml_elements_accessible_by_local_name() {
    use fastxml::schema::xsd::{compile_schemas, parser::parse_xsd_ast, register_builtin_types};

    // GML schema with elements
    let gml_xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:gml="http://www.opengis.net/gml"
               targetNamespace="http://www.opengis.net/gml"
               elementFormDefault="qualified">

        <xs:element name="boundedBy" type="xs:string"/>
        <xs:element name="Envelope" type="xs:string"/>
        <xs:element name="MultiSurface" type="xs:string"/>
        <xs:element name="Polygon" type="xs:string"/>
        <xs:element name="LinearRing" type="xs:string"/>
        <xs:element name="posList" type="xs:string"/>
    </xs:schema>"#;

    let gml_ast = parse_xsd_ast(gml_xsd.as_bytes()).unwrap();
    let mut compiled = compile_schemas(vec![gml_ast]).expect("Failed to compile schemas");
    register_builtin_types(&mut compiled);

    eprintln!("=== GML elements accessibility test ===");
    eprintln!(
        "Elements in schema: {:?}",
        compiled.elements.keys().collect::<Vec<_>>()
    );

    // Elements should be accessible with prefix
    assert!(
        compiled.get_element("gml:boundedBy").is_some(),
        "Should find gml:boundedBy with prefix"
    );
    assert!(
        compiled.get_element("gml:Envelope").is_some(),
        "Should find gml:Envelope with prefix"
    );
    assert!(
        compiled.get_element("gml:MultiSurface").is_some(),
        "Should find gml:MultiSurface with prefix"
    );

    // Elements should ALSO be accessible WITHOUT prefix (local name)
    assert!(
        compiled.get_element("boundedBy").is_some(),
        "Should find boundedBy by local name"
    );
    assert!(
        compiled.get_element("Envelope").is_some(),
        "Should find Envelope by local name"
    );
    assert!(
        compiled.get_element("MultiSurface").is_some(),
        "Should find MultiSurface by local name"
    );
    assert!(
        compiled.get_element("Polygon").is_some(),
        "Should find Polygon by local name"
    );
    assert!(
        compiled.get_element("LinearRing").is_some(),
        "Should find LinearRing by local name"
    );
    assert!(
        compiled.get_element("posList").is_some(),
        "Should find posList by local name"
    );
}

#[test]
fn test_parse_xsd_with_imports_multiple_shared_dependency() {
    use fastxml::schema::Schema;
    use fastxml::schema::fetcher::{FetchResult, SchemaFetcher};
    use std::collections::HashMap;
    use std::sync::RwLock;

    // Common dependency schema
    let common_xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               targetNamespace="http://example.com/common">
        <xs:simpleType name="IDType">
            <xs:restriction base="xs:string">
                <xs:pattern value="[A-Z0-9]+"/>
            </xs:restriction>
        </xs:simpleType>
    </xs:schema>"#;

    // Schema A imports common
    let schema_a = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:c="http://example.com/common"
               targetNamespace="http://example.com/a">
        <xs:import namespace="http://example.com/common" schemaLocation="http://example.com/common.xsd"/>
        <xs:element name="itemA">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="id" type="c:IDType"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;

    // Schema B also imports common
    let schema_b = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:c="http://example.com/common"
               targetNamespace="http://example.com/b">
        <xs:import namespace="http://example.com/common" schemaLocation="http://example.com/common.xsd"/>
        <xs:element name="itemB">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="id" type="c:IDType"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;

    // Track fetch count to verify deduplication
    struct CountingFetcher {
        responses: HashMap<String, Vec<u8>>,
        fetch_count: RwLock<HashMap<String, usize>>,
    }

    impl CountingFetcher {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
                fetch_count: RwLock::new(HashMap::new()),
            }
        }

        fn add(&mut self, url: &str, content: &[u8]) {
            self.responses.insert(url.to_string(), content.to_vec());
        }

        fn get_fetch_count(&self, url: &str) -> usize {
            *self.fetch_count.read().unwrap().get(url).unwrap_or(&0)
        }
    }

    impl SchemaFetcher for CountingFetcher {
        fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
            *self
                .fetch_count
                .write()
                .unwrap()
                .entry(url.to_string())
                .or_insert(0) += 1;

            if let Some(content) = self.responses.get(url) {
                Ok(FetchResult {
                    content: content.clone(),
                    final_url: url.to_string(),
                    redirected: false,
                })
            } else {
                Err(fastxml::error::Error::Schema(
                    fastxml::schema::error::SchemaError::SchemaNotFound {
                        uri: url.to_string(),
                    },
                ))
            }
        }
    }

    let mut fetcher = CountingFetcher::new();
    fetcher.add("http://example.com/common.xsd", common_xsd.as_bytes());

    let schema = Schema::builder()
        .add("http://example.com/a.xsd", schema_a.as_bytes())
        .add("http://example.com/b.xsd", schema_b.as_bytes())
        .resolve_with(&fetcher)
        .unwrap();

    // Both entry schemas' elements should be present
    assert!(
        schema.elements.contains_key("itemA"),
        "Should contain itemA from schema A"
    );
    assert!(
        schema.elements.contains_key("itemB"),
        "Should contain itemB from schema B"
    );

    // Common dependency should be fetched only once
    assert_eq!(
        fetcher.get_fetch_count("http://example.com/common.xsd"),
        1,
        "Common dependency should be fetched only once"
    );
}

#[test]
fn test_parse_xsd_with_imports_multiple_no_entries() {
    use fastxml::schema::Schema;
    use fastxml::schema::fetcher::NoopFetcher;

    let fetcher = NoopFetcher;

    let schema = Schema::builder().resolve_with(&fetcher).unwrap();

    // Should have built-in types
    assert!(schema.types.contains_key("xs:string"));
}

#[test]
fn test_parse_xsd_with_imports_multiple_single_entry() {
    use fastxml::schema::Schema;
    use fastxml::schema::fetcher::NoopFetcher;

    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               targetNamespace="http://example.com/test">
        <xs:element name="root" type="xs:string"/>
    </xs:schema>"#;

    let fetcher = NoopFetcher;

    let schema = Schema::builder()
        .add("http://example.com/test.xsd", xsd.as_bytes())
        .resolve_with(&fetcher)
        .unwrap();

    assert!(schema.elements.contains_key("root"));
}

#[test]
fn test_parse_xsd_with_imports_multiple_duplicate_entry() {
    use fastxml::schema::Schema;
    use fastxml::schema::fetcher::NoopFetcher;

    let xsd = r#"<?xml version="1.0"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root" type="xs:string"/>
    </xs:schema>"#;

    let fetcher = NoopFetcher;

    // Same schema passed twice - should not error
    let schema = Schema::builder()
        .add("http://example.com/test.xsd", xsd.as_bytes())
        .add("http://example.com/test.xsd", xsd.as_bytes())
        .resolve_with(&fetcher)
        .unwrap();

    assert!(schema.elements.contains_key("root"));
}
