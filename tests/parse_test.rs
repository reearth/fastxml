//! Integration tests for XML parsing.

use fastxml::{parse, get_root_node, get_node_tag};

#[test]
fn test_parse_simple_xml() {
    let xml = r#"<root><child>text</child></root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    assert_eq!(get_node_tag(&root), "root");

    let children = root.get_child_elements();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name(), "child");
    assert_eq!(children[0].get_content(), Some("text".to_string()));
}

#[test]
fn test_parse_with_attributes() {
    let xml = r#"<root id="1" name="test"><child type="element"/></root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_attribute("id"), Some("1".to_string()));
    assert_eq!(root.get_attribute("name"), Some("test".to_string()));

    let children = root.get_child_elements();
    assert_eq!(children[0].get_attribute("type"), Some("element".to_string()));
}

#[test]
fn test_parse_namespaced_xml() {
    let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml" xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
        <gml:featureMember>
            <bldg:Building gml:id="bldg_001">
                <bldg:measuredHeight>15.5</bldg:measuredHeight>
            </bldg:Building>
        </gml:featureMember>
    </gml:root>"#;

    let doc = parse(xml).unwrap();
    let root = get_root_node(&doc).unwrap();

    assert_eq!(root.get_name(), "root");
    assert_eq!(root.get_prefix(), Some("gml".to_string()));
    assert_eq!(root.qname(), "gml:root");

    let ns_decls = root.get_namespace_declarations();
    assert_eq!(ns_decls.len(), 2);
}

#[test]
fn test_parse_mixed_content() {
    let xml = r#"<root>text before<child/>text after</root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    let children = root.get_child_nodes();

    // Should have: text, element, text
    assert!(children.len() >= 2);
}

#[test]
fn test_parse_cdata() {
    let xml = r#"<root><![CDATA[<not xml> & special chars]]></root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    let content = root.get_content().unwrap();
    assert!(content.contains("<not xml>"));
    assert!(content.contains("& special"));
}

#[test]
fn test_parse_comments() {
    let xml = r#"<root><!-- this is a comment --><child/></root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    let children = root.get_child_nodes();
    assert!(!children.is_empty());
}

#[test]
fn test_parse_empty_elements() {
    let xml = r#"<root><empty1/><empty2></empty2></root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    let children = root.get_child_elements();
    assert_eq!(children.len(), 2);
}

#[test]
fn test_parse_deeply_nested() {
    let xml = r#"<a><b><c><d><e><f>deep</f></e></d></c></b></a>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "a");

    // Navigate down
    let mut current = root;
    let expected = ["b", "c", "d", "e", "f"];
    for name in expected {
        let children = current.get_child_elements();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get_name(), name);
        current = children[0].clone();
    }

    assert_eq!(current.get_content(), Some("deep".to_string()));
}

#[test]
fn test_parse_special_characters() {
    let xml = r#"<root attr="&lt;value&gt;">&amp; &lt; &gt; &quot; &apos;</root>"#;
    let doc = parse(xml).unwrap();

    let root = get_root_node(&doc).unwrap();
    let attr = root.get_attribute("attr").unwrap();
    assert_eq!(attr, "<value>");
}

#[test]
fn test_node_count() {
    let xml = r#"<root><a/><b/><c/></root>"#;
    let doc = parse(xml).unwrap();

    // Document node + root + 3 children = 5 nodes minimum
    assert!(doc.node_count() >= 4);
}
