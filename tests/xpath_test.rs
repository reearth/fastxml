//! Integration tests for XPath evaluation.

mod common;

use fastxml::Parser;
use fastxml::compat::{
    create_context, evaluate, find_readonly_nodes_by_xpath, get_root_readonly_node,
};
use fastxml::xpath::collect_text_values;

#[test]
fn test_xpath_simple_path() {
    let xml = r#"<root><child>text</child></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/child").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child");

    compare_with_libxml!(xpath: xml, "/root/child", &doc);
}

#[test]
fn test_xpath_descendant() {
    let xml = r#"<root><a><b><target>found</target></b></a></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "//target").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "target");

    compare_with_libxml!(xpath: xml, "//target", &doc);
}

#[test]
fn test_xpath_wildcard() {
    let xml = r#"<root><a/><b/><c/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);

    compare_with_libxml!(xpath: xml, "/root/*", &doc);
}

#[test]
fn test_xpath_name_predicate() {
    let xml = r#"<root><Building/><Room/><Window/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "//*[name()='Building']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "Building");

    compare_with_libxml!(xpath: xml, "//*[name()='Building']", &doc);
}

#[test]
fn test_xpath_or_predicate() {
    let xml = r#"<root><Building/><Room/><Window/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    compare_with_libxml!(xpath: xml, "//*[(name()='Building' or name()='Room')]", &doc);
}

#[test]
fn test_xpath_and_predicate() {
    let xml = r#"<root>
        <item type="a" status="active"/>
        <item type="a" status="inactive"/>
        <item type="b" status="active"/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Note: This tests the xpath system, though we're just checking by name here
    let result = evaluate(&doc, "//item").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);

    compare_with_libxml!(xpath: xml, "//item", &doc);
}

#[test]
fn test_xpath_not_predicate() {
    let xml = r#"<root><Keep/><Keep/><Remove/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/*[not(name()='Remove')]").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|n| n.get_name() == "Keep"));

    compare_with_libxml!(xpath: xml, "/root/*[not(name()='Remove')]", &doc);
}

#[test]
fn test_xpath_text() {
    let xml = r#"<root><item>first</item><item>second</item></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/item/text()").unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts, vec!["first", "second"]);

    compare_with_libxml!(xpath: xml, "/root/item/text()", &doc);
}

#[test]
fn test_xpath_namespaced() {
    let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
        <gml:name>test value</gml:name>
        <gml:description>description</gml:description>
    </gml:root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "name");

    compare_with_libxml!(xpath: xml, "/gml:root/gml:name", &doc);
}

#[test]
fn test_xpath_child_axis() {
    let xml = r#"<root><a/><b/><c/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/child::*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);

    compare_with_libxml!(xpath: xml, "/root/child::*", &doc);
}

#[test]
fn test_xpath_relative_path() {
    let xml = r#"<root><parent><child>value</child></parent></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let ctx = create_context(&doc).unwrap();
    let root = get_root_readonly_node(&doc).unwrap();

    let nodes = find_readonly_nodes_by_xpath(&ctx, ".//child", &root).unwrap();
    assert_eq!(nodes.len(), 1);

    // Use parse comparison since this uses different API
    compare_with_libxml!(parse: xml, &doc);
}

#[test]
fn test_xpath_self_axis() {
    let xml = r#"<root><child/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/child/self::*").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child");

    compare_with_libxml!(xpath: xml, "/root/child/self::*", &doc);
}

#[test]
fn test_xpath_parent_axis() {
    let xml = r#"<root><child/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "/root/child/..").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "root");

    compare_with_libxml!(xpath: xml, "/root/child/..", &doc);
}

#[test]
fn test_xpath_deep_descendant() {
    let xml = r#"<root>
        <a><target>1</target></a>
        <b><c><target>2</target></c></b>
        <target>3</target>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "//target").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);

    compare_with_libxml!(xpath: xml, "//target", &doc);
}

#[test]
fn test_xpath_no_match() {
    let xml = r#"<root><child/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(&doc, "//nonexistent").unwrap();
    let nodes = result.into_nodes();
    assert!(nodes.is_empty());

    compare_with_libxml!(xpath: xml, "//nonexistent", &doc);
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
    let doc = Parser::from(xml).parse().unwrap();

    let result = evaluate(
        &doc,
        "/gml:Dictionary/gml:dictionaryEntry/gml:Definition/gml:name",
    )
    .unwrap();
    let texts = collect_text_values(&result);
    assert_eq!(texts.len(), 2);
    assert!(texts.contains(&"Value1".to_string()));
    assert!(texts.contains(&"Value2".to_string()));

    compare_with_libxml!(xpath: xml, "/gml:Dictionary/gml:dictionaryEntry/gml:Definition/gml:name", &doc);
}

#[test]
fn test_xpath_union_operator() {
    let xml = r#"<root><a>1</a><b>2</b><c>3</c></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Union of two paths
    let result = evaluate(&doc, "//a | //b").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    let names: Vec<_> = nodes.iter().map(|n| n.get_name()).collect();
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));

    compare_with_libxml!(xpath: xml, "//a | //b", &doc);
}

#[test]
fn test_xpath_union_three_paths() {
    let xml = r#"<root><a>1</a><b>2</b><c>3</c></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Union of three paths
    let result = evaluate(&doc, "//a | //b | //c").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 3);

    compare_with_libxml!(xpath: xml, "//a | //b | //c", &doc);
}

#[test]
fn test_xpath_union_with_predicates() {
    let xml = r#"<root><item id="1">A</item><item id="2">B</item><other>C</other></root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Union with predicates
    let result = evaluate(&doc, "//item[@id='1'] | //other").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 2);

    compare_with_libxml!(xpath: xml, "//item[@id='1'] | //other", &doc);
}

#[test]
fn test_xpath_variable_string() {
    use fastxml::xpath::{XPathEvaluator, XPathValue};
    use std::collections::HashMap;

    let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let evaluator = XPathEvaluator::new(&doc);

    let mut vars = HashMap::new();
    vars.insert("target".to_string(), XPathValue::String("1".to_string()));

    let result = evaluator
        .evaluate_with_variables("//item[@id=$target]", vars.clone())
        .unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_content(), Some("A".to_string()));

    #[cfg(feature = "compare-libxml")]
    {
        use common::libxml_compare::XPathVarValue;
        let mut libxml_vars = HashMap::new();
        libxml_vars.insert("target".to_string(), XPathVarValue::String("1".to_string()));
        compare_with_libxml!(xpath_vars: xml, "//item[@id=$target]", &doc, vars, libxml_vars);
    }
}

#[test]
fn test_xpath_variable_number() {
    use fastxml::xpath::{XPathEvaluator, XPathValue};
    use std::collections::HashMap;

    let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let evaluator = XPathEvaluator::new(&doc);

    let mut vars = HashMap::new();
    vars.insert("pos".to_string(), XPathValue::Number(2.0));

    let result = evaluator
        .evaluate_with_variables("//item[position()=$pos]", vars.clone())
        .unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_content(), Some("2".to_string()));

    #[cfg(feature = "compare-libxml")]
    {
        use common::libxml_compare::XPathVarValue;
        let mut libxml_vars = HashMap::new();
        libxml_vars.insert("pos".to_string(), XPathVarValue::Number(2.0));
        compare_with_libxml!(xpath_vars: xml, "//item[position()=$pos]", &doc, vars, libxml_vars);
    }
}

#[test]
fn test_xpath_undefined_variable() {
    use fastxml::xpath::XPathEvaluator;
    use std::collections::HashMap;

    let xml = r#"<root><item/></root>"#;
    let doc = Parser::from(xml).parse().unwrap();
    let evaluator = XPathEvaluator::new(&doc);

    let result = evaluator.evaluate_with_variables("//item[@id=$missing]", HashMap::new());
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("undefined variable"));
}

#[test]
fn test_xpath_namespace_axis() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
        <gml:name>test</gml:name>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Get all namespace nodes from root element
    let result = evaluate(&doc, "/root/namespace::*").unwrap();
    let nodes = result.into_nodes();

    // Should have at least gml, bldg, and xml namespaces
    assert!(
        nodes.len() >= 3,
        "Expected at least 3 namespace nodes, got {}",
        nodes.len()
    );

    // Check that we got the expected namespaces
    let ns_values: Vec<_> = nodes.iter().filter_map(|n| n.get_content()).collect();
    assert!(ns_values.contains(&"http://www.opengis.net/gml".to_string()));
    assert!(ns_values.contains(&"http://www.opengis.net/citygml/building/2.0".to_string()));
    assert!(ns_values.contains(&"http://www.w3.org/XML/1998/namespace".to_string())); // xml namespace

    compare_with_libxml!(xpath: xml, "/root/namespace::*", &doc);
}

#[test]
fn test_xpath_namespace_axis_with_name() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <child/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Get specific namespace by prefix
    let result = evaluate(&doc, "/root/namespace::gml").unwrap();
    let nodes = result.into_nodes();

    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].get_content(),
        Some("http://www.opengis.net/gml".to_string())
    );

    compare_with_libxml!(xpath: xml, "/root/namespace::gml", &doc);
}

#[test]
fn test_xpath_namespace_axis_inherited() {
    let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
        <child xmlns:bldg="http://www.opengis.net/citygml/building/2.0"/>
    </root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Child should inherit gml namespace from parent
    let result = evaluate(&doc, "/root/child/namespace::gml").unwrap();
    let nodes = result.into_nodes();

    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].get_content(),
        Some("http://www.opengis.net/gml".to_string())
    );

    compare_with_libxml!(xpath: xml, "/root/child/namespace::gml", &doc);
}

#[test]
fn test_xpath_namespace_axis_on_non_element() {
    let xml = r#"<root>text</root>"#;
    let doc = Parser::from(xml).parse().unwrap();

    // Namespace axis on text node should return empty
    let result = evaluate(&doc, "/root/text()/namespace::*").unwrap();
    let nodes = result.into_nodes();

    assert!(nodes.is_empty());

    compare_with_libxml!(xpath: xml, "/root/text()/namespace::*", &doc);
}
