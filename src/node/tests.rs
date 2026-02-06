//! Tests for node types.

use super::*;

// NodeType tests
#[test]
fn test_node_type_can_have_children() {
    assert!(NodeType::Document.can_have_children());
    assert!(NodeType::Element.can_have_children());
    assert!(!NodeType::Text.can_have_children());
    assert!(!NodeType::CData.can_have_children());
    assert!(!NodeType::Comment.can_have_children());
    assert!(!NodeType::ProcessingInstruction.can_have_children());
    assert!(!NodeType::Attribute.can_have_children());
    assert!(!NodeType::Namespace.can_have_children());
}

#[test]
fn test_node_type_debug() {
    let debug = format!("{:?}", NodeType::Element);
    assert_eq!(debug, "Element");
}

#[test]
fn test_node_type_clone_eq() {
    let t1 = NodeType::Element;
    let t2 = t1;
    assert_eq!(t1, t2);
}

// NodeData tests
#[test]
fn test_node_data_document() {
    let node = NodeData::document();
    assert_eq!(node.id, 0);
    assert_eq!(node.node_type, NodeType::Document);
    assert!(node.name.is_empty());
    assert!(node.prefix.is_none());
    assert!(node.namespace_uri.is_none());
    assert!(node.content.is_none());
    assert!(node.attributes.is_empty());
    assert!(node.namespace_decls.is_empty());
    assert!(node.parent.is_none());
    assert!(node.children.is_empty());
}

#[test]
fn test_node_data_element() {
    let node = NodeData::element(
        1,
        "test".to_string(),
        Some("ns".to_string()),
        Some("http://example.com".to_string()),
    );
    assert_eq!(node.id, 1);
    assert_eq!(node.node_type, NodeType::Element);
    assert_eq!(node.name, "test");
    assert_eq!(node.prefix, Some("ns".to_string()));
    assert_eq!(node.namespace_uri, Some("http://example.com".to_string()));
}

#[test]
fn test_node_data_text() {
    let node = NodeData::text(2, "hello world".to_string());
    assert_eq!(node.id, 2);
    assert_eq!(node.node_type, NodeType::Text);
    assert_eq!(node.content, Some("hello world".to_string()));
}

#[test]
fn test_node_data_cdata() {
    let node = NodeData::cdata(3, "<special>".to_string());
    assert_eq!(node.id, 3);
    assert_eq!(node.node_type, NodeType::CData);
    assert_eq!(node.content, Some("<special>".to_string()));
}

#[test]
fn test_node_data_comment() {
    let node = NodeData::comment(4, "a comment".to_string());
    assert_eq!(node.id, 4);
    assert_eq!(node.node_type, NodeType::Comment);
    assert_eq!(node.content, Some("a comment".to_string()));
}

#[test]
fn test_node_data_processing_instruction() {
    let node =
        NodeData::processing_instruction(5, "xml".to_string(), Some("version=\"1.0\"".to_string()));
    assert_eq!(node.id, 5);
    assert_eq!(node.node_type, NodeType::ProcessingInstruction);
    assert_eq!(node.name, "xml");
    assert_eq!(node.content, Some("version=\"1.0\"".to_string()));
}

#[test]
fn test_node_data_processing_instruction_no_content() {
    let node = NodeData::processing_instruction(6, "target".to_string(), None);
    assert_eq!(node.node_type, NodeType::ProcessingInstruction);
    assert_eq!(node.name, "target");
    assert!(node.content.is_none());
}

#[test]
fn test_node_data_attribute() {
    let node = NodeData::attribute(7, "id".to_string(), "123".to_string(), None, None);
    assert_eq!(node.id, 7);
    assert_eq!(node.node_type, NodeType::Attribute);
    assert_eq!(node.name, "id");
    assert_eq!(node.content, Some("123".to_string()));
}

#[test]
fn test_node_data_namespace_node() {
    let node = NodeData::namespace_node(
        8,
        "gml".to_string(),
        "http://www.opengis.net/gml".to_string(),
    );
    assert_eq!(node.id, 8);
    assert_eq!(node.node_type, NodeType::Namespace);
    assert_eq!(node.name, "gml");
    assert_eq!(node.content, Some("http://www.opengis.net/gml".to_string()));
}

#[test]
fn test_node_data_qname_with_prefix() {
    let node = NodeData::element(1, "name".to_string(), Some("ns".to_string()), None);
    assert_eq!(node.qname(), "ns:name");
}

#[test]
fn test_node_data_qname_without_prefix() {
    let node = NodeData::element(1, "name".to_string(), None, None);
    assert_eq!(node.qname(), "name");
}

#[test]
fn test_node_data_qname_with_empty_prefix() {
    let node = NodeData::element(1, "name".to_string(), Some("".to_string()), None);
    assert_eq!(node.qname(), "name");
}

#[test]
fn test_node_data_debug() {
    let node = NodeData::element(1, "test".to_string(), None, None);
    let debug = format!("{:?}", node);
    assert!(debug.contains("NodeData"));
    assert!(debug.contains("test"));
}

// XmlNode tests (using parse helper)
fn create_test_document() -> crate::document::XmlDocument {
    crate::parse("<root attr=\"val\"><child>text</child></root>").unwrap()
}

#[test]
fn test_xml_node_id() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    // Root node should have id > 0 (document is 0)
    assert!(root.id() > 0);
}

#[test]
fn test_xml_node_get_type() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    assert_eq!(root.get_type(), NodeType::Element);
}

#[test]
fn test_xml_node_get_name() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "root");
}

#[test]
fn test_xml_node_get_attribute() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    assert_eq!(root.get_attribute("attr"), Some("val".to_string()));
    assert_eq!(root.get_attribute("nonexistent"), None);
}

#[test]
fn test_xml_node_get_attributes() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let attrs = root.get_attributes();
    assert_eq!(attrs.get("attr"), Some(&"val".to_string()));
}

#[test]
fn test_xml_node_get_child_nodes() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let children = root.get_child_nodes();
    assert!(!children.is_empty());
}

#[test]
fn test_xml_node_get_child_elements() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let children = root.get_child_elements();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name(), "child");
}

#[test]
fn test_xml_node_first_child() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let first = root.first_child();
    assert!(first.is_some());
}

#[test]
fn test_xml_node_last_child() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let last = root.last_child();
    assert!(last.is_some());
}

#[test]
fn test_xml_node_get_parent() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    let parent = child.get_parent();
    assert!(parent.is_some());
    assert_eq!(parent.unwrap().get_name(), "root");
}

#[test]
fn test_xml_node_get_content_element() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    assert_eq!(child.get_content(), Some("text".to_string()));
}

#[test]
fn test_xml_node_is_element() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    assert!(root.is_element());
}

#[test]
fn test_xml_node_is_text() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    let text_node = child.get_child_nodes().into_iter().find(|n| n.is_text());
    assert!(text_node.is_some());
}

#[test]
fn test_xml_node_set_attribute() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_attribute("new_attr", "new_value");
    assert_eq!(
        root.get_attribute("new_attr"),
        Some("new_value".to_string())
    );
}

#[test]
fn test_xml_node_remove_attribute() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let removed = root.remove_attribute("attr");
    assert_eq!(removed, Some("val".to_string()));
    assert_eq!(root.get_attribute("attr"), None);
}

#[test]
fn test_xml_node_remove_attribute_nonexistent() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let removed = root.remove_attribute("nonexistent");
    assert_eq!(removed, None);
}

#[test]
fn test_xml_node_set_content() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    child.set_content("new content");
    // After set_content, children are cleared and content is set directly
}

#[test]
fn test_xml_node_set_name() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_name("newroot");
    assert_eq!(root.get_name(), "newroot");
}

#[test]
fn test_xml_node_set_prefix() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_prefix(Some("ns"));
    assert_eq!(root.get_prefix(), Some("ns".to_string()));
}

#[test]
fn test_xml_node_set_prefix_none() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_prefix(None);
    assert_eq!(root.get_prefix(), None);
}

#[test]
fn test_xml_node_set_namespace_uri() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_namespace_uri(Some("http://example.com"));
    assert_eq!(
        root.get_namespace_uri(),
        Some("http://example.com".to_string())
    );
}

#[test]
fn test_xml_node_add_namespace_decl() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.add_namespace_decl("ns", "http://example.com");
    let decls = root.get_namespace_declarations();
    assert!(
        decls
            .iter()
            .any(|d| d.prefix() == "ns" && d.uri() == "http://example.com")
    );
}

#[test]
fn test_xml_node_clear_children() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    assert!(!root.get_child_nodes().is_empty());
    root.clear_children();
    assert!(root.get_child_nodes().is_empty());
}

#[test]
fn test_xml_node_qname() {
    let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    assert_eq!(root.qname(), "ns:root");
}

#[test]
fn test_xml_node_debug() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let debug = format!("{:?}", root);
    assert!(debug.contains("XmlNode"));
    assert!(debug.contains("root"));
}

#[test]
fn test_xml_node_clone() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let cloned = root.clone();
    assert_eq!(root.id(), cloned.id());
    assert_eq!(root.get_name(), cloned.get_name());
}

#[test]
fn test_xml_node_eq() {
    let doc = create_test_document();
    let root1 = crate::get_root_node(&doc).unwrap();
    let root2 = crate::get_root_node(&doc).unwrap();
    assert_eq!(root1, root2);
}

#[test]
#[allow(clippy::mutable_key_type)]
fn test_xml_node_hash() {
    use std::collections::HashSet;
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let mut set = HashSet::new();
    set.insert(root.clone());
    assert!(set.contains(&root));
}

// XmlRoNode tests
#[test]
fn test_xml_ro_node_from_node() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root.clone());
    assert_eq!(ro_node.id(), root.id());
}

#[test]
fn test_xml_ro_node_get_type() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(ro_node.get_type(), NodeType::Element);
}

#[test]
fn test_xml_ro_node_get_name() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(ro_node.get_name(), "root");
}

#[test]
fn test_xml_ro_node_get_prefix() {
    let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(ro_node.get_prefix(), Some("ns".to_string()));
}

#[test]
fn test_xml_ro_node_get_namespace_uri() {
    // Test with manually set namespace URI
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_namespace_uri(Some("http://example.com"));
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(
        ro_node.get_namespace_uri(),
        Some("http://example.com".to_string())
    );
}

#[test]
fn test_xml_ro_node_get_namespace() {
    // Test with manually set namespace
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    root.set_prefix(Some("ns"));
    root.set_namespace_uri(Some("http://example.com"));
    let ro_node = XmlRoNode::from_node(root);
    let ns = ro_node.get_namespace();
    assert!(ns.is_some());
    let ns = ns.unwrap();
    assert_eq!(ns.prefix(), "ns");
    assert_eq!(ns.uri(), "http://example.com");
}

#[test]
fn test_xml_ro_node_qname() {
    let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(ro_node.qname(), "ns:root");
}

#[test]
fn test_xml_ro_node_get_content() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    let ro_node = XmlRoNode::from_node(child);
    assert_eq!(ro_node.get_content(), Some("text".to_string()));
}

#[test]
fn test_xml_ro_node_get_attribute() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert_eq!(ro_node.get_attribute("attr"), Some("val".to_string()));
}

#[test]
fn test_xml_ro_node_get_attributes() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let attrs = ro_node.get_attributes();
    assert!(attrs.contains_key("attr"));
}

#[test]
fn test_xml_ro_node_get_namespace_declarations() {
    let doc = crate::parse("<root xmlns:ns=\"http://example.com\"/>").unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let decls = ro_node.get_namespace_declarations();
    assert!(decls.iter().any(|d| d.prefix() == "ns"));
}

#[test]
fn test_xml_ro_node_get_parent() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    let ro_child = XmlRoNode::from_node(child);
    let parent = ro_child.get_parent();
    assert!(parent.is_some());
    assert_eq!(parent.unwrap().get_name(), "root");
}

#[test]
fn test_xml_ro_node_get_child_nodes() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let children = ro_node.get_child_nodes();
    assert!(!children.is_empty());
}

#[test]
fn test_xml_ro_node_get_child_elements() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let children = ro_node.get_child_elements();
    assert_eq!(children.len(), 1);
}

#[test]
fn test_xml_ro_node_first_child() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert!(ro_node.first_child().is_some());
}

#[test]
fn test_xml_ro_node_last_child() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert!(ro_node.last_child().is_some());
}

#[test]
fn test_xml_ro_node_is_element() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    assert!(ro_node.is_element());
}

#[test]
fn test_xml_ro_node_is_text() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let child = root.get_child_elements()[0].clone();
    let text_node = child
        .get_child_nodes()
        .into_iter()
        .find(|n| n.is_text())
        .unwrap();
    let ro_node = XmlRoNode::from_node(text_node);
    assert!(ro_node.is_text());
}

#[test]
fn test_xml_ro_node_into_node() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root.clone());
    let back_to_node = ro_node.into_node();
    assert_eq!(back_to_node.id(), root.id());
}

#[test]
fn test_xml_ro_node_debug() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let debug = format!("{:?}", ro_node);
    assert!(debug.contains("XmlRoNode"));
    assert!(debug.contains("root"));
}

#[test]
fn test_xml_ro_node_clone() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let cloned = ro_node.clone();
    assert_eq!(ro_node.id(), cloned.id());
}

#[test]
fn test_xml_ro_node_eq() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro1 = XmlRoNode::from_node(root.clone());
    let ro2 = XmlRoNode::from_node(root);
    assert_eq!(ro1, ro2);
}

#[test]
#[allow(clippy::mutable_key_type)]
fn test_xml_ro_node_hash() {
    use std::collections::HashSet;
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let mut set = HashSet::new();
    set.insert(ro_node.clone());
    assert!(set.contains(&ro_node));
}

// get_attribute_ns tests
#[test]
fn test_xml_node_get_attribute_ns() {
    let doc = crate::parse(r#"<root xmlns:ns="http://example.com" ns:attr="value"/>"#).unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    // Attributes are stored with local names only (libxml compatible)
    assert_eq!(root.get_attribute("attr"), Some("value".to_string()));
}

#[test]
fn test_xml_ro_node_get_attribute_ns() {
    let doc = crate::parse(r#"<root xmlns:ns="http://example.com" ns:attr="value"/>"#).unwrap();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    // get_attribute_ns searches by namespace URI
    let result = ro_node.get_attribute_ns("attr", "http://example.com");
    assert_eq!(result, Some("value".to_string()));
    // Attributes are keyed by local name only (libxml compatible)
    let attrs = ro_node.get_attributes();
    assert!(attrs.contains_key("attr"));
}

// line and column tests (usually None unless explicitly set during parsing)
#[test]
fn test_xml_node_line_column() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    // Line/column may or may not be available depending on parser config
    let _ = root.line();
    let _ = root.column();
}

#[test]
fn test_xml_ro_node_line_column() {
    let doc = create_test_document();
    let root = crate::get_root_node(&doc).unwrap();
    let ro_node = XmlRoNode::from_node(root);
    let _ = ro_node.line();
    let _ = ro_node.column();
}
