//! Namespace well-formedness that does not conflict with plain XML 1.0 (where
//! a colon is an ordinary name character), in both engines.

use fastxml::Parser;
use std::io::Cursor;

fn rejected_by_both(xml: &str) -> (bool, bool) {
    let dom = Parser::from(xml.as_bytes()).parse().is_err();
    let streaming = Parser::from_reader(Cursor::new(xml.as_bytes().to_vec()))
        .for_each_event(|_| Ok(()))
        .is_err();
    (dom, streaming)
}

fn assert_rejected(xml: &str, why: &str) {
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject {why}: {xml:?}");
    assert!(streaming, "streaming should reject {why}: {xml:?}");
}

fn assert_accepted(xml: &str, why: &str) {
    let (dom, streaming) = rejected_by_both(xml);
    assert!(!dom, "DOM should accept {why}: {xml:?}");
    assert!(!streaming, "streaming should accept {why}: {xml:?}");
}

#[test]
fn rejects_bad_namespace_declarations() {
    assert_rejected(
        "<foo xmlns:=\"http://example.org/\"/>",
        "an empty prefix in an xmlns declaration",
    );
    assert_rejected(
        "<foo xmlns:xmlns=\"http://example.org/\"/>",
        "declaring the 'xmlns' prefix",
    );
    assert_rejected(
        "<foo xmlns:xml=\"http://wrong.example/\"/>",
        "binding 'xml' to the wrong namespace",
    );
    assert_rejected(
        "<a:foo xmlns:a=\"http://example.org/\"><a:bar xmlns:a=\"\"/></a:foo>",
        "unbinding a prefix (illegal in XML 1.0)",
    );
}

#[test]
fn rejects_duplicate_expanded_attribute() {
    assert_rejected(
        "<bar xmlns:a=\"http://example.org/x\" xmlns:b=\"http://example.org/x\" a:attr=\"1\" b:attr=\"2\"/>",
        "two attributes with the same expanded name",
    );
}

#[test]
fn accepts_correct_namespace_usage() {
    assert_accepted(
        "<foo xmlns:xml=\"http://www.w3.org/XML/1998/namespace\" xml:lang=\"en\"/>",
        "declaring 'xml' correctly and using it",
    );
    assert_accepted(
        "<bar xmlns:a=\"http://example.org/x\" xmlns:b=\"http://example.org/y\" a:attr=\"1\" b:attr=\"2\"/>",
        "same local name under different namespaces",
    );
}

#[test]
fn accepts_plain_xml_10_colons() {
    // Colons are ordinary name characters in XML 1.0; a namespace-aware QName
    // check must not reject these well-formed documents.
    assert_accepted("<doc :=\"v\"></doc>", "an attribute named ':'");
    assert_accepted("<a:b:c/>", "an element name with two colons");
}
