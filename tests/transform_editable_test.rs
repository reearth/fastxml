//! EditableNode tests for the transform module.

use fastxml::transform::{EditableNode, StreamTransformer};

// =============================================================================
// EditableNode Basic Tests
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
// EditableNode::to_xml() Tests
// =============================================================================

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
// Namespace Auto-Attachment Tests
// =============================================================================

#[test]
fn test_to_xml_with_namespaces_basic() {
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
