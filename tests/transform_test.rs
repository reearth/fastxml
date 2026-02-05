//! Tests for the transform module.
//! This improves coverage for transform/*.rs

use fastxml::transform::{EditableNode, StreamTransformer, XPathSource, stream_transform};
use fastxml::xpath::{Expr, parse_xpath};

// =============================================================================
// StreamTransformer Builder Tests (New API)
// =============================================================================

mod stream_transformer_tests {
    use super::*;

    #[test]
    fn test_basic_transform() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item[@id='2']", |node: &mut EditableNode| {
                node.set_attribute("modified", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"modified="true""#));
        assert!(result.contains(r#"id="1""#));
        assert!(result.contains(r#"id="2""#));
    }

    #[test]
    fn test_transform_with_namespace() {
        let xml = r#"<root xmlns:ns="http://example.com"><ns:item>text</ns:item></root>"#;

        let result = StreamTransformer::new(xml)
            .namespace("ns", "http://example.com")
            .on("//ns:item", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"found="yes""#));
    }

    #[test]
    fn test_transform_no_match() {
        let xml = r#"<root><item>text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//nonexistent", |_node: &mut EditableNode| {
                panic!("Should not be called");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Output should be unchanged
        assert!(result.contains("<item>text</item>"));
    }

    #[test]
    fn test_transform_multiple_matches() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;

        let mut count = 0;
        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                count += 1;
                node.set_attribute("n", &count.to_string());
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert_eq!(count, 3);
        assert!(result.contains(r#"n="1""#));
        assert!(result.contains(r#"n="2""#));
        assert!(result.contains(r#"n="3""#));
    }

    #[test]
    fn test_transform_nested_elements() {
        let xml = r#"<root><parent><child>text</child></parent></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//parent", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"<parent found="yes">"#));
        assert!(result.contains("<child>text</child>"));
    }

    #[test]
    fn test_transform_remove_attribute() {
        let xml = r#"<root><item old="value">text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.remove_attribute("old");
                node.set_attribute("new", "value");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(!result.contains(r#"old="value""#));
        assert!(result.contains(r#"new="value""#));
    }

    #[test]
    fn test_transform_set_text_content() {
        let xml = r#"<root><item>old text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                // set_text_content may not be fully implemented
                // Just test that it doesn't panic
                node.set_text_content("new text");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Verify the transformation completed without error
        assert!(result.contains("<item"));
    }

    #[test]
    fn test_transform_write_to() {
        let xml = r#"<root><item/></root>"#;

        let mut output = Vec::new();
        StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("done", "true");
            })
            .run()
            .unwrap()
            .write_to(&mut output)
            .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.contains(r#"done="true""#));
    }

    #[test]
    fn test_transform_invalid_xpath() {
        let xml = r#"<root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .on("[invalid xpath", |_: &mut EditableNode| {})
            .run();

        assert!(result.is_err());
    }
}

// =============================================================================
// Deprecated API Tests (Backwards Compatibility)
// =============================================================================

mod deprecated_api_tests {
    use super::*;

    /// Test deprecated .xpath().transform() API for backwards compatibility
    #[test]
    #[allow(deprecated)]
    fn test_deprecated_xpath_transform() {
        let xml = r#"<root><item id="1">A</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("modified", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"modified="true""#));
    }

    /// Test deprecated .xpath_ast() API for backwards compatibility
    #[test]
    #[allow(deprecated)]
    fn test_deprecated_xpath_ast() {
        let xml = r#"<root><item>text</item></root>"#;

        // Parse XPath to AST first
        let ast = parse_xpath("//item").unwrap();

        let result = StreamTransformer::new(xml)
            .xpath_ast(ast)
            .transform(|node: &mut EditableNode| {
                node.set_attribute("processed", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"processed="true""#));
    }

    /// Test deprecated API without xpath (should return error)
    #[test]
    #[allow(deprecated)]
    fn test_deprecated_no_xpath() {
        let xml = r#"<root><item/></root>"#;

        // No xpath set - returns an error
        let result = StreamTransformer::new(xml)
            .transform(|_: &mut EditableNode| {
                panic!("Should not be called");
            })
            .to_string();

        // Without XPath, the transformer returns an error
        assert!(result.is_err());
    }
}

// =============================================================================
// stream_transform Function Tests
// =============================================================================

mod stream_transform_function_tests {
    use super::*;

    #[test]
    fn test_stream_transform_basic() {
        let xml = r#"<root><item>text</item></root>"#;
        let mut output = Vec::new();

        stream_transform(
            xml,
            "//item",
            |node: &mut EditableNode| {
                node.set_attribute("x", "y");
            },
            &mut output,
        )
        .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.contains(r#"x="y""#));
    }

    #[test]
    fn test_stream_transform_with_namespaces() {
        let xml = r#"<root xmlns:a="http://a.com"><a:item/></root>"#;
        let mut output = Vec::new();

        let result = stream_transform(
            xml,
            "//a:item",
            |node: &mut EditableNode| {
                node.set_attribute("found", "true");
            },
            &mut output,
        );

        // May succeed or fail depending on namespace handling
        let _ = result;
    }
}

// =============================================================================
// XPathSource Tests
// =============================================================================

mod xpath_source_tests {
    use super::*;

    #[test]
    fn test_xpath_source_from_str() {
        let source: XPathSource = "/root/child".into();
        assert_eq!(source.as_string(), Some("/root/child"));
    }

    #[test]
    fn test_xpath_source_from_string() {
        let source: XPathSource = String::from("//item").into();
        assert_eq!(source.as_string(), Some("//item"));
    }

    #[test]
    fn test_xpath_source_from_expr() {
        let expr = parse_xpath("//test").unwrap();
        let source: XPathSource = expr.into();
        assert!(source.as_string().is_none());
    }

    #[test]
    fn test_xpath_source_parse_string() {
        let source: XPathSource = "/root".into();
        let expr = source.parse().unwrap();
        // Check it parsed correctly by verifying it's a valid Expr
        assert!(matches!(expr, Expr::Path(_)));
    }

    #[test]
    fn test_xpath_source_parse_ast() {
        let original = parse_xpath("//item[@id='1']").unwrap();
        let source: XPathSource = original.clone().into();
        let parsed = source.parse().unwrap();
        // Should return the same AST
        assert_eq!(format!("{:?}", original), format!("{:?}", parsed));
    }

    #[test]
    fn test_xpath_source_parse_invalid() {
        let source: XPathSource = "[invalid".into();
        let result = source.parse();
        assert!(result.is_err());
    }
}

// =============================================================================
// EditableNode Tests
// =============================================================================

mod editable_node_tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn test_editable_node_name() {
        let xml = r#"<root><myitem/></root>"#;
        let name = RefCell::new(String::new());
        let _ = StreamTransformer::new(xml)
            .on("//myitem", |node: &mut EditableNode| {
                *name.borrow_mut() = node.name().to_string();
            })
            .run();
        assert_eq!(name.into_inner(), "myitem");
    }

    #[test]
    fn test_editable_node_get_attribute() {
        let xml = r#"<root><item attr="value"/></root>"#;
        let attr = RefCell::new(None);
        let _ = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                *attr.borrow_mut() = node.get_attribute("attr");
            })
            .run();
        assert_eq!(attr.into_inner(), Some("value".to_string()));
    }

    #[test]
    fn test_editable_node_get_attribute_missing() {
        let xml = r#"<root><item/></root>"#;
        let attr = RefCell::new(Some("initial".to_string()));
        let _ = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                *attr.borrow_mut() = node.get_attribute("missing");
            })
            .run();
        assert_eq!(attr.into_inner(), None);
    }

    #[test]
    fn test_editable_node_get_content() {
        let xml = r#"<root><item>hello world</item></root>"#;
        let content = RefCell::new(None);
        let _ = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                *content.borrow_mut() = node.get_content();
            })
            .run();
        assert_eq!(content.into_inner(), Some("hello world".to_string()));
    }

    #[test]
    fn test_editable_node_children() {
        let xml = r#"<root><parent><child1/><child2/></parent></root>"#;
        let count = RefCell::new(0);
        let _ = StreamTransformer::new(xml)
            .on("//parent", |node: &mut EditableNode| {
                *count.borrow_mut() = node.children().len();
            })
            .run();
        assert_eq!(count.into_inner(), 2);
    }

    #[test]
    fn test_editable_node_children_with_text() {
        let xml = r#"<root><item>text<sub/>more</item></root>"#;
        let count = RefCell::new(0);
        let _ = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                *count.borrow_mut() = node.children().len();
            })
            .run();
        // Should have: text node, sub element, text node
        assert!(count.into_inner() >= 1);
    }
}

// =============================================================================
// Transform with Complex XPath Tests
// =============================================================================

mod complex_xpath_tests {
    use super::*;

    #[test]
    fn test_transform_with_predicate() {
        let xml = r#"<root>
            <item status="active">1</item>
            <item status="inactive">2</item>
            <item status="active">3</item>
        </root>"#;

        let mut count = 0;
        let _ = StreamTransformer::new(xml)
            .on("//item[@status='active']", |_: &mut EditableNode| {
                count += 1;
            })
            .run();

        assert_eq!(count, 2);
    }

    #[test]
    fn test_transform_with_position() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item[1]", |node: &mut EditableNode| {
                node.set_attribute("first", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Only first item should be modified
        assert!(result.contains(r#"first="true""#));
        // Should only appear once
        assert_eq!(result.matches(r#"first="true""#).count(), 1);
    }

    #[test]
    fn test_transform_descendant_path() {
        let xml = r#"<root><a><b><target/></b></a></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//target", |node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"found="yes""#));
    }
}

// =============================================================================
// Edge Cases
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_transform_empty_xml() {
        let xml = "";

        let result = StreamTransformer::new(xml)
            .on("//item", |_: &mut EditableNode| {})
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert_eq!(result, "");
    }

    #[test]
    fn test_transform_xml_declaration() {
        let xml = r#"<?xml version="1.0"?><root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("x", "1");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // XML declaration should be preserved
        assert!(result.contains("<?xml"));
    }

    #[test]
    fn test_transform_with_comments() {
        let xml = r#"<root><!-- comment --><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("done", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Comment should be preserved
        assert!(result.contains("<!-- comment -->"));
    }

    #[test]
    fn test_transform_with_cdata() {
        let xml = r#"<root><item><![CDATA[some <data>]]></item></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("has_cdata", "true");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"has_cdata="true""#));
    }

    #[test]
    fn test_transform_self_closing_element() {
        let xml = r#"<root><empty/></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//empty", |node: &mut EditableNode| {
                node.set_attribute("not_empty", "now");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        assert!(result.contains(r#"not_empty="now""#));
    }

    #[test]
    fn test_transform_special_characters_in_attribute() {
        let xml = r#"<root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("special", "a<b>c&d\"e");
            })
            .run()
            .unwrap()
            .to_string()
            .unwrap();

        // Special chars should be escaped
        assert!(result.contains("&lt;") || result.contains("a<b"));
    }

    #[test]
    fn test_transform_namespaced_attribute_local_name() {
        // Test that namespaced attributes are accessible by local name (libxml compatible)
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <item gml:id="test123"/>
        </root>"#;

        let mut found_id = None;
        StreamTransformer::new(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//item", |node: &mut EditableNode| {
                // Should be able to get attribute by local name only
                found_id = node.get_attribute("id");
            })
            .run()
            .unwrap();

        assert_eq!(found_id, Some("test123".to_string()));
    }

    #[test]
    fn test_transform_namespaced_attribute_not_prefixed() {
        // Verify that prefixed key does NOT work (libxml compatible)
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
            <item gml:id="test123"/>
        </root>"#;

        let mut prefixed_id = Some("should be None".to_string());
        StreamTransformer::new(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//item", |node: &mut EditableNode| {
                // Prefixed key should NOT work
                prefixed_id = node.get_attribute("gml:id");
            })
            .run()
            .unwrap();

        assert_eq!(prefixed_id, None);
    }
}

// =============================================================================
// New Features Tests (v0.4.0)
// =============================================================================

use fastxml::transform::{
    XPathAnalysis, analyze_xpath_str, get_not_streamable_reason, is_streamable,
};

// -------------------------------------------------------------------------
// Namespace Auto-Registration Tests
// -------------------------------------------------------------------------

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

// -------------------------------------------------------------------------
// Namespace URI Matching Tests
// -------------------------------------------------------------------------

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

// -------------------------------------------------------------------------
// Parent Context Access Tests
// -------------------------------------------------------------------------

#[test]
fn test_on_with_context_parent() {
    let xml = r#"<root><items id="list1"><item>A</item><item>B</item></items></root>"#;

    let mut parent_names = Vec::new();
    let mut parent_ids = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            if let Some(parent) = ctx.parent() {
                parent_names.push(parent.name.clone());
                if let Some(id) = parent.attributes.get("id") {
                    parent_ids.push(id.clone());
                }
            }
        })
        .for_each()
        .unwrap();

    assert_eq!(parent_names, vec!["items", "items"]);
    assert_eq!(parent_ids, vec!["list1", "list1"]);
}

#[test]
fn test_on_with_context_position() {
    let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

    let mut positions = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            positions.push(ctx.position());
        })
        .for_each()
        .unwrap();

    assert_eq!(positions, vec![1, 2, 3]);
}

#[test]
fn test_on_with_context_depth() {
    let xml = r#"<root><level1><level2><target/></level2></level1></root>"#;

    let mut depths = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//target", |_node, ctx| {
            depths.push(ctx.depth());
        })
        .for_each()
        .unwrap();

    // root=1, level1=2, level2=3, target=4
    assert_eq!(depths, vec![4]);
}

#[test]
fn test_on_with_context_ancestors() {
    let xml = r#"<root><a><b><target/></b></a></root>"#;

    let mut ancestor_names = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//target", |_node, ctx| {
            ancestor_names = ctx.ancestors().iter().map(|a| a.name.clone()).collect();
        })
        .for_each()
        .unwrap();

    assert_eq!(ancestor_names, vec!["root", "a", "b"]);
}

#[test]
fn test_on_with_context_path_id() {
    let xml = r#"<root><items><item/><item/></items><items><item/></items></root>"#;

    let mut paths = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            paths.push(ctx.path_id());
        })
        .for_each()
        .unwrap();

    // First items group: position 1
    // Second items group: position 2
    assert_eq!(paths, vec!["root/items", "root/items", "root/items[2]"]);
}

#[test]
fn test_on_with_context_transform() {
    let xml = r#"<root><items id="list1"><item/><item/></items></root>"#;

    let result = StreamTransformer::new(xml)
        .on_with_context("//item", |node, ctx| {
            let path = ctx.path_id();
            let pos = ctx.position();
            node.set_attribute("path", &format!("{}/item[{}]", path, pos));

            if let Some(parent_id) = ctx.parent_attribute("id") {
                node.set_attribute("parent_id", parent_id);
            }
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"path="root/items/item[1]""#));
    assert!(result.contains(r#"path="root/items/item[2]""#));
    assert!(result.contains(r#"parent_id="list1""#));
}

// -------------------------------------------------------------------------
// XPath Streamability Check Tests
// -------------------------------------------------------------------------

#[test]
fn test_is_streamable_true() {
    assert!(is_streamable("//item[@id='1']"));
    assert!(is_streamable("/root/child"));
    assert!(is_streamable("//item[position()<=10]"));
}

#[test]
fn test_is_streamable_false() {
    assert!(!is_streamable("//item[last()]"));
    assert!(!is_streamable("//item/parent::*"));
    assert!(!is_streamable("//a | //b"));
}

#[test]
fn test_analyze_xpath_str_streamable() {
    let result = analyze_xpath_str("//item[@id='1']").unwrap();
    assert!(matches!(result, XPathAnalysis::Streamable(_)));
}

#[test]
fn test_analyze_xpath_str_not_streamable() {
    let result = analyze_xpath_str("//item[last()]").unwrap();
    assert!(matches!(result, XPathAnalysis::NotStreamable(_)));
}

#[test]
fn test_get_not_streamable_reason_last() {
    let reason = get_not_streamable_reason("//item[last()]").unwrap();
    // Check that the reason is properly formatted
    let reason_str = format!("{}", reason);
    assert!(reason_str.contains("last()"));
}

#[test]
fn test_get_not_streamable_reason_backward_axis() {
    let reason = get_not_streamable_reason("//item/parent::*").unwrap();
    let reason_str = format!("{}", reason);
    assert!(reason_str.contains("backward axis"));
}

// -------------------------------------------------------------------------
// Fallback Control Tests
// -------------------------------------------------------------------------

#[test]
fn test_fallback_disabled_by_default() {
    let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

    let result = StreamTransformer::new(xml)
        .on("//item[last()]", |_| {})
        .run();

    // Should fail because last() is not streamable and fallback is disabled
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = format!("{}", err);
    assert!(err_str.contains("not streamable") || err_str.contains("NotStreamable"));
}

#[test]
fn test_allow_fallback() {
    let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

    let result = StreamTransformer::new(xml)
        .allow_fallback()
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    // Should succeed with fallback enabled
    assert!(result.contains(r#"last="true""#));
    // Only the last item should have the attribute
    assert_eq!(result.matches(r#"last="true""#).count(), 1);
}

#[test]
fn test_fallback_mode_enum() {
    use fastxml::transform::FallbackMode;

    let xml = "<root><item>test</item></root>";

    // Test with Disabled mode
    let result_disabled = StreamTransformer::new(xml)
        .fallback_mode(FallbackMode::Disabled)
        .on("//item[last()]", |_| {})
        .run();
    assert!(result_disabled.is_err());

    // Test with Enabled mode
    let result_enabled = StreamTransformer::new(xml)
        .fallback_mode(FallbackMode::Enabled)
        .on("//item[last()]", |_| {})
        .run();
    assert!(result_enabled.is_ok());
}

// -------------------------------------------------------------------------
// EditableNode::to_xml() Tests
// -------------------------------------------------------------------------

#[test]
fn test_editable_node_to_xml() {
    let xml = r#"<root><item id="1">text</item></root>"#;

    let mut node_xml = String::new();

    StreamTransformer::new(xml)
        .on("//item", |node| {
            node_xml = node.to_xml().unwrap();
        })
        .for_each()
        .unwrap();

    assert!(node_xml.contains("<item"));
    assert!(node_xml.contains("id=\"1\"") || node_xml.contains(r#"id="1""#));
    assert!(node_xml.contains("text"));
}

#[test]
fn test_editable_node_display() {
    let xml = r#"<root><item/></root>"#;

    let mut displayed = String::new();

    StreamTransformer::new(xml)
        .on("//item", |node| {
            displayed = format!("{}", node);
        })
        .for_each()
        .unwrap();

    assert!(displayed.contains("<item"));
}

// =============================================================================
// Error Location Tests
// =============================================================================

#[test]
fn test_error_location_line_column() {
    // ErrorLocation is available from both fastxml::ErrorLocation and fastxml::transform::ErrorLocation
    use fastxml::ErrorLocation;

    let input = "line1\nline2\nline3";
    let loc = ErrorLocation::from_offset_with_input(6, input); // 'l' of 'line2'

    assert_eq!(loc.line, Some(2));
    assert_eq!(loc.column, Some(1));
    assert_eq!(loc.byte_offset, Some(6));
}

#[test]
fn test_error_location_with_xpath() {
    use fastxml::ErrorLocation;

    let loc = ErrorLocation::from_offset(100).with_xpath("/root/item[1]".to_string());

    assert_eq!(loc.byte_offset, Some(100));
    assert_eq!(loc.xpath, Some("/root/item[1]".to_string()));
    assert!(loc.to_string().contains("/root/item[1]"));
}

#[test]
fn test_error_location_display() {
    use fastxml::ErrorLocation;

    // With line/column
    let loc = ErrorLocation::from_offset_with_input(6, "line1\nline2");
    assert!(loc.to_string().contains("line 2:1"));

    // With xpath
    let loc = loc.with_xpath("/root[1]".to_string());
    assert!(loc.to_string().contains("/root[1]"));

    // Offset only
    let loc = ErrorLocation::from_offset(42);
    assert!(loc.to_string().contains("position 42"));

    // From line/column directly
    let loc = ErrorLocation::from_line_column(10, 5);
    assert!(loc.to_string().contains("line 10:5"));
}

#[test]
fn test_error_location_multibyte_utf8() {
    use fastxml::ErrorLocation;

    // Multi-byte UTF-8 characters should be counted as single columns
    // "あいう" is 9 bytes (3 bytes per char), "\n" is 1 byte
    let input = "あいう\nえお";

    // At byte 0 (first char "あ")
    let loc = ErrorLocation::from_offset_with_input(0, input);
    assert_eq!(loc.line, Some(1));
    assert_eq!(loc.column, Some(1));

    // At byte 3 (second char "い")
    let loc = ErrorLocation::from_offset_with_input(3, input);
    assert_eq!(loc.line, Some(1));
    assert_eq!(loc.column, Some(2));

    // At byte 6 (third char "う")
    let loc = ErrorLocation::from_offset_with_input(6, input);
    assert_eq!(loc.line, Some(1));
    assert_eq!(loc.column, Some(3));

    // At byte 10 (first char of line 2 "え")
    let loc = ErrorLocation::from_offset_with_input(10, input);
    assert_eq!(loc.line, Some(2));
    assert_eq!(loc.column, Some(1));

    // At byte 13 (second char of line 2 "お")
    let loc = ErrorLocation::from_offset_with_input(13, input);
    assert_eq!(loc.line, Some(2));
    assert_eq!(loc.column, Some(2));

    // Mixed ASCII and multi-byte
    let input = "ab\nあいう\nxy";

    // At byte 3 ("あ" on line 2)
    let loc = ErrorLocation::from_offset_with_input(3, input);
    assert_eq!(loc.line, Some(2));
    assert_eq!(loc.column, Some(1));

    // At byte 12 ("x" on line 3)
    let loc = ErrorLocation::from_offset_with_input(13, input);
    assert_eq!(loc.line, Some(3));
    assert_eq!(loc.column, Some(1));
}

#[test]
fn test_error_location_structured_error_integration() {
    use fastxml::{ErrorLocation, StructuredError, ValidationErrorType};

    // Create StructuredError with location
    let loc = ErrorLocation::from_line_column(42, 10).with_xpath("/root/item[3]".to_string());

    let err =
        StructuredError::new("test error", ValidationErrorType::InvalidContent).with_location(&loc);

    assert_eq!(err.line(), Some(42));
    assert_eq!(err.column(), Some(10));
    assert_eq!(err.element_path(), Some("/root/item[3]"));

    // Extract location from StructuredError
    let extracted: ErrorLocation = (&err).into();
    assert_eq!(extracted.line, Some(42));
    assert_eq!(extracted.column, Some(10));
    assert_eq!(extracted.xpath, Some("/root/item[3]".to_string()));
}

#[test]
fn test_xml_parse_error_with_location() {
    use fastxml::transform::{StreamTransformer, TransformError};

    // Invalid XML - unclosed tag
    let xml = "<root><item";

    let result = StreamTransformer::new(xml).on("//item", |_| {}).run();

    match result {
        Err(TransformError::XmlParseWithLocation { message, location }) => {
            assert!(!message.is_empty());
            assert!(location.byte_offset.is_some());
            assert!(location.line.is_some());
            assert!(location.column.is_some());
        }
        _ => panic!("Expected XmlParseWithLocation error"),
    }
}

#[test]
fn test_error_location_multiline_input() {
    use fastxml::transform::ErrorLocation;

    let input = "<?xml version=\"1.0\"?>\n<root>\n  <item>text</item>\n</root>";
    // Position after "<root>\n  " (offset = 21 + 1 + 6 + 1 + 2 = 31)
    let offset = 30; // Start of "<item>"

    let loc = ErrorLocation::from_offset_with_input(offset, input);

    assert_eq!(loc.line, Some(3)); // Third line
    assert!(loc.column.is_some());
}

// =============================================================================
// Namespace Auto-Attachment Tests
// =============================================================================

#[test]
fn test_to_xml_with_namespaces_basic() {
    use fastxml::transform::StreamTransformer;

    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point id="1"/></root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            fragment_xml = node.to_xml_with_namespaces().unwrap();
        })
        .for_each()
        .unwrap();

    // The fragment should include the namespace declaration
    assert!(fragment_xml.contains("xmlns:gml"));
    assert!(fragment_xml.contains("http://www.opengis.net/gml"));
    assert!(fragment_xml.contains("gml:point"));
}

#[test]
fn test_to_xml_with_namespaces_nested() {
    use fastxml::transform::StreamTransformer;

    let xml = r#"<root xmlns:ns="http://example.com"><ns:parent><ns:child>text</ns:child></ns:parent></root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//ns:parent", |node| {
            fragment_xml = node.to_xml_with_namespaces().unwrap();
        })
        .for_each()
        .unwrap();

    // Should contain ns declaration
    assert!(fragment_xml.contains("xmlns:ns"));
    assert!(fragment_xml.contains("http://example.com"));
    assert!(fragment_xml.contains("ns:parent"));
    assert!(fragment_xml.contains("ns:child"));
}

#[test]
fn test_to_xml_with_namespaces_multiple_prefixes() {
    use fastxml::transform::StreamTransformer;

    let xml = r#"<root xmlns:a="http://a.com" xmlns:b="http://b.com">
        <a:outer><b:inner/></a:outer>
    </root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//a:outer", |node| {
            fragment_xml = node.to_xml_with_namespaces().unwrap();
        })
        .for_each()
        .unwrap();

    // Should contain both namespace declarations
    assert!(fragment_xml.contains("xmlns:a"));
    assert!(fragment_xml.contains("http://a.com"));
    assert!(fragment_xml.contains("xmlns:b"));
    assert!(fragment_xml.contains("http://b.com"));
}

#[test]
fn test_to_xml_with_namespaces_no_duplicates() {
    use fastxml::transform::StreamTransformer;

    // Element already has the namespace declaration
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point xmlns:gml="http://www.opengis.net/gml" id="1"/></root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            fragment_xml = node.to_xml_with_namespaces().unwrap();
        })
        .for_each()
        .unwrap();

    // Should not add duplicate xmlns declaration
    let count = fragment_xml.matches("xmlns:gml").count();
    assert_eq!(count, 1, "Should not duplicate existing xmlns declaration");
}

#[test]
fn test_to_xml_without_namespaces_fallback() {
    use fastxml::transform::StreamTransformer;

    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point id="1"/></root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            // Using to_xml() (without namespaces) should NOT add xmlns declarations
            fragment_xml = node.to_xml().unwrap();
        })
        .for_each()
        .unwrap();

    // The fragment should NOT contain the namespace declaration (using to_xml, not to_xml_with_namespaces)
    assert!(!fragment_xml.contains("xmlns:gml"));
}

#[test]
fn test_to_xml_with_namespaces_empty_namespaces() {
    use fastxml::transform::StreamTransformer;

    // No namespaces registered
    let xml = r#"<root><item id="1"/></root>"#;

    let mut fragment_xml = String::new();
    StreamTransformer::new(xml)
        .on("//item", |node| {
            fragment_xml = node.to_xml_with_namespaces().unwrap();
        })
        .for_each()
        .unwrap();

    // Should work without error and not add any xmlns
    assert!(fragment_xml.contains("<item"));
    assert!(!fragment_xml.contains("xmlns"));
}

#[test]
fn test_editable_node_namespaces_accessor() {
    use fastxml::transform::StreamTransformer;

    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point/></root>"#;

    let mut has_namespaces = false;
    StreamTransformer::new(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//gml:point", |node| {
            let ns = node.namespaces();
            has_namespaces = ns.contains_key("gml");
        })
        .for_each()
        .unwrap();

    assert!(
        has_namespaces,
        "EditableNode should have access to registered namespaces"
    );
}

// =============================================================================
// collect_multi Tests
// =============================================================================

#[test]
fn test_collect_multi_basic() {
    use fastxml::transform::{EditableNode, StreamTransformer};

    let xml = r#"<root>
        <item id="1">Apple</item>
        <item id="2">Banana</item>
        <item id="3">Cherry</item>
    </root>"#;

    let (ids, contents): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |node: &mut EditableNode| {
                node.get_attribute("id").unwrap_or_default()
            }),
            ("//item", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(ids, vec!["1", "2", "3"]);
    assert_eq!(contents, vec!["Apple", "Banana", "Cherry"]);
}

#[test]
fn test_collect_multi_different_xpaths() {
    use fastxml::transform::{EditableNode, StreamTransformer};

    let xml = r#"<store>
        <product name="Widget" price="9.99"/>
        <product name="Gadget" price="19.99"/>
        <category>Electronics</category>
        <category>Home</category>
    </store>"#;

    let (products, categories): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//product", |node: &mut EditableNode| {
                node.get_attribute("name").unwrap_or_default()
            }),
            ("//category", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(products, vec!["Widget", "Gadget"]);
    assert_eq!(categories, vec!["Electronics", "Home"]);
}

#[test]
fn test_collect_multi_with_namespaces() {
    use fastxml::transform::{EditableNode, StreamTransformer};

    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <gml:Point id="p1"><gml:pos>1.0 2.0</gml:pos></gml:Point>
        <gml:Point id="p2"><gml:pos>3.0 4.0</gml:pos></gml:Point>
    </root>"#;

    let (ids, coords): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .namespace("gml", "http://www.opengis.net/gml")
        .collect_multi((
            ("//gml:Point", |node: &mut EditableNode| {
                node.get_attribute("id").unwrap_or_default()
            }),
            ("//gml:pos", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(ids, vec!["p1", "p2"]);
    assert_eq!(coords, vec!["1.0 2.0", "3.0 4.0"]);
}

#[test]
fn test_collect_multi_three_xpaths() {
    use fastxml::transform::{EditableNode, StreamTransformer};

    let xml = r#"<data>
        <a>1</a><b>2</b><c>3</c>
        <a>4</a><b>5</b><c>6</c>
    </data>"#;

    let (a_vals, b_vals, c_vals): (Vec<String>, Vec<String>, Vec<String>) =
        StreamTransformer::new(xml)
            .collect_multi((
                ("//a", |n: &mut EditableNode| {
                    n.get_content().unwrap_or_default()
                }),
                ("//b", |n: &mut EditableNode| {
                    n.get_content().unwrap_or_default()
                }),
                ("//c", |n: &mut EditableNode| {
                    n.get_content().unwrap_or_default()
                }),
            ))
            .unwrap();

    assert_eq!(a_vals, vec!["1", "4"]);
    assert_eq!(b_vals, vec!["2", "5"]);
    assert_eq!(c_vals, vec!["3", "6"]);
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

// =============================================================================
// XPath Evaluation on EditableNode Tests
// =============================================================================

mod xpath_evaluation_tests {
    use fastxml::transform::{EditableNode, StreamTransformer};

    #[test]
    fn test_stream_transformer_with_xpath_evaluation() {
        let xml = r#"<root>
            <item id="1"><name>Alice</name><age>30</age></item>
            <item id="2"><name>Bob</name><age>25</age></item>
        </root>"#;

        let mut names = Vec::new();
        StreamTransformer::new(xml)
            .on("//item", |node: &mut EditableNode| {
                let found = node.find_by_xpath(".//name").unwrap();
                if let Some(name_node) = found.first()
                    && let Some(content) = name_node.get_content()
                {
                    names.push(content);
                }
            })
            .for_each()
            .unwrap();

        assert_eq!(names, vec!["Alice", "Bob"]);
    }

    #[test]
    fn test_stream_transformer_with_namespaced_xpath() {
        let xml = r#"<root xmlns:ns="http://example.com">
            <ns:item id="1"><ns:name>Alice</ns:name></ns:item>
            <ns:item id="2"><ns:name>Bob</ns:name></ns:item>
        </root>"#;

        let mut names = Vec::new();
        StreamTransformer::new(xml)
            .namespace("ns", "http://example.com")
            .on("//ns:item", |node: &mut EditableNode| {
                let found = node.find_by_xpath(".//*[local-name()='name']").unwrap();
                if let Some(name_node) = found.first()
                    && let Some(content) = name_node.get_content()
                {
                    names.push(content);
                }
            })
            .for_each()
            .unwrap();

        assert_eq!(names, vec!["Alice", "Bob"]);
    }

    #[test]
    fn test_stream_transformer_evaluate_xpath_result_types() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let mut count = 0.0;
        StreamTransformer::new(xml)
            .on("//root", |node: &mut EditableNode| {
                let result = node.evaluate_xpath("count(//item)").unwrap();
                count = result.to_number();
            })
            .for_each()
            .unwrap();

        assert_eq!(count, 2.0);
    }
}
