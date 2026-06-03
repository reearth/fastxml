//! Tests for StreamTransformer builder API.

use crate::transform::builder::StreamTransformer;
use crate::transform::{
    FallbackMode, NotStreamableReason, TransformError, XPathAnalysis, analyze_xpath_str,
    get_not_streamable_reason, is_streamable,
};

// =============================================================================
// New API Tests
// =============================================================================

#[test]
fn test_on_single_handler() {
    let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//item[@id='2']", |node| {
            node.set_attribute("modified", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"modified="true""#));
    assert!(result.contains("<item id=\"1\">A</item>"));
}

#[test]
fn test_on_multiple_handlers() {
    let xml = r#"<root><item>A</item><other>B</other></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//item", |node| {
            node.set_attribute("type", "item");
        })
        .on("//other", |node| {
            node.set_attribute("type", "other");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"type="item""#));
    assert!(result.contains(r#"type="other""#));
}

#[test]
fn test_for_each_single_handler() {
    let xml = r#"<root><item id="1"/><item id="2"/></root>"#;

    let mut ids = Vec::new();
    StreamTransformer::new(xml)
        .on("//item", |node| {
            if let Some(id) = node.get_attribute("id") {
                ids.push(id);
            }
        })
        .for_each()
        .unwrap();

    assert_eq!(ids, vec!["1", "2"]);
}

#[test]
fn test_for_each_multiple_handlers() {
    let xml = r#"<root><item>A</item><other>B</other></root>"#;

    let mut items = Vec::new();
    let mut others = Vec::new();

    StreamTransformer::new(xml)
        .on("//item", |node| {
            items.push(node.get_content().unwrap_or_default());
        })
        .on("//other", |node| {
            others.push(node.get_content().unwrap_or_default());
        })
        .for_each()
        .unwrap();

    assert_eq!(items, vec!["A"]);
    assert_eq!(others, vec!["B"]);
}

#[test]
fn test_collect() {
    let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

    let contents: Vec<String> = StreamTransformer::new(xml)
        .collect("//item", |node| node.get_content().unwrap_or_default())
        .unwrap();

    assert_eq!(contents, vec!["A", "B", "C"]);
}

#[test]
fn test_collect_attributes() {
    let xml = r#"<root><item id="1"/><item id="2"/><item id="3"/></root>"#;

    let ids: Vec<String> = StreamTransformer::new(xml)
        .collect("//item", |node| {
            node.get_attribute("id").unwrap_or_default()
        })
        .unwrap();

    assert_eq!(ids, vec!["1", "2", "3"]);
}

#[test]
fn test_run_no_handlers_error() {
    let xml = "<root/>";
    let result = StreamTransformer::new(xml).run();
    assert!(result.is_err());
}

#[test]
fn test_for_each_no_handlers_error() {
    let xml = "<root/>";
    let result = StreamTransformer::new(xml).for_each();
    assert!(result.is_err());
}

#[test]
fn test_transform_output_count() {
    let xml = r#"<root><item/><item/><item/></root>"#;

    let output = StreamTransformer::new(xml)
        .on("//item", |node| {
            node.set_attribute("found", "true");
        })
        .run()
        .unwrap();

    assert_eq!(output.count(), 3);
}

#[test]
fn test_with_namespaces() {
    let xml = r#"<root xmlns:ns="http://example.com"><ns:item/></root>"#;

    let result = StreamTransformer::new(xml)
        .namespace("ns", "http://example.com")
        .on("//ns:item", |node| {
            node.set_attribute("found", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"found="true""#));
}

#[test]
fn test_remove_element() {
    let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//remove", |node| {
            node.remove();
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(!result.contains("<remove>"));
    assert!(result.contains("<keep>A</keep>"));
    assert!(result.contains("<keep>C</keep>"));
}

#[test]
fn test_fallback_for_last_with_allow_fallback() {
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

    assert!(result.contains(r#"last="true""#));
    assert_eq!(result.matches(r#"last="true""#).count(), 1);
}

#[test]
fn test_not_streamable_error_without_fallback() {
    let xml = "<root><item>A</item><item>B</item></root>";

    let result = StreamTransformer::new(xml)
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
        .run();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, TransformError::NotStreamable { .. }));
    let err_msg = format!("{}", err);
    assert!(err_msg.contains("last()"));
    assert!(err_msg.contains("allow_fallback()"));
}

#[test]
fn test_fallback_mode_enabled() {
    let xml = "<root><item>A</item><item>B</item></root>";

    let result = StreamTransformer::new(xml)
        .fallback_mode(FallbackMode::Enabled)
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
        .run();

    assert!(result.is_ok());
}

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
// Deprecated API Tests (ensure backward compatibility)
// =============================================================================

// =============================================================================
// XPath Analysis API Tests
// =============================================================================

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
fn test_analyze_xpath_str_parse_error() {
    let result = analyze_xpath_str("//[invalid");
    assert!(result.is_err());
}

#[test]
fn test_is_streamable_true() {
    assert!(is_streamable("//item"));
    assert!(is_streamable("//item[@id='1']"));
    assert!(is_streamable("/root/items/item"));
}

#[test]
fn test_is_streamable_false() {
    assert!(!is_streamable("//item[last()]"));
    assert!(!is_streamable("//item/parent::*"));
    assert!(!is_streamable("//a | //b"));
}

#[test]
fn test_get_not_streamable_reason_some() {
    let reason = get_not_streamable_reason("//item[last()]");
    assert!(reason.is_some());
    assert!(matches!(reason.unwrap(), NotStreamableReason::UsesLast));
}

#[test]
fn test_get_not_streamable_reason_none() {
    let reason = get_not_streamable_reason("//item[@id='1']");
    assert!(reason.is_none());
}

#[test]
fn test_not_streamable_reason_display() {
    let reason = NotStreamableReason::UsesLast;
    let display = format!("{}", reason);
    assert!(display.contains("last()"));
}

// =============================================================================
// Function API Tests
// =============================================================================

#[test]
fn test_function_api() {
    let xml = "<root><item>test</item></root>";
    let mut output = Vec::new();

    let transformed = StreamTransformer::new(xml)
        .on("//item", |node| {
            node.set_attribute("processed", "true");
        })
        .run()
        .unwrap();
    let count = transformed.count();
    transformed.write_to(&mut output).unwrap();

    assert_eq!(count, 1);
    let result = String::from_utf8(output).unwrap();
    assert!(result.contains(r#"processed="true""#));
}

// =============================================================================
// Namespace URI Matching Tests
// =============================================================================

#[test]
fn test_namespace_uri_matching() {
    // Test that namespace-uri() and local-name() predicates work for matching
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
    // Test that namespace-uri() doesn't match when URI is different
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
    // Test that local-name() alone matches elements regardless of prefix
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
// Multi-handler Single-pass Tests
// =============================================================================

#[test]
fn test_for_each_multi_handler_call_order() {
    use std::cell::RefCell;

    // Test that multiple handlers are called in document order (1,2,1,2,...)
    // This verifies single-pass processing correctly interleaves callbacks
    let xml = r#"<root>
        <item id="1"/>
        <other id="2"/>
        <item id="3"/>
        <other id="4"/>
        <item id="5"/>
    </root>"#;

    let call_order = RefCell::new(Vec::new());

    StreamTransformer::new(xml)
        .on("//item", |node| {
            if let Some(id) = node.get_attribute("id") {
                call_order.borrow_mut().push(format!("item:{}", id));
            }
        })
        .on("//other", |node| {
            if let Some(id) = node.get_attribute("id") {
                call_order.borrow_mut().push(format!("other:{}", id));
            }
        })
        .for_each()
        .unwrap();

    // Should be called in document order, not handler order
    assert_eq!(
        *call_order.borrow(),
        vec!["item:1", "other:2", "item:3", "other:4", "item:5"]
    );
}

#[test]
fn test_for_each_multi_handler_same_element() {
    use std::cell::RefCell;

    // Test that multiple handlers matching the same element both get called
    let xml = r#"<root><item class="a" type="b"/></root>"#;

    let call_order = RefCell::new(Vec::<&str>::new());

    StreamTransformer::new(xml)
        .on("//item[@class='a']", |_node| {
            call_order.borrow_mut().push("handler1");
        })
        .on("//item[@type='b']", |_node| {
            call_order.borrow_mut().push("handler2");
        })
        .for_each()
        .unwrap();

    // Both handlers should be called for the same element
    assert_eq!(*call_order.borrow(), vec!["handler1", "handler2"]);
}

#[test]
fn test_for_each_multi_handler_nested_independent() {
    use std::cell::RefCell;

    // Test that nested elements are processed independently
    // //items matches parent, //item matches children - both should be called
    let xml = r#"<root><items id="parent"><item id="child1"/><item id="child2"/></items></root>"#;

    let call_order = RefCell::new(Vec::new());

    StreamTransformer::new(xml)
        .on("//items", |node| {
            if let Some(id) = node.get_attribute("id") {
                call_order.borrow_mut().push(format!("items:{}", id));
            }
        })
        .on("//item", |node| {
            if let Some(id) = node.get_attribute("id") {
                call_order.borrow_mut().push(format!("item:{}", id));
            }
        })
        .for_each()
        .unwrap();

    // items is called when its subtree is complete, item is called for each child
    // But since items contains item elements, they are handled independently
    assert_eq!(
        *call_order.borrow(),
        vec!["item:child1", "item:child2", "items:parent"]
    );
}

#[test]
fn test_for_each_three_handlers_interleaved() {
    use std::cell::RefCell;

    // Test with three different handlers interleaved
    let xml = r#"<root>
        <a>1</a>
        <b>2</b>
        <c>3</c>
        <a>4</a>
        <b>5</b>
        <c>6</c>
    </root>"#;

    let call_order = RefCell::new(Vec::new());

    StreamTransformer::new(xml)
        .on("//a", |node| {
            call_order
                .borrow_mut()
                .push(format!("a:{}", node.get_content().unwrap_or_default()));
        })
        .on("//b", |node| {
            call_order
                .borrow_mut()
                .push(format!("b:{}", node.get_content().unwrap_or_default()));
        })
        .on("//c", |node| {
            call_order
                .borrow_mut()
                .push(format!("c:{}", node.get_content().unwrap_or_default()));
        })
        .for_each()
        .unwrap();

    assert_eq!(
        *call_order.borrow(),
        vec!["a:1", "b:2", "c:3", "a:4", "b:5", "c:6"]
    );
}

// =============================================================================
// Multi-handler Transform (run) Tests
// =============================================================================

#[test]
fn test_run_multi_handler_single_pass() {
    // Test that multiple handlers in run() are processed in single pass
    let xml = r#"<root><item>A</item><other>B</other><item>C</item></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//item", |node| {
            node.set_attribute("type", "item");
        })
        .on("//other", |node| {
            node.set_attribute("type", "other");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    // Verify both handlers were applied
    assert!(result.contains(r#"type="item""#));
    assert!(result.contains(r#"type="other""#));

    // Verify order is preserved
    let item1_pos = result.find("<item type=\"item\">A</item>").unwrap();
    let other_pos = result.find("<other type=\"other\">B</other>").unwrap();
    let item2_pos = result.rfind("<item type=\"item\">C</item>").unwrap();

    assert!(item1_pos < other_pos);
    assert!(other_pos < item2_pos);
}

#[test]
fn test_run_multi_handler_zero_copy_preservation() {
    // Verify that non-matched content is preserved exactly
    let xml = r#"<?xml version="1.0"?>
<!-- header comment -->
<root>
  <unchanged>keep me</unchanged>
  <item>transform</item>
</root>"#;

    let result = StreamTransformer::new(xml)
        .on("//item", |node| {
            node.set_attribute("modified", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    // XML declaration preserved
    assert!(result.starts_with(r#"<?xml version="1.0"?>"#));
    // Comment preserved
    assert!(result.contains("<!-- header comment -->"));
    // Unchanged element preserved exactly
    assert!(result.contains("<unchanged>keep me</unchanged>"));
    // Transformation applied
    assert!(result.contains(r#"<item modified="true">transform</item>"#));
}

#[test]
fn test_run_multi_handler_remove_element() {
    // Test removing elements with multi-handler transform
    let xml = r#"<root><keep>A</keep><remove>B</remove><modify>C</modify></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//remove", |node| {
            node.remove();
        })
        .on("//modify", |node| {
            node.set_attribute("changed", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(!result.contains("<remove>"));
    assert!(result.contains("<keep>A</keep>"));
    assert!(result.contains(r#"<modify changed="true">C</modify>"#));
}

#[test]
fn test_run_multi_handler_first_match_wins_nested() {
    // Test first-match-wins behavior for nested elements
    let xml = r#"<root><outer><inner>content</inner></outer></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//outer", |node| {
            node.set_attribute("matched", "outer");
        })
        .on("//inner", |node| {
            // This should NOT be called because outer matched first
            node.set_attribute("matched", "inner");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    // Only outer should have been modified
    assert!(result.contains(r#"matched="outer""#));
    // Inner should NOT have been modified (it's nested inside outer)
    assert!(!result.contains(r#"matched="inner""#));
}

#[test]
fn test_run_multi_handler_not_streamable_error() {
    // Test that NotStreamable error is returned for non-streamable XPath
    let xml = r#"<root><item>A</item></root>"#;

    let result = StreamTransformer::new(xml)
        .on("//item[last()]", |_| {})
        .on("//other", |_| {})
        .run();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TransformError::NotStreamable { .. }
    ));
}

#[test]
fn test_run_multi_handler_fallback_enabled() {
    // Test that fallback works for non-streamable XPath with allow_fallback
    let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

    let result = StreamTransformer::new(xml)
        .allow_fallback()
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
        .on("//item[1]", |node| {
            node.set_attribute("first", "true");
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"first="true""#));
    assert!(result.contains(r#"last="true""#));
}
