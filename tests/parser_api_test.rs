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

#[test]
fn for_each_event_borrows_local_state() {
    // The closure captures and mutates a local — constant memory, no Arc needed.
    let mut starts = 0;
    Parser::from(XML)
        .for_each_event(|event| {
            if matches!(event, XmlEvent::StartElement { .. }) {
                starts += 1;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(starts, 3); // root, a, b
}

#[test]
fn for_each_event_from_reader() {
    let mut texts = Vec::new();
    Parser::from_reader(BufReader::new(XML.as_bytes()))
        .for_each_event(|event| {
            if let XmlEvent::Text(t) = event {
                texts.push(t.clone());
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(texts, vec!["1".to_string(), "2".to_string()]);
}
