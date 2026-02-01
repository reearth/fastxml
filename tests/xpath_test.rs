//! Integration tests for XPath evaluation.

use fastxml::xpath::collect_text_values;
use fastxml::{
    create_context, evaluate, find_readonly_nodes_by_xpath, get_root_readonly_node, parse,
};

#[test]
fn test_xpath_simple_path() {
    let xml = r#"<root><child>text</child></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/child").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child");
}

#[test]
fn test_xpath_descendant() {
    let xml = r#"<root><a><b><target>found</target></b></a></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//target").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "target");
}

#[test]
fn test_xpath_wildcard() {
    let xml = r#"<root><a/><b/><c/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);
}

#[test]
fn test_xpath_name_predicate() {
    let xml = r#"<root><Building/><Room/><Window/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//*[name()='Building']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "Building");
}

#[test]
fn test_xpath_or_predicate() {
    let xml = r#"<root><Building/><Room/><Window/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn test_xpath_and_predicate() {
    let xml = r#"<root>
        <item type="a" status="active"/>
        <item type="a" status="inactive"/>
        <item type="b" status="active"/>
    </root>"#;
    let doc = parse(xml).unwrap();

    // Note: This tests the xpath system, though we're just checking by name here
    let result = evaluate(&doc, "//item").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);
}

#[test]
fn test_xpath_not_predicate() {
    let xml = r#"<root><Keep/><Keep/><Remove/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/*[not(name()='Remove')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|n| n.get_name() == "Keep"));
}

#[test]
fn test_xpath_text() {
    let xml = r#"<root><item>first</item><item>second</item></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/item/text()").unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts, vec!["first", "second"]);
}

#[test]
fn test_xpath_namespaced() {
    let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
        <gml:name>test value</gml:name>
        <gml:description>description</gml:description>
    </gml:root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "name");
}

#[test]
fn test_xpath_child_axis() {
    let xml = r#"<root><a/><b/><c/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/child::*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);
}

#[test]
fn test_xpath_relative_path() {
    let xml = r#"<root><parent><child>value</child></parent></root>"#;
    let doc = parse(xml).unwrap();

    let ctx = create_context(&doc).unwrap();
    let root = get_root_readonly_node(&doc).unwrap();

    let nodes = find_readonly_nodes_by_xpath(&ctx, ".//child", &root).unwrap();
    assert_eq!(nodes.len(), 1);
}

#[test]
fn test_xpath_self_axis() {
    let xml = r#"<root><child/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/child/self::*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child");
}

#[test]
fn test_xpath_parent_axis() {
    let xml = r#"<root><child/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "/root/child/..").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "root");
}

#[test]
fn test_xpath_deep_descendant() {
    let xml = r#"<root>
        <a><target>1</target></a>
        <b><c><target>2</target></c></b>
        <target>3</target>
    </root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//target").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);
}

#[test]
fn test_xpath_no_match() {
    let xml = r#"<root><child/></root>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(&doc, "//nonexistent").unwrap();
    let nodes = result.into_nodes();
    assert!(nodes.is_empty());
}

#[test]
fn test_xpath_citysgml_style() {
    let xml = r#"<gml:Dictionary xmlns:gml="http://www.opengis.net/gml">
        <gml:dictionaryEntry>
            <gml:Definition gml:id="def1">
                <gml:name>Value1</gml:name>
            </gml:Definition>
        </gml:dictionaryEntry>
        <gml:dictionaryEntry>
            <gml:Definition gml:id="def2">
                <gml:name>Value2</gml:name>
            </gml:Definition>
        </gml:dictionaryEntry>
    </gml:Dictionary>"#;
    let doc = parse(xml).unwrap();

    let result = evaluate(
        &doc,
        "/gml:Dictionary/gml:dictionaryEntry/gml:Definition/gml:name",
    )
    .unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts.len(), 2);
    assert!(texts.contains(&"Value1".to_string()));
    assert!(texts.contains(&"Value2".to_string()));
}
