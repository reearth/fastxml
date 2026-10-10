//! Namespace-related tests for the transform module.

use fastxml::transform::{EditableNode, Transformer};

// =============================================================================
// Namespace Auto-Registration Tests
// =============================================================================

#[test]
fn test_with_root_namespaces() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <gml:point id="1"/>
        </root>"#;

    let result = Transformer::from(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            node.set_attribute("found", "true");
        })
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

    Transformer::from(xml)
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

    let result = Transformer::from(xml)
        .namespace("gml", "http://www.opengis.net/gml")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |node| {
                node.set_attribute("matched", "true");
            },
        )
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

    let result = Transformer::from(xml)
        .namespace("g", "http://www.opengis.net/gml")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |node| {
                node.set_attribute("matched", "true");
            },
        )
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

    Transformer::from(xml)
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

    Transformer::from(xml)
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

/// Element namespaces are resolved from the document's own xmlns declarations
/// and compared by URI with the caller's prefix bindings.
#[test]
fn test_prefixed_step_matches_by_namespace_uri() {
    let xml = r#"<root xmlns:g="urn:gml" xmlns:h="urn:gml" xmlns:o="urn:other"><g:f id="1"/><h:f id="2"/><o:f id="3"/><f id="4"/></root>"#;

    let ids = |prefix: &str, uri: &str, xpath: &str| {
        Transformer::from(xml)
            .namespace(prefix, uri)
            .collect(xpath, |n| n.get_attribute("id").unwrap_or_default())
            .unwrap()
    };

    // Both g:f and h:f are in urn:gml, whatever prefix the XPath uses
    assert_eq!(ids("gml", "urn:gml", "//gml:f"), vec!["1", "2"]);
    // The document's g is urn:gml, not the caller's urn:other
    assert_eq!(ids("g", "urn:other", "//g:f"), vec!["3"]);
}

#[test]
fn test_namespace_scope_follows_nested_declarations() {
    // The same prefix is rebound on a descendant; the default namespace is
    // declared below the root.
    let xml = r#"<root xmlns:p="urn:a"><p:x id="1"/><wrap xmlns:p="urn:b"><p:x id="2"/></wrap><d xmlns="urn:a"><x id="3"/></d></root>"#;

    let ids: Vec<String> = Transformer::from(xml)
        .namespace("a", "urn:a")
        .collect("//a:x", |n| n.get_attribute("id").unwrap_or_default())
        .unwrap();
    assert_eq!(ids, vec!["1", "3"]);
}

/// The README "Namespace URI Matching" example: different prefixes, same URI.
#[test]
fn test_readme_namespace_uri_matching_example() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns:g="http://www.opengis.net/gml"><gml:feature id="1"/><g:feature id="2"/><gml:other id="3"/></root>"#;

    let mut ids = Vec::new();
    Transformer::from(xml)
        .namespace("gml", "http://www.opengis.net/gml")
        .on(
            "//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']",
            |node| ids.push(node.get_attribute("id").unwrap_or_default()),
        )
        .for_each()
        .unwrap();

    assert_eq!(ids, vec!["1", "2"]);
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

        let result = Transformer::from(xml)
            .namespace("xlink", "http://www.w3.org/1999/xlink")
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
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

        Transformer::from(xml)
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
        Transformer::from(xml)
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

        let result = Transformer::from(xml)
            .namespace("xlink", "http://www.w3.org/1999/xlink")
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
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

        let result = Transformer::from(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//gml:Point", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .to_string()
            .unwrap();

        assert!(
            result.contains("gml:id"),
            "Expected 'gml:id' in output, got: {}",
            result
        );
    }
}
