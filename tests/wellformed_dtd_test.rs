//! Well-formedness of the `DOCTYPE` declaration and its internal subset. Both
//! the DOM and streaming parsers must agree, and neither may reject the many
//! well-formed internal subsets that appear in real documents.

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

// --- accepted internal subsets (no false positives) ---

#[test]
fn accepts_well_formed_subsets() {
    assert_accepted(
        "<!DOCTYPE s [ <!ELEMENT s (#PCDATA)> ]>\n<s>x</s>",
        "a PCDATA element declaration",
    );
    assert_accepted(
        "<!DOCTYPE s [\n<!ELEMENT s (a|b)+>\n<!ELEMENT a ANY>\n<!ELEMENT b EMPTY>\n]>\n<s><a/></s>",
        "a children content model",
    );
    assert_accepted(
        "<!DOCTYPE s [ <!ELEMENT s EMPTY> <!ATTLIST s x CDATA #REQUIRED y (a|b) 'a'> ]>\n<s x='v'/>",
        "an attribute-list declaration",
    );
    assert_accepted(
        "<!DOCTYPE s [ <!ENTITY e \"val\"> <!ENTITY % p \"x\"> ]>\n<s/>",
        "general and parameter entity declarations",
    );
    assert_accepted(
        "<!DOCTYPE s [ <!NOTATION n SYSTEM \"x\"> <!ENTITY g SYSTEM \"g.gif\" NDATA n> ]>\n<s/>",
        "a notation and an unparsed entity",
    );
    assert_accepted(
        "<!DOCTYPE s PUBLIC 'The latest version' 'student.dtd' [ ]>\n<s/>",
        "a PUBLIC external id with spaces in the public literal",
    );
    assert_accepted(
        "<!DOCTYPE s [ <?target some data?> <!-- a comment --> ]>\n<s/>",
        "a processing instruction and comment in the subset",
    );
}

// --- rejected internal subsets ---

#[test]
fn rejects_bad_pi_target_in_subset() {
    assert_rejected(
        "<!DOCTYPE s [ <? no target?> ]>\n<s/>",
        "a PI with no target in the subset",
    );
    assert_rejected(
        "<!DOCTYPE s [ <?xml v?> ]>\n<s/>",
        "the reserved target 'xml' in the subset",
    );
}

#[test]
fn rejects_conditional_section_in_internal_subset() {
    assert_rejected(
        "<!DOCTYPE s [ <![INCLUDE[ <!ELEMENT s ANY> ]]> ]>\n<s/>",
        "a conditional section in the internal subset",
    );
}

#[test]
fn rejects_malformed_declarations() {
    assert_rejected(
        "<!DOCTYPE s [ <!ELEMENT s (a,b|c)> ]>\n<s/>",
        "mixed ',' and '|' connectors",
    );
    assert_rejected(
        "<!DOCTYPE s [ <!ATTLIST s x WRONG #IMPLIED> ]>\n<s/>",
        "an unknown attribute type",
    );
    assert_rejected(
        "<!DOCTYPE s [ <!NOTATION n \"x\"> ]>\n<s/>",
        "a notation with no SYSTEM/PUBLIC keyword",
    );
    assert_rejected(
        "<!DOCTYPE s [ <!BOGUS foo> ]>\n<s/>",
        "an unknown markup declaration",
    );
}

#[test]
fn rejects_bad_external_id() {
    assert_rejected(
        "<!DOCTYPE s system \"s.dtd\">\n<s/>",
        "a lowercase external-id keyword",
    );
    assert_rejected(
        "<!DOCTYPE s PUBLIC \"a`b\" \"s.dtd\">\n<s/>",
        "an illegal PubidChar",
    );
}
