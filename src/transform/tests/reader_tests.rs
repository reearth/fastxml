//! Tests for StreamTransformerReader.

use std::cell::RefCell;
use std::rc::Rc;

use crate::transform::{StreamTransformerReader, TransformError};

fn make_reader(xml: &str) -> std::io::BufReader<std::io::Cursor<&[u8]>> {
    std::io::BufReader::new(std::io::Cursor::new(xml.as_bytes()))
}

#[test]
fn test_reader_single_handler_transform() {
    let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

    let mut output = Vec::new();
    let count = StreamTransformerReader::new(make_reader(xml))
        .on("//item[@id='2']", |node| {
            node.set_attribute("modified", "true");
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 1);
    assert!(result.contains(r#"modified="true""#));
    assert!(result.contains("id=\"1\""));
}

#[test]
fn test_reader_multiple_handlers() {
    let xml = r#"<root><item>A</item><other>B</other></root>"#;

    let mut output = Vec::new();
    let count = StreamTransformerReader::new(make_reader(xml))
        .on("//item", |node| {
            node.set_attribute("type", "item");
        })
        .on("//other", |node| {
            node.set_attribute("type", "other");
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert_eq!(count, 2);
    assert!(result.contains(r#"type="item""#));
    assert!(result.contains(r#"type="other""#));
}

#[test]
fn test_reader_for_each() {
    let xml = r#"<root><item id="1"/><item id="2"/></root>"#;

    let mut ids = Vec::new();
    StreamTransformerReader::new(make_reader(xml))
        .on("//item", |node| {
            if let Some(id) = node.get_attribute("id") {
                ids.push(id);
            }
        })
        .for_each()
        .unwrap();

    assert_eq!(ids, vec!["1", "2"]);
}

#[test]
fn test_reader_for_each_multiple_handlers() {
    let xml = r#"<root><item>A</item><other>B</other></root>"#;

    let mut items = Vec::new();
    let mut others = Vec::new();

    StreamTransformerReader::new(make_reader(xml))
        .on("//item", |node| {
            items.push(node.get_content().unwrap_or_default());
        })
        .on("//other", |node| {
            others.push(node.get_content().unwrap_or_default());
        })
        .for_each()
        .unwrap();

    assert_eq!(items, vec!["A"]);
    assert_eq!(others, vec!["B"]);
}

#[test]
fn test_reader_with_namespaces() {
    let xml = r#"<root xmlns:ns="http://example.com"><ns:item/></root>"#;

    let mut output = Vec::new();
    StreamTransformerReader::new(make_reader(xml))
        .namespace("ns", "http://example.com")
        .on("//ns:item", |node| {
            node.set_attribute("found", "true");
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert!(result.contains(r#"found="true""#));
}

#[test]
fn test_reader_remove_element() {
    let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep></root>"#;

    let mut output = Vec::new();
    StreamTransformerReader::new(make_reader(xml))
        .on("//remove", |node| {
            node.remove();
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert!(!result.contains("<remove>"));
    assert!(result.contains("A"));
    assert!(result.contains("C"));
}

#[test]
fn test_reader_no_handlers_error() {
    let xml = "<root/>";
    let result = StreamTransformerReader::new(make_reader(xml)).run_to_writer(&mut Vec::new());
    assert!(result.is_err());
}

#[test]
fn test_reader_for_each_no_handlers_error() {
    let xml = "<root/>";
    let result = StreamTransformerReader::new(make_reader(xml)).for_each();
    assert!(result.is_err());
}

#[test]
fn test_reader_not_streamable_error() {
    let xml = "<root><item>A</item></root>";
    let result = StreamTransformerReader::new(make_reader(xml))
        .on("//item[last()]", |node| {
            node.set_attribute("last", "true");
        })
        .run_to_writer(&mut Vec::new());

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, TransformError::NotStreamable { .. }));
}

#[test]
fn test_reader_empty_elements() {
    let xml = r#"<root><item/><item/><item/></root>"#;

    let mut output = Vec::new();
    let count = StreamTransformerReader::new(make_reader(xml))
        .on("//item", |node| {
            node.set_attribute("found", "true");
        })
        .run_to_writer(&mut output)
        .unwrap();

    assert_eq!(count, 3);
    let result = String::from_utf8(output).unwrap();
    assert_eq!(result.matches(r#"found="true""#).count(), 3);
}

#[test]
fn test_reader_preserves_xml_declaration() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><root><item>A</item></root>"#;

    let mut output = Vec::new();
    StreamTransformerReader::new(make_reader(xml))
        .on("//item", |node| {
            node.set_attribute("ok", "true");
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert!(result.contains("<?xml"));
    assert!(result.contains(r#"ok="true""#));
}

#[test]
fn test_reader_nested_elements() {
    let xml = r#"<root><parent><child>text</child></parent></root>"#;

    let mut output = Vec::new();
    StreamTransformerReader::new(make_reader(xml))
        .on("//child", |node| {
            node.set_attribute("level", "deep");
        })
        .run_to_writer(&mut output)
        .unwrap();

    let result = String::from_utf8(output).unwrap();
    assert!(result.contains(r#"level="deep""#));
    assert!(result.contains("text"));
}

#[test]
fn test_reader_write_to_sink() {
    // Verify that writing to sink works (memory-efficient path)
    let xml = r#"<root><item id="1"/><item id="2"/></root>"#;

    let count = StreamTransformerReader::new(make_reader(xml))
        .on("//item", |_node| {})
        .run_to_writer(&mut std::io::sink())
        .unwrap();

    assert_eq!(count, 2);
}

#[test]
fn test_reader_namespaces_method() {
    let xml = r#"<root xmlns:a="http://a.com" xmlns:b="http://b.com"><a:item/><b:item/></root>"#;

    let found = Rc::new(RefCell::new(Vec::new()));
    let found_a = found.clone();
    let found_b = found.clone();
    StreamTransformerReader::new(make_reader(xml))
        .namespaces([("a", "http://a.com"), ("b", "http://b.com")])
        .on("//a:item", move |_| found_a.borrow_mut().push("a"))
        .on("//b:item", move |_| found_b.borrow_mut().push("b"))
        .for_each()
        .unwrap();

    assert_eq!(*found.borrow(), vec!["a", "b"]);
}
