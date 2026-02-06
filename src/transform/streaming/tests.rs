//! Tests for streaming processing.

use super::*;
use crate::transform::xpath_analyze::{XPathAnalysis, analyze_xpath};
use crate::xpath::parser::parse_xpath;
use std::collections::HashMap;

fn get_streamable_xpath(xpath_str: &str) -> StreamableXPath {
    let expr = parse_xpath(xpath_str).unwrap();
    match analyze_xpath(&expr) {
        XPathAnalysis::Streamable(s) => s,
        XPathAnalysis::NotStreamable(r) => panic!("Expected streamable xpath: {:?}", r),
    }
}

#[test]
fn test_simple_transform() {
    let input = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
    let xpath = get_streamable_xpath("//item[@id='2']");

    let mut output = Vec::new();
    let count = process_streaming(
        input,
        &xpath,
        &HashMap::new(),
        |node| {
            node.set_attribute("modified", "true");
        },
        &mut output,
    )
    .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 1);
    assert!(result.contains(r#"item id="1""#));
    assert!(result.contains(r#"modified="true""#));
}

#[test]
fn test_no_match() {
    let input = r#"<root><item id="1">A</item></root>"#;
    let xpath = get_streamable_xpath("//item[@id='999']");

    let mut output = Vec::new();
    let count = process_streaming(input, &xpath, &HashMap::new(), |_node| {}, &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 0);
    assert_eq!(result, input);
}

// =============================================================================
// Multi Transform Tests
// =============================================================================

#[test]
fn test_multi_transform_non_overlapping() {
    // //item and //other match different elements
    let input = r#"<root><item id="1">A</item><other id="2">B</other></root>"#;
    let xpath_item = get_streamable_xpath("//item");
    let xpath_other = get_streamable_xpath("//other");

    let mut handler1 = |node: &mut EditableNode| {
        node.set_attribute("type", "item");
    };
    let mut handler2 = |node: &mut EditableNode| {
        node.set_attribute("type", "other");
    };

    let mut handlers: Vec<MultiTransformHandler<'_>> =
        vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 2);
    assert!(result.contains(r#"type="item""#));
    assert!(result.contains(r#"type="other""#));
}

#[test]
fn test_multi_transform_interleaved() {
    // <item/><other/><item/> order is preserved
    let input = r#"<root><item>1</item><other>2</other><item>3</item></root>"#;
    let xpath_item = get_streamable_xpath("//item");
    let xpath_other = get_streamable_xpath("//other");

    let mut handler1 = |node: &mut EditableNode| {
        node.set_attribute("type", "item");
    };
    let mut handler2 = |node: &mut EditableNode| {
        node.set_attribute("type", "other");
    };

    let mut handlers: Vec<MultiTransformHandler<'_>> =
        vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 3);

    // Check that order is preserved
    let item1_pos = result.find("<item type=\"item\">1</item>").unwrap();
    let other_pos = result.find("<other type=\"other\">2</other>").unwrap();
    let item2_pos = result.rfind("<item type=\"item\">3</item>").unwrap();

    assert!(item1_pos < other_pos);
    assert!(other_pos < item2_pos);
}

#[test]
fn test_multi_transform_zero_copy() {
    // Non-matched parts are preserved exactly
    let input = r#"<?xml version="1.0"?>
<root>
  <!-- comment -->
  <unchanged>keep me</unchanged>
  <item>transform</item>
  <also-unchanged attr="value">keep this too</also-unchanged>
</root>"#;
    let xpath = get_streamable_xpath("//item");

    let mut handler = |node: &mut EditableNode| {
        node.set_attribute("modified", "true");
    };

    let mut handlers: Vec<MultiTransformHandler<'_>> = vec![(&xpath, &mut handler)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 1);

    // Check that XML declaration is preserved
    assert!(result.starts_with(r#"<?xml version="1.0"?>"#));

    // Check that unchanged elements are preserved exactly
    assert!(result.contains("<unchanged>keep me</unchanged>"));
    assert!(result.contains(r#"<also-unchanged attr="value">keep this too</also-unchanged>"#));

    // Check that comment is preserved
    assert!(result.contains("<!-- comment -->"));

    // Check that transformation was applied
    assert!(result.contains(r#"<item modified="true">transform</item>"#));
}

#[test]
fn test_multi_transform_empty_elements() {
    // Test with empty elements
    let input = r#"<root><item/><other/><item/></root>"#;
    let xpath_item = get_streamable_xpath("//item");
    let xpath_other = get_streamable_xpath("//other");

    let mut handler1 = |node: &mut EditableNode| {
        node.set_attribute("type", "item");
    };
    let mut handler2 = |node: &mut EditableNode| {
        node.set_attribute("type", "other");
    };

    let mut handlers: Vec<MultiTransformHandler<'_>> =
        vec![(&xpath_item, &mut handler1), (&xpath_other, &mut handler2)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 3);
    assert_eq!(result.matches(r#"type="item""#).count(), 2);
    assert_eq!(result.matches(r#"type="other""#).count(), 1);
}

#[test]
fn test_multi_transform_with_context() {
    // Test context-aware multi transform
    use crate::transform::context::TransformContext;

    let input = r#"<root><items><item>A</item><item>B</item></items></root>"#;
    let xpath = get_streamable_xpath("//item");

    let mut handler = |node: &mut EditableNode, ctx: &TransformContext| {
        node.set_attribute("pos", &ctx.position().to_string());
        node.set_attribute("depth", &ctx.depth().to_string());
    };

    let mut handlers: Vec<MultiTransformHandlerWithContext<'_>> = vec![(&xpath, &mut handler)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi_with_context(input, &mut handlers, &HashMap::new(), &mut output)
            .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 2);
    assert!(result.contains(r#"pos="1""#));
    assert!(result.contains(r#"pos="2""#));
    assert!(result.contains(r#"depth="3""#)); // root=1, items=2, item=3
}

#[test]
fn test_multi_transform_first_match_wins() {
    // Test first-match-wins: if //items matches, //item inside it is ignored
    let input = r#"<root><items><item>A</item></items></root>"#;
    let xpath_items = get_streamable_xpath("//items");
    let xpath_item = get_streamable_xpath("//item");

    let mut items_matched = false;
    let mut item_matched = false;

    let mut handler1 = |_node: &mut EditableNode| {
        items_matched = true;
    };
    let mut handler2 = |_node: &mut EditableNode| {
        item_matched = true;
    };

    // items handler is first, so it wins for nested elements
    let mut handlers: Vec<MultiTransformHandler<'_>> =
        vec![(&xpath_items, &mut handler1), (&xpath_item, &mut handler2)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    // Only items should match (first-match-wins), item is inside items
    assert_eq!(count, 1);
    assert!(items_matched);
    assert!(!item_matched); // Nested item is ignored because items matched first
}

#[test]
fn test_multi_transform_remove_elements() {
    // Test removing elements
    let input = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;
    let xpath_remove = get_streamable_xpath("//remove");

    let mut handler = |node: &mut EditableNode| {
        node.remove();
    };

    let mut handlers: Vec<MultiTransformHandler<'_>> = vec![(&xpath_remove, &mut handler)];

    let mut output = Vec::new();
    let count =
        process_streaming_multi(input, &mut handlers, &HashMap::new(), &mut output).unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 1);
    assert!(!result.contains("<remove>"));
    assert!(result.contains("<keep>A</keep>"));
    assert!(result.contains("<keep>C</keep>"));
}
