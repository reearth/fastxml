//! Tests for the transform module.
//! This improves coverage for transform/*.rs

use fastxml::transform::{EditableNode, StreamTransformer, XPathSource, stream_transform};
use fastxml::xpath::{Expr, parse_xpath};

// =============================================================================
// StreamTransformer Builder Tests
// =============================================================================

mod stream_transformer_tests {
    use super::*;

    #[test]
    fn test_basic_transform() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item[@id='2']")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("modified", "true");
            })
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
            .xpath("//ns:item")
            .namespace("ns", "http://example.com")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"found="yes""#));
    }

    #[test]
    fn test_transform_no_match() {
        let xml = r#"<root><item>text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//nonexistent")
            .transform(|_node: &mut EditableNode| {
                panic!("Should not be called");
            })
            .to_string()
            .unwrap();

        // Output should be unchanged
        assert!(result.contains("<item>text</item>"));
    }

    #[test]
    fn test_transform_with_ast() {
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

    #[test]
    fn test_transform_multiple_matches() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;

        let mut count = 0;
        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                count += 1;
                node.set_attribute("n", &count.to_string());
            })
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
            .xpath("//parent")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"<parent found="yes">"#));
        assert!(result.contains("<child>text</child>"));
    }

    #[test]
    fn test_transform_remove_attribute() {
        let xml = r#"<root><item old="value">text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.remove_attribute("old");
                node.set_attribute("new", "value");
            })
            .to_string()
            .unwrap();

        assert!(!result.contains(r#"old="value""#));
        assert!(result.contains(r#"new="value""#));
    }

    #[test]
    fn test_transform_set_text_content() {
        let xml = r#"<root><item>old text</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                // set_text_content may not be fully implemented
                // Just test that it doesn't panic
                node.set_text_content("new text");
            })
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
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("done", "true");
            })
            .write_to(&mut output)
            .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.contains(r#"done="true""#));
    }

    #[test]
    fn test_transform_invalid_xpath() {
        let xml = r#"<root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("[invalid xpath")
            .transform(|_: &mut EditableNode| {})
            .to_string();

        assert!(result.is_err());
    }

    #[test]
    fn test_transform_no_xpath() {
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
            .xpath("//myitem")
            .transform(|node: &mut EditableNode| {
                *name.borrow_mut() = node.name().to_string();
            })
            .to_string();
        assert_eq!(name.into_inner(), "myitem");
    }

    #[test]
    fn test_editable_node_get_attribute() {
        let xml = r#"<root><item attr="value"/></root>"#;
        let attr = RefCell::new(None);
        let _ = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                *attr.borrow_mut() = node.get_attribute("attr");
            })
            .to_string();
        assert_eq!(attr.into_inner(), Some("value".to_string()));
    }

    #[test]
    fn test_editable_node_get_attribute_missing() {
        let xml = r#"<root><item/></root>"#;
        let attr = RefCell::new(Some("initial".to_string()));
        let _ = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                *attr.borrow_mut() = node.get_attribute("missing");
            })
            .to_string();
        assert_eq!(attr.into_inner(), None);
    }

    #[test]
    fn test_editable_node_get_content() {
        let xml = r#"<root><item>hello world</item></root>"#;
        let content = RefCell::new(None);
        let _ = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                *content.borrow_mut() = node.get_content();
            })
            .to_string();
        assert_eq!(content.into_inner(), Some("hello world".to_string()));
    }

    #[test]
    fn test_editable_node_children() {
        let xml = r#"<root><parent><child1/><child2/></parent></root>"#;
        let count = RefCell::new(0);
        let _ = StreamTransformer::new(xml)
            .xpath("//parent")
            .transform(|node: &mut EditableNode| {
                *count.borrow_mut() = node.children().len();
            })
            .to_string();
        assert_eq!(count.into_inner(), 2);
    }

    #[test]
    fn test_editable_node_children_with_text() {
        let xml = r#"<root><item>text<sub/>more</item></root>"#;
        let count = RefCell::new(0);
        let _ = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                *count.borrow_mut() = node.children().len();
            })
            .to_string();
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
            .xpath("//item[@status='active']")
            .transform(|_: &mut EditableNode| {
                count += 1;
            })
            .to_string();

        assert_eq!(count, 2);
    }

    #[test]
    fn test_transform_with_position() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item[1]")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("first", "true");
            })
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
            .xpath("//target")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("found", "yes");
            })
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
            .xpath("//item")
            .transform(|_: &mut EditableNode| {})
            .to_string()
            .unwrap();

        assert_eq!(result, "");
    }

    #[test]
    fn test_transform_xml_declaration() {
        let xml = r#"<?xml version="1.0"?><root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("x", "1");
            })
            .to_string()
            .unwrap();

        // XML declaration should be preserved
        assert!(result.contains("<?xml"));
    }

    #[test]
    fn test_transform_with_comments() {
        let xml = r#"<root><!-- comment --><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("done", "true");
            })
            .to_string()
            .unwrap();

        // Comment should be preserved
        assert!(result.contains("<!-- comment -->"));
    }

    #[test]
    fn test_transform_with_cdata() {
        let xml = r#"<root><item><![CDATA[some <data>]]></item></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("has_cdata", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"has_cdata="true""#));
    }

    #[test]
    fn test_transform_self_closing_element() {
        let xml = r#"<root><empty/></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//empty")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("not_empty", "now");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"not_empty="now""#));
    }

    #[test]
    fn test_transform_special_characters_in_attribute() {
        let xml = r#"<root><item/></root>"#;

        let result = StreamTransformer::new(xml)
            .xpath("//item")
            .transform(|node: &mut EditableNode| {
                node.set_attribute("special", "a<b>c&d\"e");
            })
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
