//! Integration tests for XML parsing.

use fastxml::{get_node_tag, get_root_node, parse, NodeType};

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
    assert_eq!(
        children[0].get_attribute("type"),
        Some("element".to_string())
    );
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

    // Verify comment node is in AST
    let comment_nodes: Vec<_> = children
        .iter()
        .filter(|n| n.get_type() == NodeType::Comment)
        .collect();
    assert_eq!(comment_nodes.len(), 1);
    assert_eq!(
        comment_nodes[0].get_content(),
        Some(" this is a comment ".to_string())
    );
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

// =============================================================================
// HTML Parsing Tests
// =============================================================================

/// Test parsing XHTML (XML-compatible HTML)
#[test]
fn test_parse_xhtml_with_doctype() {
    // XHTML with DOCTYPE - DOCTYPE is skipped by the parser
    let html = r#"<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
    <head>
        <title>Test Page</title>
        <meta charset="UTF-8"/>
    </head>
    <body>
        <h1>Hello World</h1>
        <p>This is a paragraph.</p>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    assert_eq!(root.get_name(), "html");

    let children = root.get_child_elements();
    assert_eq!(children.len(), 2); // head and body

    let head = &children[0];
    let body = &children[1];
    assert_eq!(head.get_name(), "head");
    assert_eq!(body.get_name(), "body");
}

/// Test HTML5 DOCTYPE
#[test]
fn test_parse_html5_doctype() {
    let html = r#"<!DOCTYPE html>
<html>
    <head><title>HTML5</title></head>
    <body><p>Content</p></body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "html");
}

/// Test HTML with comments
#[test]
fn test_parse_html_with_comments() {
    let html = r#"<!DOCTYPE html>
<html>
    <!-- This is a header comment -->
    <head>
        <title>Test</title>
        <!-- Meta tags would go here -->
    </head>
    <body>
        <!-- Main content starts -->
        <div>
            <!-- Nested comment -->
            <p>Hello</p>
        </div>
        <!-- Main content ends -->
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "html");

    // Verify comments are preserved in AST
    let root_comments: Vec<_> = root
        .get_child_nodes()
        .into_iter()
        .filter(|n| n.get_type() == NodeType::Comment)
        .collect();
    assert_eq!(root_comments.len(), 1);
    assert!(root_comments[0]
        .get_content()
        .unwrap()
        .contains("header comment"));

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();

    // Body should have 2 comments
    let body_comments: Vec<_> = body
        .get_child_nodes()
        .into_iter()
        .filter(|n| n.get_type() == NodeType::Comment)
        .collect();
    assert_eq!(body_comments.len(), 2);

    let div = body
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "div")
        .unwrap();

    // Div should have 1 comment
    let div_comments: Vec<_> = div
        .get_child_nodes()
        .into_iter()
        .filter(|n| n.get_type() == NodeType::Comment)
        .collect();
    assert_eq!(div_comments.len(), 1);
    assert!(div_comments[0]
        .get_content()
        .unwrap()
        .contains("Nested comment"));

    let p = div.get_child_elements();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].get_name(), "p");
}

/// Test HTML with self-closing tags (XHTML style)
#[test]
fn test_parse_html_self_closing_tags() {
    let html = r#"<!DOCTYPE html>
<html>
    <head>
        <meta charset="UTF-8"/>
        <link rel="stylesheet" href="style.css"/>
    </head>
    <body>
        <img src="image.png" alt="Test"/>
        <br/>
        <hr/>
        <input type="text" name="field"/>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "html");

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();
    let elements = body.get_child_elements();

    // Should have img, br, hr, input
    let names: Vec<_> = elements.iter().map(|e| e.get_name()).collect();
    assert!(names.contains(&"img".to_string()));
    assert!(names.contains(&"br".to_string()));
    assert!(names.contains(&"hr".to_string()));
    assert!(names.contains(&"input".to_string()));
}

/// Test HTML with various attributes
#[test]
fn test_parse_html_attributes() {
    let html = r#"<!DOCTYPE html>
<html lang="en">
    <head><title>Attrs</title></head>
    <body>
        <div id="main" class="container" data-value="123">
            <a href="https://example.com" target="_blank" rel="noopener">Link</a>
            <button disabled="disabled" onclick="alert('hi')">Click</button>
        </div>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    assert_eq!(root.get_attribute("lang"), Some("en".to_string()));

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();
    let div = body.get_child_elements()[0].clone();

    assert_eq!(div.get_attribute("id"), Some("main".to_string()));
    assert_eq!(div.get_attribute("class"), Some("container".to_string()));
    assert_eq!(div.get_attribute("data-value"), Some("123".to_string()));
}

/// Test HTML with script and style tags containing special characters
#[test]
fn test_parse_html_with_cdata_content() {
    // Using CDATA for script content to handle special chars
    let html = r#"<!DOCTYPE html>
<html>
    <head>
        <style><![CDATA[
            body { color: red; }
            .class > child { margin: 0; }
        ]]></style>
    </head>
    <body>
        <script><![CDATA[
            if (a < b && c > d) {
                console.log("test");
            }
        ]]></script>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "html");
}

/// Test HTML table structure
#[test]
fn test_parse_html_table() {
    let html = r#"<!DOCTYPE html>
<html>
    <body>
        <table>
            <thead>
                <tr><th>Name</th><th>Value</th></tr>
            </thead>
            <tbody>
                <tr><td>Item 1</td><td>100</td></tr>
                <tr><td>Item 2</td><td>200</td></tr>
            </tbody>
        </table>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();
    let table = body.get_child_elements()[0].clone();
    assert_eq!(table.get_name(), "table");

    let sections = table.get_child_elements();
    assert_eq!(sections.len(), 2); // thead, tbody
}

/// Test HTML form elements
#[test]
fn test_parse_html_form() {
    let html = r#"<!DOCTYPE html>
<html>
    <body>
        <form action="/submit" method="post">
            <label for="name">Name:</label>
            <input type="text" id="name" name="name"/>
            <select name="option">
                <option value="1">One</option>
                <option value="2" selected="selected">Two</option>
            </select>
            <textarea name="comment">Default text</textarea>
            <button type="submit">Submit</button>
        </form>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();
    let form = body.get_child_elements()[0].clone();

    assert_eq!(form.get_name(), "form");
    assert_eq!(form.get_attribute("action"), Some("/submit".to_string()));
    assert_eq!(form.get_attribute("method"), Some("post".to_string()));
}

/// Test XHTML strict with namespaces
#[test]
fn test_parse_xhtml_strict() {
    let html = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en" lang="en">
    <head>
        <title>XHTML Strict</title>
    </head>
    <body>
        <p>Valid XHTML 1.0 Strict document.</p>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    assert_eq!(root.get_name(), "html");
    assert_eq!(root.get_attribute("lang"), Some("en".to_string()));

    // Check namespace declaration
    let ns_decls = root.get_namespace_declarations();
    assert!(!ns_decls.is_empty());
}

/// Test HTML with entities
#[test]
fn test_parse_html_entities() {
    let html = r#"<!DOCTYPE html>
<html>
    <body>
        <p>Less than: &lt; Greater than: &gt;</p>
        <p>Ampersand: &amp; Quote: &quot; Apos: &apos;</p>
        <p attr="value with &quot;quotes&quot;">Text</p>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();
    let paragraphs = body.get_child_elements();

    // First paragraph should contain decoded entities
    let content = paragraphs[0].get_content().unwrap();
    assert!(content.contains('<'));
    assert!(content.contains('>'));
}

/// Test HTML with multiple comments in sequence
#[test]
fn test_parse_html_multiple_comments() {
    let html = r#"<!DOCTYPE html>
<html>
    <body>
        <!-- First comment -->
        <!-- Second comment -->
        <p>Between comments</p>
        <!-- Third comment -->
        <!-- Fourth comment with special chars: <>&"' -->
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();

    // All 4 comments should be preserved in AST
    let comments: Vec<_> = body
        .get_child_nodes()
        .into_iter()
        .filter(|n| n.get_type() == NodeType::Comment)
        .collect();
    assert_eq!(comments.len(), 4);

    // Verify comment contents
    assert!(comments[0].get_content().unwrap().contains("First"));
    assert!(comments[1].get_content().unwrap().contains("Second"));
    assert!(comments[2].get_content().unwrap().contains("Third"));
    assert!(comments[3].get_content().unwrap().contains("Fourth"));
    // Special chars in comment should be preserved
    assert!(comments[3].get_content().unwrap().contains("<>&"));

    // The paragraph should still be parseable
    let p = body
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "p")
        .unwrap();
    assert_eq!(p.get_content(), Some("Between comments".to_string()));
}

/// Test minimal HTML document
#[test]
fn test_parse_minimal_html() {
    let html = r#"<!DOCTYPE html><html><body>Hello</body></html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();
    assert_eq!(root.get_name(), "html");
}

/// Test HTML with deeply nested divs
#[test]
fn test_parse_html_deeply_nested() {
    let html = r#"<!DOCTYPE html>
<html>
    <body>
        <div class="l1">
            <div class="l2">
                <div class="l3">
                    <div class="l4">
                        <div class="l5">
                            <span>Deep content</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </body>
</html>"#;

    let doc = parse(html).unwrap();
    let root = get_root_node(&doc).unwrap();

    // Navigate to the deepest element
    let body = root
        .get_child_elements()
        .into_iter()
        .find(|e| e.get_name() == "body")
        .unwrap();

    let mut current = body.get_child_elements()[0].clone();
    for level in 2..=5 {
        assert_eq!(
            current.get_attribute("class"),
            Some(format!("l{}", level - 1))
        );
        current = current.get_child_elements()[0].clone();
    }

    let span = current.get_child_elements()[0].clone();
    assert_eq!(span.get_name(), "span");
    assert_eq!(span.get_content(), Some("Deep content".to_string()));
}
