//! Namespace-related tests for the transform module.

use fastxml::transform::{EditableNode, StreamTransformer};

// =============================================================================
// Namespace Auto-Registration Tests
// =============================================================================

#[test]
fn test_with_root_namespaces() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <gml:point id="1"/>
        </root>"#;

    let result = StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            node.set_attribute("found", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"found="true""#));
}

#[test]
fn test_with_root_namespaces_multiple() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns:uro="http://example.com/uro">
            <gml:point/><uro:item/>
        </root>"#;

    let mut found_gml = false;
    let mut found_uro = false;

    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |_| found_gml = true)
        .on("//uro:item", |_| found_uro = true)
        .for_each()
        .unwrap();

    assert!(found_gml);
    assert!(found_uro);
}

// =============================================================================
// Namespace URI Matching Tests
// =============================================================================

#[test]
fn test_namespace_uri_matching() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <gml:feature id="1">Test</gml:feature>
        </root>"#;

    let result = StreamTransformer::new(xml)
        .namespace("gml", "http://www.opengis.net/gml")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |node| {
                node.set_attribute("matched", "true");
            },
        )
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"matched="true""#));
}

#[test]
fn test_namespace_uri_matching_different_prefix() {
    // Test that namespace-uri() matches elements with different prefixes but same URI
    let xml = r#"<root xmlns:g="http://www.opengis.net/gml">
            <g:feature id="1">Test</g:feature>
        </root>"#;

    let result = StreamTransformer::new(xml)
        .namespace("g", "http://www.opengis.net/gml")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |node| {
                node.set_attribute("matched", "true");
            },
        )
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    // Should match even though the prefix is 'g' instead of 'gml'
    assert!(result.contains(r#"matched="true""#));
}

#[test]
fn test_namespace_uri_no_match_wrong_uri() {
    let xml = r#"<root xmlns:gml="http://different.uri.com">
            <gml:feature id="1">Test</gml:feature>
        </root>"#;

    let mut matched = false;

    StreamTransformer::new(xml)
        .namespace("gml", "http://different.uri.com")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |_| {
                matched = true;
            },
        )
        .for_each()
        .unwrap();

    // Should NOT match because the URI is different
    assert!(!matched);
}

#[test]
fn test_local_name_only_matching() {
    let xml = r#"<root><item id="1">A</item><ns:item xmlns:ns="http://example.com" id="2">B</ns:item></root>"#;

    let mut matched_ids = Vec::new();

    StreamTransformer::new(xml)
        .namespace("ns", "http://example.com")
        .on("//*[local-name()='item']", |node| {
            if let Some(id) = node.get_attribute("id") {
                matched_ids.push(id);
            }
        })
        .for_each()
        .unwrap();

    // Should match both items regardless of namespace
    assert_eq!(matched_ids, vec!["1", "2"]);
}

// =============================================================================
// Attribute Namespace Preservation Tests
// =============================================================================

mod attribute_namespace_tests {
    use super::*;

    /// Test that xlink:href is serialized as xlink:href (not just href)
    #[test]
    fn test_attribute_prefix_preserved_in_serialization() {
        let xml = r#"<root xmlns:xlink="http://www.w3.org/1999/xlink">
            <item xlink:href="http://example.com"/>
        </root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("xlink", "http://www.w3.org/1999/xlink")
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // The attribute should keep its xlink: prefix
        assert!(
            result.contains("xlink:href"),
            "Expected 'xlink:href' in output, got: {}",
            result
        );
    }

    /// Test that namespace-uri() works on attributes via XPath
    #[test]
    fn test_attribute_namespace_uri_xpath_match() {
        let xml = r#"<root xmlns:xlink="http://www.w3.org/1999/xlink">
            <item xlink:href="http://example.com">text</item>
        </root>"#;

        let mut matched = false;

        StreamTransformer::new(xml)
            .namespace("xlink", "http://www.w3.org/1999/xlink")
            .allow_fallback()
            .on(
                "//*[@*[namespace-uri()='http://www.w3.org/1999/xlink' and local-name()='href']]",
                |_node: &mut EditableNode| {
                    matched = true;
                },
            )
            .for_each()
            .unwrap();

        assert!(
            matched,
            "XPath with namespace-uri() on attribute should match"
        );
    }

    /// Test that to_xml_with_namespaces() includes xmlns:xlink when attribute uses xlink prefix
    #[test]
    fn test_to_xml_with_namespaces_includes_attribute_prefix() {
        let xml = r#"<root xmlns:xlink="http://www.w3.org/1999/xlink">
            <item xlink:href="http://example.com"/>
        </root>"#;

        let mut fragment_xml = String::new();
        StreamTransformer::new(xml)
            .with_root_namespaces()
            .unwrap()
            .on("//item", |node: &mut EditableNode| {
                fragment_xml = node.to_xml_with_namespaces().unwrap();
            })
            .for_each()
            .unwrap();

        // The fragment should include xmlns:xlink because the attribute uses the xlink prefix
        assert!(
            fragment_xml.contains("xmlns:xlink"),
            "Expected 'xmlns:xlink' in fragment, got: {}",
            fragment_xml
        );
        assert!(
            fragment_xml.contains("xlink:href"),
            "Expected 'xlink:href' in fragment, got: {}",
            fragment_xml
        );
    }

    /// Test that self-closing elements also preserve attribute prefixes
    /// (add_empty_to_builder delegates to add_start_to_builder)
    #[test]
    fn test_attribute_prefix_preserved_self_closing() {
        let xml = r#"<root xmlns:xlink="http://www.w3.org/1999/xlink"><item xlink:href="http://example.com"/></root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("xlink", "http://www.w3.org/1999/xlink")
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(
            result.contains("xlink:href"),
            "Self-closing element should preserve attribute prefix, got: {}",
            result
        );
    }

    /// Test with gml:id (common in CityGML/PLATEAU)
    #[test]
    fn test_gml_id_attribute_prefix_preserved() {
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:Point gml:id="p1"><gml:pos>1.0 2.0</gml:pos></gml:Point></root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//gml:Point", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(
            result.contains("gml:id"),
            "Expected 'gml:id' in output, got: {}",
            result
        );
    }
}
