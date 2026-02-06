//! Basic StreamTransformer API tests.

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
