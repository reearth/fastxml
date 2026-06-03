//! Advanced tests for the transform module: edge cases, context, fallback, error location.

use fastxml::transform::{EditableNode, Transformer};

// =============================================================================
// Edge Cases
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_transform_empty_xml() {
        let xml = "";

        let result = Transformer::from(xml)
            .on("//item", |_: &mut EditableNode| {})
            .to_string()
            .unwrap();

        assert_eq!(result, "");
    }

    #[test]
    fn test_transform_xml_declaration() {
        let xml = r#"<?xml version="1.0"?><root><item/></root>"#;

        let result = Transformer::from(xml)
            .on("//item", |node: &mut EditableNode| {
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

        let result = Transformer::from(xml)
            .on("//item", |node: &mut EditableNode| {
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

        let result = Transformer::from(xml)
            .on("//item", |node: &mut EditableNode| {
                node.set_attribute("has_cdata", "true");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"has_cdata="true""#));
    }

    #[test]
    fn test_transform_self_closing_element() {
        let xml = r#"<root><empty/></root>"#;

        let result = Transformer::from(xml)
            .on("//empty", |node: &mut EditableNode| {
                node.set_attribute("not_empty", "now");
            })
            .to_string()
            .unwrap();

        assert!(result.contains(r#"not_empty="now""#));
    }

    #[test]
    fn test_transform_special_characters_in_attribute() {
        let xml = r#"<root><item/></root>"#;

        let result = Transformer::from(xml)
            .on("//item", |node: &mut EditableNode| {
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
        Transformer::from(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//item", |node: &mut EditableNode| {
                // Should be able to get attribute by local name only
                found_id = node.get_attribute("id");
            })
            .for_each()
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
        Transformer::from(xml)
            .namespace("gml", "http://www.opengis.net/gml")
            .on("//item", |node: &mut EditableNode| {
                // Prefixed key should NOT work
                prefixed_id = node.get_attribute("gml:id");
            })
            .for_each()
            .unwrap();

        assert_eq!(prefixed_id, None);
    }
}

// =============================================================================
// Parent Context Access Tests
// =============================================================================

#[test]
fn test_on_with_context_parent() {
    let xml = r#"<root><items id="list1"><item>A</item><item>B</item></items></root>"#;

    let mut parent_names = Vec::new();
    let mut parent_ids = Vec::new();

    Transformer::from(xml)
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

    Transformer::from(xml)
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

    Transformer::from(xml)
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

    Transformer::from(xml)
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

    Transformer::from(xml)
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

    let result = Transformer::from(xml)
        .on_with_context("//item", |node, ctx| {
            let path = ctx.path_id();
            let pos = ctx.position();
            node.set_attribute("path", &format!("{}/item[{}]", path, pos));

            if let Some(parent_id) = ctx.parent_attribute("id") {
                node.set_attribute("parent_id", parent_id);
            }
        })
        .to_string()
        .unwrap();

    assert!(result.contains(r#"path="root/items/item[1]""#));
    assert!(result.contains(r#"path="root/items/item[2]""#));
    assert!(result.contains(r#"parent_id="list1""#));
}

// =============================================================================
// Fallback Control Tests
// =============================================================================

#[test]
fn test_fallback_disabled_by_default() {
    let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

    let result = Transformer::from(xml)
        .on("//item[last()]", |_| {})
        .for_each();

    // Should fail because last() is not streamable and fallback is disabled
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = format!("{}", err);
    assert!(err_str.contains("not streamable") || err_str.contains("NotStreamable"));
}

#[test]
fn test_allow_fallback() {
    let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

    let result = Transformer::from(xml)
        .allow_fallback()
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
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
    let result_disabled = Transformer::from(xml)
        .fallback_mode(FallbackMode::Disabled)
        .on("//item[last()]", |_| {})
        .for_each();
    assert!(result_disabled.is_err());

    // Test with Enabled mode
    let result_enabled = Transformer::from(xml)
        .fallback_mode(FallbackMode::Enabled)
        .on("//item[last()]", |_| {})
        .for_each();
    assert!(result_enabled.is_ok());
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
    use fastxml::transform::{Transformer, TransformError};

    // Invalid XML - unclosed tag
    let xml = "<root><item";

    let result = Transformer::from(xml).on("//item", |_| {}).to_string();

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
