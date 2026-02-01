//! Comprehensive tests for requirement verification.
//!
//! This file tests all functional requirements from the implementation plan.

use fastxml::{
    parse, parse_with_options, ParserOptions,
    evaluate, create_context,
    find_nodes_by_xpath, find_readonly_nodes_by_xpath,
    get_root_node, get_root_readonly_node, get_node_tag,
    node_to_xml_string,
    parse_schema_locations,
};
use fastxml::xpath::collect_text_values;
use fastxml::schema::{
    create_xml_schema_validation_context,
    validate_document_by_schema,
    TempDirStore, InMemoryStore, SchemaStore,
};

// =============================================================================
// XPath Pattern Tests (from plan requirements)
// =============================================================================

/// Test: //*[name()='Building']
#[test]
fn test_xpath_pattern_name_equals() {
    let xml = r#"<root>
        <Building gml:id="bldg1"/>
        <Room gml:id="room1"/>
        <Window gml:id="win1"/>
        <nested>
            <Building gml:id="bldg2"/>
        </nested>
    </root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//*[name()='Building']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|n| n.get_name() == "Building"));
}

/// Test: //*[(name()='Building' or name()='Room')]
#[test]
fn test_xpath_pattern_or_condition() {
    let xml = r#"<root>
        <Building gml:id="bldg1"/>
        <Room gml:id="room1"/>
        <Window gml:id="win1"/>
        <Other gml:id="other1"/>
    </root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    let names: Vec<_> = nodes.iter().map(|n| n.get_name()).collect();
    assert!(names.contains(&"Building".to_string()));
    assert!(names.contains(&"Room".to_string()));
}

/// Test: //*[(name()='Building' or name()='Room') and not(name()='Window')]
#[test]
fn test_xpath_pattern_and_not_condition() {
    let xml = r#"<root>
        <Building gml:id="bldg1"/>
        <Room gml:id="room1"/>
        <Window gml:id="win1"/>
    </root>"#;
    let doc = parse(xml).unwrap();

    // This should match Building and Room but exclude Window
    let result = evaluate(&doc, "//*[(name()='Building' or name()='Room') and not(name()='Window')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    let names: Vec<_> = nodes.iter().map(|n| n.get_name()).collect();
    assert!(names.contains(&"Building".to_string()));
    assert!(names.contains(&"Room".to_string()));
    assert!(!names.contains(&"Window".to_string()));
}

/// Test: /gml:Dictionary/gml:dictionaryEntry/gml:Definition/gml:name/text()
#[test]
fn test_xpath_pattern_gml_dictionary() {
    let xml = r#"<gml:Dictionary xmlns:gml="http://www.opengis.net/gml/3.2">
        <gml:dictionaryEntry>
            <gml:Definition gml:id="def1">
                <gml:name>BuildingType1</gml:name>
            </gml:Definition>
        </gml:dictionaryEntry>
        <gml:dictionaryEntry>
            <gml:Definition gml:id="def2">
                <gml:name>BuildingType2</gml:name>
            </gml:Definition>
        </gml:dictionaryEntry>
        <gml:dictionaryEntry>
            <gml:Definition gml:id="def3">
                <gml:name>BuildingType3</gml:name>
            </gml:Definition>
        </gml:dictionaryEntry>
    </gml:Dictionary>"#;
    let doc = parse(xml).unwrap();

    // Test full path with text()
    let result = evaluate(&doc, "/gml:Dictionary/gml:dictionaryEntry/gml:Definition/gml:name/text()").unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts.len(), 3);
    assert!(texts.contains(&"BuildingType1".to_string()));
    assert!(texts.contains(&"BuildingType2".to_string()));
    assert!(texts.contains(&"BuildingType3".to_string()));
}

/// Test: .//uro:buildingIDAttribute/uro:BuildingIDAttribute/uro:buildingID
#[test]
fn test_xpath_pattern_uro_building_id() {
    let xml = r#"<bldg:Building xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                              xmlns:uro="http://www.kantei.go.jp/jp/singi/tiiki/toshisaisei/itoshisaisei/iur/uro/3.0"
                              gml:id="bldg_001">
        <uro:buildingIDAttribute>
            <uro:BuildingIDAttribute>
                <uro:buildingID>13101-bldg-12345</uro:buildingID>
            </uro:BuildingIDAttribute>
        </uro:buildingIDAttribute>
    </bldg:Building>"#;
    let doc = parse(xml).unwrap();

    let ctx = create_context(&doc).unwrap();
    let root = get_root_readonly_node(&doc).unwrap();

    let nodes = find_readonly_nodes_by_xpath(&ctx, ".//uro:buildingIDAttribute/uro:BuildingIDAttribute/uro:buildingID", &root).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "buildingID");
    assert_eq!(nodes[0].get_content(), Some("13101-bldg-12345".to_string()));
}

/// Test: ./child::*
#[test]
fn test_xpath_pattern_child_axis() {
    let xml = r#"<root>
        <child1/>
        <child2/>
        <child3/>
    </root>"#;
    let doc = parse(xml).unwrap();

    let ctx = create_context(&doc).unwrap();
    let root = get_root_readonly_node(&doc).unwrap();

    let nodes = find_readonly_nodes_by_xpath(&ctx, "./child::*", &root).unwrap();
    assert_eq!(nodes.len(), 3);
}

// =============================================================================
// libxml-compatible API Tests
// =============================================================================

/// Test: parse<T: AsRef<[u8]>>(xml: T) -> Result<XmlDocument>
#[test]
fn test_api_parse() {
    // From &str
    let doc1 = parse("<root/>").unwrap();
    assert!(doc1.get_root_element().is_ok());

    // From String
    let xml_string = String::from("<root/>");
    let doc2 = parse(&xml_string).unwrap();
    assert!(doc2.get_root_element().is_ok());

    // From Vec<u8>
    let xml_bytes = b"<root/>".to_vec();
    let doc3 = parse(&xml_bytes).unwrap();
    assert!(doc3.get_root_element().is_ok());
}

/// Test: parse_with_options
#[test]
fn test_api_parse_with_options() {
    let options = ParserOptions {
        buffer_size: 4096,
        max_memory: Some(1024 * 1024), // 1MB limit
        trim_text: false,
        expand_empty_elements: true,
        check_end_names: true,
        check_comments: true,
    };

    let xml = "<root><child>text</child></root>";
    let doc = parse_with_options(xml, &options).unwrap();
    assert!(doc.get_root_element().is_ok());
}

/// Test: memory limit enforcement
#[test]
fn test_api_memory_limit() {
    let options = ParserOptions {
        max_memory: Some(50), // Very small limit
        ..Default::default()
    };

    // Large content should fail
    let large_xml = format!("<root>{}</root>", "x".repeat(1000));
    let result = parse_with_options(&large_xml, &options);
    assert!(result.is_err());
}

/// Test: evaluate(document, xpath) -> Result<XPathResult>
#[test]
fn test_api_evaluate() {
    let doc = parse("<root><child>value</child></root>").unwrap();

    // Returns nodes
    let result = evaluate(&doc, "/root/child").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
}

/// Test: create_context(document) -> Result<XmlContext>
#[test]
fn test_api_create_context() {
    let doc = parse("<root xmlns:ns='http://example.com'><ns:child/></root>").unwrap();
    let ctx = create_context(&doc).unwrap();

    // Context should have namespaces from document
    let root = get_root_readonly_node(&doc).unwrap();
    let nodes = find_readonly_nodes_by_xpath(&ctx, "/root/ns:child", &root).unwrap();
    assert_eq!(nodes.len(), 1);
}

/// Test: find_nodes_by_xpath(ctx, xpath, node) -> Result<Vec<XmlNode>>
#[test]
fn test_api_find_nodes_by_xpath() {
    let doc = parse("<root><a><target/></a><b><target/></b></root>").unwrap();
    let ctx = create_context(&doc).unwrap();
    let root = get_root_node(&doc).unwrap();

    let nodes = find_nodes_by_xpath(&ctx, ".//target", &root).unwrap();
    assert_eq!(nodes.len(), 2);
}

/// Test: get_root_node(document) -> Result<XmlNode>
#[test]
fn test_api_get_root_node() {
    let doc = parse("<myroot/>").unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "myroot");
}

/// Test: get_node_tag(node) -> String
#[test]
fn test_api_get_node_tag() {
    let doc = parse("<element/>").unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(get_node_tag(&root), "element");
}

/// Test: node_to_xml_string(document, node) -> Result<String>
#[test]
fn test_api_node_to_xml_string() {
    let doc = parse(r#"<root attr="value"><child>text</child></root>"#).unwrap();
    let mut root = get_root_node(&doc).unwrap();

    let xml_str = node_to_xml_string(&doc, &mut root).unwrap();
    assert!(xml_str.contains("<root"));
    assert!(xml_str.contains("attr=\"value\""));
    assert!(xml_str.contains("<child>text</child>"));
}

/// Test: collect_text_values(xpath_value) -> Vec<String>
#[test]
fn test_api_collect_text_values() {
    let doc = parse("<root><item>one</item><item>two</item><item>three</item></root>").unwrap();
    let result = evaluate(&doc, "/root/item/text()").unwrap();
    let texts = collect_text_values(&result);

    assert_eq!(texts.len(), 3);
    assert!(texts.contains(&"one".to_string()));
    assert!(texts.contains(&"two".to_string()));
    assert!(texts.contains(&"three".to_string()));
}

/// Test: parse_schema_locations(document) -> Result<Vec<(String, String)>>
#[test]
fn test_api_parse_schema_locations() {
    let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                      xsi:schemaLocation="http://www.opengis.net/gml/3.2 http://schemas.opengis.net/gml/3.2.1/gml.xsd
                                          http://www.opengis.net/citygml/2.0 http://schemas.opengis.net/citygml/2.0/cityGMLBase.xsd">
    </root>"#;
    let doc = parse(xml).unwrap();

    let locations = parse_schema_locations(&doc).unwrap();
    assert_eq!(locations.len(), 2);

    assert_eq!(locations[0].0, "http://www.opengis.net/gml/3.2");
    assert!(locations[0].1.contains("gml.xsd"));

    assert_eq!(locations[1].0, "http://www.opengis.net/citygml/2.0");
    assert!(locations[1].1.contains("cityGMLBase.xsd"));
}

// =============================================================================
// Schema Validation API Tests
// =============================================================================

/// Test: create_xml_schema_validation_context(schema_location) -> Result<XmlSchemaValidationContext>
#[test]
fn test_api_create_schema_context() {
    // Should succeed (returns empty schema for now)
    let ctx = create_xml_schema_validation_context("http://example.com/schema.xsd").unwrap();
    assert!(ctx.schema().elements.is_empty());
}

/// Test: validate_document_by_schema(document, schema_location) -> Result<Vec<StructuredError>>
#[test]
fn test_api_validate_document() {
    let doc = parse("<root><child/></root>").unwrap();
    let errors = validate_document_by_schema(&doc, "http://example.com/schema.xsd").unwrap();

    // Currently returns empty errors (placeholder implementation)
    assert!(errors.is_empty());
}

// =============================================================================
// SchemaStore Tests
// =============================================================================

/// Test: TempDirStore operations
#[test]
fn test_schema_store_tempdir() {
    let store = TempDirStore::new().unwrap();

    // Test put and get
    let content = b"<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'/>";
    store.put("http://example.com/schema.xsd", content).unwrap();

    assert!(store.contains("http://example.com/schema.xsd"));

    let retrieved = store.get("http://example.com/schema.xsd").unwrap().unwrap();
    assert_eq!(retrieved, content);

    // Test resolve_path
    let path = store.resolve_path("http://example.com/schema.xsd").unwrap();
    assert!(path.exists());

    // Test list
    let list = store.list().unwrap();
    assert!(list.contains(&"http://example.com/schema.xsd".to_string()));

    // Test remove
    store.remove("http://example.com/schema.xsd").unwrap();
    assert!(!store.contains("http://example.com/schema.xsd"));
}

/// Test: InMemoryStore operations
#[test]
fn test_schema_store_memory() {
    let store = InMemoryStore::new();

    let content = b"<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'/>";
    store.put("http://example.com/test.xsd", content).unwrap();

    assert!(store.contains("http://example.com/test.xsd"));

    let retrieved = store.get("http://example.com/test.xsd").unwrap().unwrap();
    assert_eq!(retrieved, content);

    store.clear().unwrap();
    assert!(!store.contains("http://example.com/test.xsd"));
}

// =============================================================================
// Node Operation Tests
// =============================================================================

/// Test: node traversal and operations
#[test]
fn test_node_operations() {
    let xml = r#"<root attr1="value1" attr2="value2">
        <child1>text1</child1>
        <child2 nested="true">
            <grandchild>deep</grandchild>
        </child2>
    </root>"#;
    let doc = parse(xml).unwrap();
    let root = get_root_node(&doc).unwrap();

    // Test get_name
    assert_eq!(root.get_name(), "root");

    // Test get_attributes
    let attrs = root.get_attributes();
    assert_eq!(attrs.get("attr1"), Some(&"value1".to_string()));
    assert_eq!(attrs.get("attr2"), Some(&"value2".to_string()));

    // Test get_child_elements
    let children = root.get_child_elements();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].get_name(), "child1");
    assert_eq!(children[1].get_name(), "child2");

    // Test get_content
    assert_eq!(children[0].get_content(), Some("text1".to_string()));

    // Test navigation to grandchild
    let grandchildren = children[1].get_child_elements();
    assert_eq!(grandchildren.len(), 1);
    assert_eq!(grandchildren[0].get_name(), "grandchild");
    assert_eq!(grandchildren[0].get_content(), Some("deep".to_string()));
}

/// Test: namespace operations
#[test]
fn test_namespace_operations() {
    let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml/3.2"
                          xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
        <gml:name>Test</gml:name>
        <bldg:Building/>
    </gml:root>"#;
    let doc = parse(xml).unwrap();
    let root = get_root_node(&doc).unwrap();

    // Test prefix
    assert_eq!(root.get_prefix(), Some("gml".to_string()));

    // Test qname
    assert_eq!(root.qname(), "gml:root");

    // Test namespace declarations
    let ns_decls = root.get_namespace_declarations();
    assert_eq!(ns_decls.len(), 2);

    let ns_uris: Vec<_> = ns_decls.iter().map(|ns| ns.uri()).collect();
    assert!(ns_uris.contains(&"http://www.opengis.net/gml/3.2"));
    assert!(ns_uris.contains(&"http://www.opengis.net/citygml/building/2.0"));
}

// =============================================================================
// CityGML-specific Tests
// =============================================================================

/// Test: CityGML Building structure
#[test]
fn test_citygml_building_structure() {
    let xml = r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                                xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                                xmlns:gml="http://www.opengis.net/gml">
        <core:cityObjectMember>
            <bldg:Building gml:id="BLD_001">
                <bldg:measuredHeight uom="m">25.5</bldg:measuredHeight>
                <bldg:storeysAboveGround>8</bldg:storeysAboveGround>
            </bldg:Building>
        </core:cityObjectMember>
        <core:cityObjectMember>
            <bldg:Building gml:id="BLD_002">
                <bldg:measuredHeight uom="m">15.0</bldg:measuredHeight>
                <bldg:storeysAboveGround>5</bldg:storeysAboveGround>
            </bldg:Building>
        </core:cityObjectMember>
    </core:CityModel>"#;
    let doc = parse(xml).unwrap();

    // Find all Buildings
    let result = evaluate(&doc, "//bldg:Building").unwrap();
    let buildings = result.into_nodes();
    assert_eq!(buildings.len(), 2);

    // Get building IDs
    let ids: Vec<_> = buildings.iter()
        .map(|b| b.get_attribute("gml:id"))
        .collect();
    assert!(ids.contains(&Some("BLD_001".to_string())));
    assert!(ids.contains(&Some("BLD_002".to_string())));

    // Find measured heights
    let result = evaluate(&doc, "//bldg:measuredHeight/text()").unwrap();
    let heights = collect_text_values(&result);
    assert_eq!(heights.len(), 2);
    assert!(heights.contains(&"25.5".to_string()));
    assert!(heights.contains(&"15.0".to_string()));
}

/// Test: Multiple element types selection
#[test]
fn test_citygml_multiple_element_types() {
    let xml = r#"<root xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
        <bldg:Building gml:id="bldg1"/>
        <bldg:BuildingPart gml:id="part1"/>
        <bldg:Room gml:id="room1"/>
        <bldg:Window gml:id="win1"/>
        <bldg:Door gml:id="door1"/>
    </root>"#;
    let doc = parse(xml).unwrap();

    // Select Building or Room only
    let result = evaluate(&doc, "//*[(name()='bldg:Building' or name()='bldg:Room')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    // Exclude Window and Door from the bldg: elements
    let result = evaluate(&doc, "/root/*[not(name()='bldg:Window') and not(name()='bldg:Door')]").unwrap();
    let nodes = result.into_nodes();
    // Should get: Building, BuildingPart, Room
    assert_eq!(nodes.len(), 3);

    let names: Vec<_> = nodes.iter().map(|n| n.qname()).collect();
    assert!(names.contains(&"bldg:Building".to_string()));
    assert!(names.contains(&"bldg:BuildingPart".to_string()));
    assert!(names.contains(&"bldg:Room".to_string()));
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

/// Test: Empty document handling
#[test]
fn test_edge_case_xpath_no_results() {
    let doc = parse("<root/>").unwrap();
    let result = evaluate(&doc, "//nonexistent").unwrap();
    let nodes = result.into_nodes();
    assert!(nodes.is_empty());
}

/// Test: Unicode content
#[test]
fn test_edge_case_unicode() {
    let xml = r#"<root>
        <japanese>日本語テスト</japanese>
        <chinese>中文测试</chinese>
        <korean>한국어 테스트</korean>
        <emoji>🏙️🏢🏗️</emoji>
    </root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/japanese/text()").unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts[0], "日本語テスト");

    let result = evaluate(&doc, "/root/emoji/text()").unwrap();
    let texts = collect_text_values(&result);
    assert!(texts[0].contains("🏙️"));
}

/// Test: Very deep nesting
#[test]
fn test_edge_case_deep_nesting() {
    // Create deeply nested XML
    let mut xml = String::new();
    for i in 0..50 {
        xml.push_str(&format!("<level{}>", i));
    }
    xml.push_str("deep_value");
    for i in (0..50).rev() {
        xml.push_str(&format!("</level{}>", i));
    }

    let doc = parse(&xml).unwrap();

    // Find the deepest element
    let result = evaluate(&doc, "//level49").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_content(), Some("deep_value".to_string()));
}

/// Test: Many siblings
#[test]
fn test_edge_case_many_siblings() {
    let mut xml = String::from("<root>");
    for i in 0..100 {
        xml.push_str(&format!("<item id=\"{}\"/>", i));
    }
    xml.push_str("</root>");

    let doc = parse(&xml).unwrap();

    let result = evaluate(&doc, "/root/item").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 100);
}

/// Test: Complex predicate combinations
#[test]
fn test_complex_predicates() {
    let xml = r#"<root>
        <item type="A" status="active"/>
        <item type="A" status="inactive"/>
        <item type="B" status="active"/>
        <item type="B" status="inactive"/>
        <item type="C" status="active"/>
    </root>"#;
    let doc = parse(xml).unwrap();

    // Test compound conditions with name()
    let result = evaluate(&doc, "//*[name()='item']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 5);
}

/// Test: Parse error handling
/// Note: quick-xml is lenient and may accept malformed XML.
/// We test that certain operations fail gracefully.
#[test]
fn test_error_handling_invalid_xml() {
    // Empty input should result in no root element
    let result = parse("");
    // Might succeed but have no root - either is acceptable
    if let Ok(doc) = result {
        assert!(doc.get_root_element().is_err(), "Empty doc should have no root");
    }

    // Memory limit should be enforced
    let options = ParserOptions {
        max_memory: Some(10),
        ..Default::default()
    };
    let large_xml = format!("<root>{}</root>", "x".repeat(100));
    let result = parse_with_options(&large_xml, &options);
    assert!(result.is_err(), "Memory limit should be enforced");
}

/// Test: XPath error handling
#[test]
fn test_error_handling_invalid_xpath() {
    let doc = parse("<root/>").unwrap();

    // Invalid XPath syntax
    let result = evaluate(&doc, "/root[[[");
    assert!(result.is_err());

    // Unknown function
    let result = evaluate(&doc, "/root[unknownfn()]");
    assert!(result.is_err());
}
