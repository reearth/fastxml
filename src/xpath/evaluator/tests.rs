use super::*;
use crate::parse;

#[test]
fn test_simple_path() {
    let doc = parse(r#"<root><child>hello</child></root>"#).unwrap();
    let result = evaluate(&doc, "/root/child").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "child");
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_descendant() {
    let doc = parse(r#"<root><a><b>text</b></a></root>"#).unwrap();
    let result = evaluate(&doc, "//b").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "b");
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_name_predicate() {
    let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
    let result = evaluate(&doc, "//*[name()='Building']").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "Building");
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_or_predicate() {
    let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
    let result = evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 2);
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_not_predicate() {
    let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
    let result = evaluate(&doc, "/root/*[not(name()='Window')]").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|n| n.get_name() != "Window"));
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_text() {
    let doc = parse(r#"<root><child>hello</child></root>"#).unwrap();
    let result = evaluate(&doc, "/root/child/text()").unwrap();
    assert_eq!(result.to_string_value(), "hello");
}

#[test]
fn test_namespaced_xpath() {
    let doc = parse(
        r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
        <gml:name>test</gml:name>
    </gml:root>"#,
    )
    .unwrap();

    let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "name");
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_child_axis() {
    let doc = parse(r#"<root><a/><b/></root>"#).unwrap();
    let result = evaluate(&doc, "/root/child::*").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 2);
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_collect_text_values() {
    let doc = parse(r#"<root><a>one</a><a>two</a></root>"#).unwrap();
    let result = evaluate(&doc, "/root/a").unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts, vec!["one", "two"]);
}

// New tests for added functionality

#[test]
fn test_position_function() {
    let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
    let result = evaluate(&doc, "/root/a[position()=2]").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_last_function() {
    let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
    let result = evaluate(&doc, "/root/a[last()]").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1);
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_count_function() {
    let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
    let result = evaluate(&doc, "count(/root/a)").unwrap();
    assert_eq!(result.to_number(), 3.0);
}

#[test]
fn test_concat_function() {
    let doc = parse(r#"<root><a>hello</a><b>world</b></root>"#).unwrap();
    let result = evaluate(&doc, "concat(/root/a, ' ', /root/b)").unwrap();
    assert_eq!(result.to_string_value(), "hello world");
}

#[test]
fn test_substring_function() {
    let doc = parse(r#"<root>12345</root>"#).unwrap();
    let result = evaluate(&doc, "substring(/root, 2, 3)").unwrap();
    assert_eq!(result.to_string_value(), "234");
}

#[test]
fn test_string_length_function() {
    let doc = parse(r#"<root>hello</root>"#).unwrap();
    let result = evaluate(&doc, "string-length(/root)").unwrap();
    assert_eq!(result.to_number(), 5.0);
}

#[test]
fn test_normalize_space_function() {
    let doc = parse(r#"<root>  hello   world  </root>"#).unwrap();
    let result = evaluate(&doc, "normalize-space(/root)").unwrap();
    assert_eq!(result.to_string_value(), "hello world");
}

#[test]
fn test_sum_function() {
    let doc = parse(r#"<root><n>1</n><n>2</n><n>3</n></root>"#).unwrap();
    let result = evaluate(&doc, "sum(/root/n)").unwrap();
    assert_eq!(result.to_number(), 6.0);
}

#[test]
fn test_floor_ceiling_round() {
    let doc = parse(r#"<root/>"#).unwrap();

    let result = evaluate(&doc, "floor(1.5)").unwrap();
    assert_eq!(result.to_number(), 1.0);

    let result = evaluate(&doc, "ceiling(1.5)").unwrap();
    assert_eq!(result.to_number(), 2.0);

    let result = evaluate(&doc, "round(1.5)").unwrap();
    assert_eq!(result.to_number(), 2.0);
}

#[test]
fn test_true_false_boolean() {
    let doc = parse(r#"<root/>"#).unwrap();

    let result = evaluate(&doc, "true()").unwrap();
    assert!(result.to_boolean());

    let result = evaluate(&doc, "false()").unwrap();
    assert!(!result.to_boolean());

    let result = evaluate(&doc, "boolean(1)").unwrap();
    assert!(result.to_boolean());

    let result = evaluate(&doc, "boolean(0)").unwrap();
    assert!(!result.to_boolean());
}

#[test]
fn test_arithmetic_operations() {
    let doc = parse(r#"<root/>"#).unwrap();

    for (xpath, expected) in [
        ("1 + 2", 3.0),
        ("5 - 2 - 1", 2.0),
        ("2 * 3 + 1", 7.0),
        ("1 + 2 * 3", 7.0),
        ("7 div 2", 3.5),
        ("7 mod 3", 1.0),
        ("-2 * 3", -6.0),
        ("(1 + 2) * 3", 9.0),
    ] {
        assert_eq!(
            evaluate(&doc, xpath).unwrap().to_number(),
            expected,
            "{xpath}"
        );
    }
}

#[test]
fn test_unknown_namespace_prefix_error() {
    // Test that unknown namespace prefix returns an error
    let doc = parse(r#"<root><child/></root>"#).unwrap();

    // Using an unregistered prefix should fail
    let result = evaluate(&doc, "/unknown:root");
    assert!(result.is_err());

    // Verify it's a namespace error
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("unknown namespace prefix"),
        "Expected namespace error, got: {}",
        err_str
    );
}

#[test]
fn test_registered_namespace_prefix_works() {
    // Test that registered namespace prefixes work
    let doc = parse(
        r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#,
    )
    .unwrap();

    // Using a prefix that's declared in the document should work
    let result = evaluate(&doc, "/gml:root/gml:name");
    assert!(result.is_ok());

    if let XPathResult::Nodes(nodes) = result.unwrap() {
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "name");
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_attribute_predicate() {
    let doc =
        parse(r#"<root><item id="1">A</item><item id="2">B</item><item id="3">C</item></root>"#)
            .unwrap();

    // Test attribute predicate: //item[@id='2']
    let result = evaluate(&doc, "//item[@id='2']").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1, "Expected 1 node matching //item[@id='2']");
        assert_eq!(nodes[0].get_name(), "item");
        assert_eq!(nodes[0].get_attribute("id"), Some("2".to_string()));
    } else {
        panic!("expected nodes");
    }
}

#[test]
fn test_attribute_axis() {
    let doc = parse(r#"<root><item id="1" name="test">A</item></root>"#).unwrap();

    // Test attribute axis: //item/@id
    let result = evaluate(&doc, "//item/@id").unwrap();
    if let XPathResult::Nodes(nodes) = &result {
        assert_eq!(nodes.len(), 1, "Expected 1 attribute node");
        // Attribute node should have the value as content
        assert_eq!(nodes[0].get_content(), Some("1".to_string()));
    } else if let XPathResult::String(s) = &result {
        assert_eq!(s, "1");
    } else {
        panic!("expected nodes or string, got {:?}", result);
    }
}
