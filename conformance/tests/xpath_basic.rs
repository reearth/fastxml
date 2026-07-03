//! Basic XPath evaluation unit tests.
//!
//! fastxml does not run any external standard XPath conformance suite (the
//! OASIS suite was never wired up and its data is not distributed). These
//! self-contained tests exercise the XPath evaluator directly. The test names
//! (`basic_xpath_evaluation`, `xpath_axes`, `xpath_string_functions`,
//! `xpath_number_functions`) are referenced by CI.

use fastxml::XmlContext;

/// Test basic XPath functionality.
#[test]
fn basic_xpath_evaluation() {
    let xml = br#"<?xml version="1.0"?>
<root>
  <item id="1">First</item>
  <item id="2">Second</item>
  <item id="3">Third</item>
</root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    let result = ctx.evaluate("//item").expect("evaluate //item");
    assert_eq!(result.into_nodes().len(), 3);

    let result = ctx
        .evaluate("string(//item[@id='1'])")
        .expect("evaluate string");
    assert_eq!(result.to_string_value(), "First");

    let result = ctx.evaluate("count(//item)").expect("evaluate count");
    assert_eq!(result.to_number() as i32, 3);

    let result = ctx.evaluate("//item[@id='2']").expect("evaluate predicate");
    assert_eq!(result.into_nodes().len(), 1);

    let result = ctx.evaluate("//item[2]").expect("evaluate position");
    assert_eq!(result.into_nodes().len(), 1);
}

/// Test XPath axes.
#[test]
fn xpath_axes() {
    let xml = br#"<?xml version="1.0"?>
<root>
  <parent>
    <child>
      <grandchild/>
    </child>
  </parent>
</root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    let result = ctx.evaluate("//parent/descendant::*").expect("descendant");
    assert_eq!(result.into_nodes().len(), 2); // child and grandchild

    let result = ctx.evaluate("//grandchild/ancestor::*").expect("ancestor");
    assert!(result.into_nodes().len() >= 3); // child, parent, root

    let result = ctx.evaluate("//parent/child::*").expect("child");
    assert_eq!(result.into_nodes().len(), 1);

    let result = ctx.evaluate("//child/parent::*").expect("parent");
    assert_eq!(result.into_nodes().len(), 1);
}

/// Test XPath string functions.
#[test]
fn xpath_string_functions() {
    let xml = br#"<?xml version="1.0"?><root><text>Hello World</text></root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    let result = ctx
        .evaluate("normalize-space('  Hello World  ')")
        .expect("normalize-space");
    assert_eq!(result.to_string_value(), "Hello World");

    let result = ctx
        .evaluate("contains('Hello World', 'World')")
        .expect("contains");
    assert!(result.to_boolean());

    let result = ctx
        .evaluate("starts-with('Hello World', 'Hello')")
        .expect("starts-with");
    assert!(result.to_boolean());

    let result = ctx
        .evaluate("string-length('test')")
        .expect("string-length");
    assert_eq!(result.to_number() as i32, 4);

    let result = ctx.evaluate("concat('a', 'b', 'c')").expect("concat");
    assert_eq!(result.to_string_value(), "abc");

    let result = ctx.evaluate("substring('12345', 2, 3)").expect("substring");
    assert_eq!(result.to_string_value(), "234");
}

/// Test XPath number functions.
#[test]
fn xpath_number_functions() {
    let xml = br#"<?xml version="1.0"?><root><n>42</n><n>-10</n><n>3.14</n></root>"#;

    let doc = fastxml::Parser::from(xml.as_slice())
        .parse()
        .expect("parse xml");
    let ctx = XmlContext::new(&doc);

    let result = ctx.evaluate("sum(//n)").expect("sum");
    assert!((result.to_number() - 35.14).abs() < 0.01);

    let result = ctx.evaluate("floor(3.7)").expect("floor");
    assert_eq!(result.to_number() as i32, 3);

    let result = ctx.evaluate("ceiling(3.2)").expect("ceiling");
    assert_eq!(result.to_number() as i32, 4);

    let result = ctx.evaluate("round(3.5)").expect("round");
    assert_eq!(result.to_number() as i32, 4);
}
