//! Tests for namespace handling in fastxml.
//! These tests verify that namespace URIs are properly resolved during parsing.

mod common;

use fastxml::{Parser, QueryExt};

// =============================================================================
// Problem 1: get_namespace() should return the namespace for elements
// =============================================================================

#[test]
fn test_element_namespace() {
    let xml = r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0">
        <core:child/>
    </core:CityModel>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();

    let ns = root.get_namespace();
    assert!(
        ns.is_some(),
        "get_namespace() should return Some for namespaced elements"
    );
    let ns = ns.unwrap();
    assert_eq!(ns.get_prefix(), "core");
    assert_eq!(ns.get_href(), "http://www.opengis.net/citygml/2.0");
}

#[test]
fn test_child_element_namespace() {
    let xml = r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0">
        <core:child/>
    </core:CityModel>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let child = root.get_child_elements()[0].clone();

    // Child element should also have the namespace resolved
    let ns = child.get_namespace();
    assert!(ns.is_some(), "Child element should have namespace resolved");
    let ns = ns.unwrap();
    assert_eq!(ns.get_prefix(), "core");
    assert_eq!(ns.get_href(), "http://www.opengis.net/citygml/2.0");
}

#[test]
fn test_element_namespace_uri() {
    let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
        <gml:child/>
    </gml:root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();

    assert_eq!(
        root.get_namespace_uri(),
        Some("http://www.opengis.net/gml".to_string())
    );
}

#[test]
fn test_default_namespace() {
    let xml = r#"<root xmlns="http://example.com/default">
        <child/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();

    // Default namespace should be resolved
    let ns_uri = root.get_namespace_uri();
    assert_eq!(ns_uri, Some("http://example.com/default".to_string()));
}

#[test]
fn test_multiple_namespaces() {
    let xml = r#"<root xmlns:a="http://a.com" xmlns:b="http://b.com">
        <a:child1/>
        <b:child2/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let children = root.get_child_elements();

    let child1 = &children[0];
    let child2 = &children[1];

    assert_eq!(child1.get_namespace_uri(), Some("http://a.com".to_string()));
    assert_eq!(child2.get_namespace_uri(), Some("http://b.com".to_string()));
}

#[test]
fn test_nested_namespace_override() {
    let xml = r#"<root xmlns:ns="http://outer.com">
        <ns:outer>
            <inner xmlns:ns="http://inner.com">
                <ns:nested/>
            </inner>
        </ns:outer>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let outer = root.get_child_elements()[0].clone();
    let inner = outer.get_child_elements()[0].clone();
    let nested = inner.get_child_elements()[0].clone();

    // outer should use the outer namespace
    assert_eq!(
        outer.get_namespace_uri(),
        Some("http://outer.com".to_string())
    );

    // nested should use the inner (overridden) namespace
    assert_eq!(
        nested.get_namespace_uri(),
        Some("http://inner.com".to_string())
    );
}

// =============================================================================
// Problem 2: get_attribute_ns() should work with inherited namespaces
// =============================================================================

#[test]
fn test_get_attribute_ns() {
    let xml = r#"<bldg:Building xmlns:gml="http://www.opengis.net/gml"
                                xmlns:bldg="http://example.com"
                                gml:id="test123"/>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();

    let gml_id = root.get_attribute_ns("id", "http://www.opengis.net/gml");
    assert_eq!(gml_id, Some("test123".to_string()));
}

#[test]
fn test_get_attribute_ns_inherited() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <child gml:id="child123"/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let child = root.get_child_elements()[0].clone();

    // Child should be able to resolve gml namespace from parent
    let gml_id = child.get_attribute_ns("id", "http://www.opengis.net/gml");
    assert_eq!(gml_id, Some("child123".to_string()));
}

#[test]
fn test_get_attribute_ns_deeply_nested() {
    let xml = r#"<root xmlns:ns="http://example.com">
        <level1>
            <level2>
                <level3 ns:attr="deep"/>
            </level2>
        </level1>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let level1 = root.get_child_elements()[0].clone();
    let level2 = level1.get_child_elements()[0].clone();
    let level3 = level2.get_child_elements()[0].clone();

    // Should resolve namespace from root ancestor
    let attr = level3.get_attribute_ns("attr", "http://example.com");
    assert_eq!(attr, Some("deep".to_string()));
}

// =============================================================================
// Problem 3: namespace-uri() XPath function
// =============================================================================

#[test]
fn test_xpath_namespace_uri_function() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <gml:Envelope/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = doc.query("namespace-uri(/root/gml:Envelope)").unwrap();
    assert_eq!(result.to_string_value(), "http://www.opengis.net/gml");
}

#[test]
fn test_xpath_namespace_uri_predicate() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <gml:Envelope/>
        <other/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Select elements by namespace URI
    let result = doc
        .query(".//*[namespace-uri()='http://www.opengis.net/gml' and local-name()='Envelope']")
        .unwrap();
    assert_eq!(result.into_nodes().len(), 1);
}

#[test]
fn test_xpath_namespace_uri_empty_for_no_namespace() {
    let xml = r#"<root><child/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = doc.query("namespace-uri(/root/child)").unwrap();
    assert_eq!(result.to_string_value(), "");
}

// =============================================================================
// Problem 4: local-name() XPath function (related to namespace handling)
// =============================================================================

#[test]
fn test_xpath_local_name_with_namespace() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <gml:Envelope/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = doc.query("local-name(/root/gml:Envelope)").unwrap();
    assert_eq!(result.to_string_value(), "Envelope");
}

// =============================================================================
// Step 5: get_attributes() API should return local names as keys
// =============================================================================

#[test]
fn test_get_attributes_local_name_keys() {
    let xml = r#"<element xmlns:gml="http://www.opengis.net/gml" gml:id="test" name="foo"/>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let attrs = root.get_attributes();

    // libxml compatible: attribute keys should be local names only
    assert_eq!(attrs.get("id"), Some(&"test".to_string()));
    assert_eq!(attrs.get("name"), Some(&"foo".to_string()));

    // Should NOT have prefixed keys
    assert!(attrs.get("gml:id").is_none());
}

#[test]
fn test_get_attributes_multiple_namespaced() {
    let xml = r#"<root xmlns:a="http://a.com" xmlns:b="http://b.com"
                      a:x="1" b:x="2" y="3"/>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let root = doc.get_root_element().unwrap();
    let attrs = root.get_attributes();

    // When there are two attributes with same local name but different namespaces,
    // we need to use get_attribute_ns to distinguish them
    // get_attributes() returns local names, so there might be conflicts
    // For now, let's just verify that we can access the non-namespaced one
    assert_eq!(attrs.get("y"), Some(&"3".to_string()));
}

// =============================================================================
// Combined test: Real-world CityGML-like structure
// =============================================================================

#[test]
fn test_citygml_like_structure() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:gml="http://www.opengis.net/gml"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                gml:id="citymodel1">
    <core:cityObjectMember>
        <bldg:Building gml:id="building1">
            <gml:name>Test Building</gml:name>
        </bldg:Building>
    </core:cityObjectMember>
</core:CityModel>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Test XPath with namespace-uri()
    let result = doc.query(".//*[namespace-uri()='http://www.opengis.net/citygml/building/2.0' and local-name()='Building']",
    )
    .unwrap();
    let buildings = result.into_nodes();
    assert_eq!(buildings.len(), 1);

    // Verify namespace on the building element
    let building = &buildings[0];
    assert_eq!(
        building.get_namespace_uri(),
        Some("http://www.opengis.net/citygml/building/2.0".to_string())
    );

    // Verify gml:id attribute via namespace
    let gml_id = building.get_attribute_ns("id", "http://www.opengis.net/gml");
    assert_eq!(gml_id, Some("building1".to_string()));
}
