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
fn rejects_bad_qname_syntax() {
    // fastxml is namespace-aware, so a QName must be an NCName or
    // prefix:local with both parts non-empty NCNames.
    assert_rejected("<a:b:c/>", "an element name with two colons");
    assert_rejected("<foo:/>", "an element name ending in a colon");
    assert_rejected("<:foo/>", "an element name starting with a colon");
    assert_rejected("<foo a:b:attr=\"1\"/>", "an attribute name with two colons");
    // A bare-colon attribute name is well-formed XML 1.0 but not a valid QName.
    assert_rejected("<doc :=\"v\"></doc>", "an attribute named ':'");
}

#[test]
fn rejects_unbound_prefixes() {
    assert_rejected("<a:foo/>", "an unbound element prefix");
    assert_rejected("<foo a:attr=\"1\"/>", "an unbound attribute prefix");
    // The reserved 'xml' prefix is always bound and needs no declaration.
    assert_accepted("<foo xml:lang=\"en\"/>", "the always-bound 'xml' prefix");
}

#[test]
fn rejects_reserved_namespace_bindings() {
    assert_rejected(
        "<foo xmlns:yml=\"http://www.w3.org/XML/1998/namespace\"/>",
        "binding a non-'xml' prefix to the XML namespace",
    );
    assert_rejected(
        "<foo xmlns:ymlns=\"http://www.w3.org/2000/xmlns/\"/>",
        "binding a prefix to the reserved xmlns namespace",
    );
    assert_accepted(
        "<foo xmlns:xml=\"http://www.w3.org/XML/1998/namespace\"/>",
        "declaring the 'xml' prefix with its own namespace",
    );
    assert_rejected(
        "<foo xmlns=\"http://www.w3.org/XML/1998/namespace\"/>",
        "the XML namespace as the default namespace",
    );
    assert_rejected(
        "<foo xmlns=\"http://www.w3.org/2000/xmlns/\"/>",
        "the xmlns namespace as the default namespace",
    );
}

#[test]
fn rejects_colon_in_pi_target() {
    assert_rejected("<?a:b bogus?><foo/>", "a colon in a PI target");
}

#[test]
fn rejects_colon_in_dtd_names() {
    assert_rejected(
        "<!DOCTYPE foo [ <!ELEMENT foo ANY> <!ENTITY a:b \"bogus\"> ]><foo/>",
        "a colon in an entity name",
    );
    assert_rejected(
        "<!DOCTYPE foo [ <!ELEMENT foo ANY> <!NOTATION a:b SYSTEM \"n\"> ]><foo/>",
        "a colon in a notation name",
    );
}
