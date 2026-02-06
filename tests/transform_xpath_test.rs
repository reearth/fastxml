//! XPath-related tests for the transform module.

use fastxml::transform::{
    EditableNode, StreamTransformer, XPathAnalysis, analyze_xpath_str, get_not_streamable_reason,
    is_streamable,
};

// =============================================================================
// Complex XPath Tests
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
// XPath Streamability Check Tests
// =============================================================================

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
