//! Tests for EditableNode and related types.

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::namespace::Namespace;
use crate::node::NodeType;

use super::*;

fn create_test_node() -> EditableNode {
    let mut builder = EditableNodeBuilder::new();
    builder.start_element("item", None, None, vec![("id", "1")], vec![], vec![]);
    builder.text("Hello");
    builder.end_element();
    builder.build().unwrap()
}

fn create_prefixed_node() -> EditableNode {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("ns".to_string(), "http://example.com".to_string());
    builder.start_element(
        "item",
        Some("ns"),
        Some("http://example.com"),
        vec![("id", "1")],
        vec![],
        vec![ns],
    );
    builder.text("Hello");
    builder.end_element();
    builder.build().unwrap()
}

fn create_nested_node() -> EditableNode {
    let mut builder = EditableNodeBuilder::new();
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    builder.start_element(
        "child1",
        None,
        None,
        vec![("name", "first")],
        vec![],
        vec![],
    );
    builder.text("Child 1 text");
    builder.end_element();
    builder.start_element(
        "child2",
        None,
        None,
        vec![("name", "second")],
        vec![],
        vec![],
    );
    builder.text("Child 2 text");
    builder.end_element();
    builder.end_element();
    builder.build().unwrap()
}

#[test]
fn test_read_api() {
    let node = create_test_node();
    assert_eq!(node.name(), "item");
    assert_eq!(node.get_attribute("id"), Some("1".to_string()));
    assert_eq!(node.get_content(), Some("Hello".to_string()));
}

#[test]
fn test_qname_without_prefix() {
    let node = create_test_node();
    assert_eq!(node.qname(), "item");
}

#[test]
fn test_qname_with_prefix() {
    let node = create_prefixed_node();
    assert_eq!(node.qname(), "ns:item");
}

#[test]
fn test_prefix() {
    let node = create_prefixed_node();
    assert_eq!(node.prefix(), Some("ns".to_string()));
}

#[test]
fn test_prefix_none() {
    let node = create_test_node();
    assert_eq!(node.prefix(), None);
}

#[test]
fn test_namespace_uri() {
    let node = create_prefixed_node();
    assert_eq!(node.namespace_uri(), Some("http://example.com".to_string()));
}

#[test]
fn test_namespace_uri_none() {
    let node = create_test_node();
    assert_eq!(node.namespace_uri(), None);
}

#[test]
fn test_get_attributes() {
    let node = create_test_node();
    let attrs = node.get_attributes();
    assert_eq!(attrs.get("id"), Some(&"1".to_string()));
}

#[test]
fn test_children() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name(), "child1");
    assert_eq!(children[1].name(), "child2");
}

#[test]
fn test_child_nodes() {
    let node = create_test_node();
    let child_nodes = node.child_nodes();
    // Should have at least the text node
    assert!(!child_nodes.is_empty());
}

#[test]
fn test_set_attribute() {
    let mut node = create_test_node();
    node.set_attribute("modified", "true");
    assert_eq!(node.get_attribute("modified"), Some("true".to_string()));
    assert!(node.is_modified());
}

#[test]
fn test_remove_attribute() {
    let mut node = create_test_node();
    assert_eq!(node.get_attribute("id"), Some("1".to_string()));
    node.remove_attribute("id");
    assert_eq!(node.get_attribute("id"), None);
    assert!(node.is_modified());
}

#[test]
fn test_set_text_content() {
    let mut node = create_test_node();
    node.set_text_content("New content");
    assert!(node.is_modified());
}

#[test]
fn test_append_child() {
    let mut node = create_test_node();
    node.append_child(NewNode::Text("appended".to_string()));
    assert!(node.is_modified());
    assert_eq!(node.modifications().len(), 1);
}

#[test]
fn test_prepend_child() {
    let mut node = create_test_node();
    node.prepend_child(NewNode::Text("prepended".to_string()));
    assert!(node.is_modified());
}

#[test]
fn test_replace_text() {
    let mut node = create_test_node();
    node.replace_text("Hello", "Goodbye");
    assert!(node.is_modified());
}

#[test]
fn test_remove() {
    let mut node = create_test_node();
    assert!(!node.is_removed());
    node.remove();
    assert!(node.is_removed());
    assert!(node.is_modified());
}

#[test]
fn test_is_modified_initially_false() {
    let node = create_test_node();
    assert!(!node.is_modified());
}

#[test]
fn test_modifications() {
    let mut node = create_test_node();
    node.set_attribute("a", "1");
    node.set_attribute("b", "2");
    assert_eq!(node.modifications().len(), 2);
}

#[test]
fn test_document() {
    let node = create_test_node();
    let doc = node.document();
    assert!(doc.root_element_id.is_some());
}

#[test]
fn test_document_mut() {
    let mut node = create_test_node();
    let _doc = node.document_mut();
}

// EditableNodeRef tests
#[test]
fn test_editable_node_ref_node_type() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].node_type(), NodeType::Element);
}

#[test]
fn test_editable_node_ref_name() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].name(), "child1");
}

#[test]
fn test_editable_node_ref_qname() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].qname(), "child1");
}

#[test]
fn test_editable_node_ref_prefix() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].prefix(), None);
}

#[test]
fn test_editable_node_ref_get_content() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].get_content(), Some("Child 1 text".to_string()));
}

#[test]
fn test_editable_node_ref_get_attribute() {
    let node = create_nested_node();
    let children = node.children();
    assert_eq!(children[0].get_attribute("name"), Some("first".to_string()));
}

#[test]
fn test_editable_node_ref_get_attributes() {
    let node = create_nested_node();
    let children = node.children();
    let attrs = children[0].get_attributes();
    assert!(attrs.contains_key("name"));
}

#[test]
fn test_editable_node_ref_is_element() {
    let node = create_nested_node();
    let children = node.children();
    assert!(children[0].is_element());
}

#[test]
fn test_editable_node_ref_is_text() {
    let node = create_test_node();
    let child_nodes = node.child_nodes();
    let text_node = child_nodes.iter().find(|n| n.is_text());
    assert!(text_node.is_some());
}

// EditableNodeBuilder tests
#[test]
fn test_builder_new() {
    let builder = EditableNodeBuilder::new();
    assert_eq!(builder.depth(), 0);
    assert!(builder.is_complete());
}

#[test]
fn test_builder_default() {
    let builder = EditableNodeBuilder::default();
    assert_eq!(builder.depth(), 0);
}

#[test]
fn test_builder_depth_tracking() {
    let mut builder = EditableNodeBuilder::new();
    assert_eq!(builder.depth(), 0);
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    assert_eq!(builder.depth(), 1);
    builder.start_element("child", None, None, vec![], vec![], vec![]);
    assert_eq!(builder.depth(), 2);
    builder.end_element();
    assert_eq!(builder.depth(), 1);
    builder.end_element();
    assert_eq!(builder.depth(), 0);
}

#[test]
fn test_builder_is_complete() {
    let mut builder = EditableNodeBuilder::new();
    assert!(builder.is_complete());
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    assert!(!builder.is_complete());
    builder.end_element();
    assert!(builder.is_complete());
}

#[test]
fn test_builder_cdata() {
    let mut builder = EditableNodeBuilder::new();
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    builder.cdata("<special>");
    builder.end_element();
    let node = builder.build().unwrap();
    // CDATA content should be part of the node content
    let content = node.get_content();
    assert!(content.is_some());
}

#[test]
fn test_builder_comment() {
    let mut builder = EditableNodeBuilder::new();
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    builder.comment("This is a comment");
    builder.end_element();
    let node = builder.build().unwrap();
    assert_eq!(node.name(), "root");
}

#[test]
fn test_builder_build_error_no_root() {
    let builder = EditableNodeBuilder::new();
    let result = builder.build();
    assert!(result.is_err());
}

// Modification enum tests
#[test]
fn test_modification_debug() {
    let mod1 = Modification::SetAttribute {
        name: "id".to_string(),
        value: "1".to_string(),
    };
    let debug = format!("{:?}", mod1);
    assert!(debug.contains("SetAttribute"));
}

#[test]
fn test_modification_clone() {
    let mod1 = Modification::SetAttribute {
        name: "id".to_string(),
        value: "1".to_string(),
    };
    let mod2 = mod1.clone();
    let debug1 = format!("{:?}", mod1);
    let debug2 = format!("{:?}", mod2);
    assert_eq!(debug1, debug2);
}

#[test]
fn test_modification_variants() {
    let _mod1 = Modification::SetAttribute {
        name: "id".to_string(),
        value: "1".to_string(),
    };
    let _mod2 = Modification::RemoveAttribute {
        name: "id".to_string(),
    };
    let _mod3 = Modification::SetTextContent("text".to_string());
    let _mod4 = Modification::AppendChild(NewNode::Text("text".to_string()));
    let _mod5 = Modification::PrependChild(NewNode::Text("text".to_string()));
    let _mod6 = Modification::ReplaceText {
        old: "old".to_string(),
        new: "new".to_string(),
    };
}

// NewNode enum tests
#[test]
fn test_new_node_debug() {
    let node = NewNode::Text("hello".to_string());
    let debug = format!("{:?}", node);
    assert!(debug.contains("Text"));
}

#[test]
fn test_new_node_clone() {
    let node1 = NewNode::Text("hello".to_string());
    let node2 = node1.clone();
    let debug1 = format!("{:?}", node1);
    let debug2 = format!("{:?}", node2);
    assert_eq!(debug1, debug2);
}

#[test]
fn test_new_node_variants() {
    let _text = NewNode::Text("text".to_string());
    let _cdata = NewNode::CData("<data>".to_string());
    let _comment = NewNode::Comment("comment".to_string());
    let _element = NewNode::Element {
        name: "elem".to_string(),
        prefix: Some("ns".to_string()),
        attributes: IndexMap::from([("id".to_string(), "1".to_string())]),
        children: vec![NewNode::Text("child text".to_string())],
    };
}

#[test]
fn test_new_node_element_with_children() {
    let element = NewNode::Element {
        name: "parent".to_string(),
        prefix: None,
        attributes: IndexMap::new(),
        children: vec![
            NewNode::Element {
                name: "child".to_string(),
                prefix: None,
                attributes: IndexMap::new(),
                children: vec![],
            },
            NewNode::Text("text".to_string()),
        ],
    };
    let debug = format!("{:?}", element);
    assert!(debug.contains("parent"));
    assert!(debug.contains("child"));
}

// Serialization API tests
#[test]
fn test_to_xml() {
    let node = create_test_node();
    let xml = node.to_xml().unwrap();
    assert!(xml.contains("<item"));
    assert!(xml.contains("id="));
    assert!(xml.contains("Hello"));
}

#[test]
fn test_to_xml_with_prefix() {
    let node = create_prefixed_node();
    let xml = node.to_xml().unwrap();
    assert!(xml.contains("ns:item"));
}

#[test]
fn test_display_trait() {
    let node = create_test_node();
    let display = format!("{}", node);
    assert!(display.contains("<item"));
    assert!(display.contains("Hello"));
}

#[test]
fn test_to_xml_nested() {
    let node = create_nested_node();
    let xml = node.to_xml().unwrap();
    assert!(xml.contains("<root"));
    assert!(xml.contains("<child1"));
    assert!(xml.contains("<child2"));
}

#[test]
fn test_try_from_ref() {
    let node = create_test_node();
    let xml: String = (&node).try_into().unwrap();
    assert!(xml.contains("<item"));
    assert!(xml.contains("Hello"));
}

#[test]
fn test_try_from_owned() {
    let node = create_test_node();
    let xml: String = node.try_into().unwrap();
    assert!(xml.contains("<item"));
    assert!(xml.contains("Hello"));
}

// =========================================================================
// XPath Evaluation Tests
// =========================================================================

#[test]
fn test_evaluate_xpath_simple() {
    let node = create_nested_node();
    let result = node.evaluate_xpath("//child1").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child1");
}

#[test]
fn test_evaluate_xpath_with_predicate() {
    let node = create_nested_node();
    let result = node.evaluate_xpath("//*[@name='second']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child2");
}

#[test]
fn test_find_by_xpath() {
    let node = create_nested_node();
    let refs = node.find_by_xpath("/root/*").unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].name(), "child1");
    assert_eq!(refs[1].name(), "child2");
}

#[test]
fn test_evaluate_xpath_text_content() {
    let node = create_nested_node();
    let result = node.evaluate_xpath("//child1/text()").unwrap();
    assert_eq!(result.to_string_value(), "Child 1 text");
}

#[test]
fn test_evaluate_xpath_with_namespaces() {
    let mut namespaces = HashMap::new();
    namespaces.insert("ns".to_string(), "http://example.com".to_string());

    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("ns", "http://example.com");
    builder.set_namespaces(namespaces.clone());
    builder.start_element(
        "root",
        Some("ns"),
        Some("http://example.com"),
        vec![],
        vec![],
        vec![ns.clone()],
    );
    builder.start_element(
        "child",
        Some("ns"),
        Some("http://example.com"),
        vec![],
        vec![],
        vec![],
    );
    builder.text("Hello");
    builder.end_element();
    builder.end_element();
    let node = builder.build().unwrap();

    let result = node.evaluate_xpath("//ns:child").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_name(), "child");
}

#[test]
fn test_evaluate_xpath_no_match() {
    let node = create_nested_node();
    let refs = node.find_by_xpath("//nonexistent").unwrap();
    assert!(refs.is_empty());
}

#[test]
fn test_evaluate_xpath_invalid() {
    let node = create_nested_node();
    let result = node.evaluate_xpath("[[[invalid");
    assert!(result.is_err());
}

#[test]
fn test_evaluate_xpath_local_name() {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("ns", "http://example.com");
    builder.start_element(
        "root",
        Some("ns"),
        Some("http://example.com"),
        vec![],
        vec![],
        vec![ns.clone()],
    );
    builder.start_element(
        "item",
        Some("ns"),
        Some("http://example.com"),
        vec![("id", "1")],
        vec![],
        vec![],
    );
    builder.text("A");
    builder.end_element();
    builder.end_element();
    let node = builder.build().unwrap();

    let result = node.evaluate_xpath("//*[local-name()='item']").unwrap();
    let nodes = result.into_nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].get_attribute("id"), Some("1".to_string()));
}

// =========================================================================
// get_attribute_ns Tests
// =========================================================================

#[test]
fn test_get_attribute_ns_found() {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("gml", "http://www.opengis.net/gml");
    builder.start_element(
        "Feature",
        None,
        None,
        vec![("id", "f1")],
        vec![("id", "gml", "http://www.opengis.net/gml")],
        vec![ns],
    );
    builder.end_element();
    let node = builder.build().unwrap();

    assert_eq!(
        node.get_attribute_ns("http://www.opengis.net/gml", "id"),
        Some("f1".to_string()),
    );
}

#[test]
fn test_get_attribute_ns_not_found() {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("gml", "http://www.opengis.net/gml");
    builder.start_element(
        "Feature",
        None,
        None,
        vec![("id", "f1")],
        vec![("id", "gml", "http://www.opengis.net/gml")],
        vec![ns],
    );
    builder.end_element();
    let node = builder.build().unwrap();

    // Wrong URI should return None
    assert_eq!(node.get_attribute_ns("http://wrong.uri", "id"), None,);
}

// =========================================================================
// EditableNodeRef children / namespace_uri Tests
// =========================================================================

#[test]
fn test_editable_node_ref_children() {
    let mut builder = EditableNodeBuilder::new();
    builder.start_element("root", None, None, vec![], vec![], vec![]);
    builder.start_element("parent", None, None, vec![], vec![], vec![]);
    builder.start_element("grandchild1", None, None, vec![], vec![], vec![]);
    builder.text("gc1");
    builder.end_element();
    builder.start_element("grandchild2", None, None, vec![], vec![], vec![]);
    builder.text("gc2");
    builder.end_element();
    builder.end_element();
    builder.end_element();
    let node = builder.build().unwrap();

    // Get children of root → parent
    let children = node.children();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name(), "parent");

    // Recursive: get children of parent → grandchild1, grandchild2
    let grandchildren = children[0].children();
    assert_eq!(grandchildren.len(), 2);
    assert_eq!(grandchildren[0].name(), "grandchild1");
    assert_eq!(grandchildren[1].name(), "grandchild2");
    assert_eq!(grandchildren[0].get_content(), Some("gc1".to_string()),);
}

#[test]
fn test_editable_node_ref_child_nodes() {
    let node = create_nested_node();
    let children = node.children();
    // child1 should have a text child node
    let child_nodes = children[0].child_nodes();
    assert!(!child_nodes.is_empty());
    let text_node = child_nodes.iter().find(|n| n.is_text());
    assert!(text_node.is_some());
    assert_eq!(
        text_node.unwrap().get_content(),
        Some("Child 1 text".to_string()),
    );
}

#[test]
fn test_editable_node_ref_namespace_uri() {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("ns", "http://example.com");
    builder.start_element(
        "root",
        Some("ns"),
        Some("http://example.com"),
        vec![],
        vec![],
        vec![ns.clone()],
    );
    builder.start_element(
        "child",
        Some("ns"),
        Some("http://example.com"),
        vec![],
        vec![],
        vec![],
    );
    builder.end_element();
    builder.end_element();
    let node = builder.build().unwrap();

    let children = node.children();
    assert_eq!(children.len(), 1);
    assert_eq!(
        children[0].namespace_uri(),
        Some("http://example.com".to_string()),
    );
}

#[test]
fn test_editable_node_ref_get_attribute_ns() {
    let mut builder = EditableNodeBuilder::new();
    let ns = Namespace::new("gml", "http://www.opengis.net/gml");
    builder.start_element("root", None, None, vec![], vec![], vec![ns.clone()]);
    builder.start_element(
        "child",
        None,
        None,
        vec![("id", "c1")],
        vec![("id", "gml", "http://www.opengis.net/gml")],
        vec![],
    );
    builder.end_element();
    builder.end_element();
    let node = builder.build().unwrap();

    let children = node.children();
    assert_eq!(
        children[0].get_attribute_ns("http://www.opengis.net/gml", "id"),
        Some("c1".to_string()),
    );
    assert_eq!(children[0].get_attribute_ns("http://wrong.uri", "id"), None,);
}
