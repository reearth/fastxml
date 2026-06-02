//! Tests for the redesigned `Parser` entry point.

use std::io::BufReader;

use fastxml::Parser;
use fastxml::event::XmlEvent;

const XML: &str = r#"<root><a>1</a><b>2</b></root>"#;

fn root_name(doc: &fastxml::XmlDocument) -> String {
    doc.get_root_element().unwrap().get_name()
}

#[test]
fn parse_from_str() {
    let doc = Parser::from(XML).parse().unwrap();
    assert_eq!(root_name(&doc), "root");
}

#[test]
fn parse_from_bytes() {
    let doc = Parser::from(XML.as_bytes()).parse().unwrap();
    assert_eq!(root_name(&doc), "root");
}

#[test]
fn parse_from_reader() {
    let doc = Parser::from_reader(BufReader::new(XML.as_bytes()))
        .parse()
        .unwrap();
    assert_eq!(root_name(&doc), "root");
}

#[test]
fn parse_with_options_still_parses() {
    let opts = fastxml::ParserOptions::default();
    let doc = Parser::from(XML).options(opts).parse().unwrap();
    assert_eq!(root_name(&doc), "root");
}

#[test]
fn events_contains_root_start() {
    let events = Parser::from(XML).events().unwrap();
    assert!(!events.is_empty());
    let saw_root = events
        .iter()
        .any(|e| matches!(e, XmlEvent::StartElement { name, .. } if name.as_ref() == "root"));
    assert!(saw_root, "expected a StartElement for 'root'");
}

#[test]
fn events_from_reader() {
    let events = Parser::from_reader(BufReader::new(XML.as_bytes()))
        .events()
        .unwrap();
    let starts = events
        .iter()
        .filter(|e| matches!(e, XmlEvent::StartElement { .. }))
        .count();
    // root, a, b
    assert_eq!(starts, 3);
}
