//! Well-formedness of character references and general-entity references, in
//! both the DOM and streaming engines.

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
fn rejects_malformed_character_references() {
    assert_rejected("<a>&#56.0;</a>", "a decimal char ref with a non-digit");
    assert_rejected("<a x=\"&#x003a\"/>", "an unterminated char ref");
    assert_rejected("<a>&#;</a>", "an empty char ref");
}

#[test]
fn rejects_illegal_character_reference_values() {
    assert_rejected("<a>&#0;</a>", "a reference to NUL");
    assert_rejected("<a>&#xB;</a>", "a reference to a C0 control");
    assert_rejected("<a>&#xD800;</a>", "a reference to a surrogate");
}

#[test]
fn accepts_well_formed_character_references() {
    assert_accepted("<a>x &#60; y &#x41;</a>", "legal char refs in text");
    assert_accepted("<a x=\"&#38;&#x2f;\"/>", "legal char refs in an attribute");
}

#[test]
fn rejects_entity_reference_cycle() {
    assert_rejected(
        "<!DOCTYPE d [<!ENTITY e1 \"&e2;\"><!ENTITY e2 \"&e3;\"><!ENTITY e3 \"&e1;\">]>\n<d>&e1;</d>",
        "a general-entity reference cycle in content",
    );
    assert_rejected(
        "<!DOCTYPE d [<!ENTITY e1 \"&e2;\"><!ENTITY e2 \"&e1;\">]>\n<d a=\"&e1;\"></d>",
        "a general-entity reference cycle in an attribute",
    );
}

#[test]
fn rejects_use_of_entity_referencing_undeclared() {
    assert_rejected(
        "<!DOCTYPE d [<!ENTITY foo \"&bar;\">]>\n<d a=\"&foo;\"></d>",
        "an entity whose value references an undeclared entity",
    );
}

#[test]
fn accepts_chained_entities_and_cdata_values() {
    assert_accepted(
        "<!DOCTYPE d [<!ENTITY a \"A\"><!ENTITY b \"x&a;y\">]>\n<d>&b;</d>",
        "a well-formed entity chain",
    );
    assert_accepted(
        "<!DOCTYPE d [<!ENTITY e \"<![CDATA[&foo;]]>\">]>\n<d>&e;</d>",
        "an undeclared name inside a CDATA section within an entity value",
    );
}
