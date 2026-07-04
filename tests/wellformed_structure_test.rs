//! Well-formedness: the top-level document grammar
//! `document ::= prolog element Misc*`. Both the DOM and streaming parsers must
//! agree on which documents are structurally well-formed.

use fastxml::Parser;
use std::io::Cursor;

/// Returns `(dom_rejected, streaming_rejected)`.
fn rejected_by_both(xml: &str) -> (bool, bool) {
    let dom = Parser::from(xml.as_bytes()).parse().is_err();
    let streaming = Parser::from_reader(Cursor::new(xml.as_bytes().to_vec()))
        .for_each_event(|_| Ok(()))
        .is_err();
    (dom, streaming)
}

/// Asserts both engines reject `xml`.
fn assert_rejected(xml: &str, why: &str) {
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject {why}: {xml:?}");
    assert!(streaming, "streaming should reject {why}: {xml:?}");
}

/// Asserts both engines accept `xml`.
fn assert_accepted(xml: &str, why: &str) {
    let (dom, streaming) = rejected_by_both(xml);
    assert!(!dom, "DOM should accept {why}: {xml:?}");
    assert!(!streaming, "streaming should accept {why}: {xml:?}");
}

// --- rejections ---

#[test]
fn rejects_no_document_element() {
    assert_rejected("", "an empty document");
    assert_rejected("   \n  ", "a whitespace-only document");
    assert_rejected("<!-- just a comment -->", "a comment-only document");
}

#[test]
fn rejects_second_root_element() {
    assert_rejected("<a/><b/>", "a second document element");
    assert_rejected("<a></a><b></b>", "a second document element");
}

#[test]
fn rejects_content_after_root() {
    assert_rejected("<a/>trailing", "text after the root element");
    assert_rejected("<a/>&#32;", "a character reference after the root element");
}

#[test]
fn rejects_content_before_root() {
    assert_rejected("illegal<a/>", "text before the root element");
    assert_rejected("<![CDATA[x]]><a/>", "a CDATA section in the prolog");
}

#[test]
fn rejects_cdata_outside_root() {
    assert_rejected("<a/><![CDATA[x]]>", "a CDATA section in the epilog");
}

#[test]
fn rejects_unclosed_root() {
    assert_rejected("<a>", "an unclosed root element");
    assert_rejected("<a><b></b>", "an unclosed root element");
}

#[test]
fn rejects_cdata_close_in_char_data() {
    assert_rejected("<a>]]></a>", "']]>' in character data");
    assert_rejected("<a>x]]]>y</a>", "']]>' in character data");
}

#[test]
fn rejects_less_than_in_attribute_value() {
    assert_rejected("<a x=\"<\"/>", "'<' in an attribute value");
    assert_rejected("<a x=\"a<b\"></a>", "'<' in an attribute value");
}

#[test]
fn rejects_bad_pi_target() {
    assert_rejected("<a><? ?></a>", "a processing instruction with no target");
    // Note: `<?xml ...?>` is tokenized as an XML declaration, not a PI, so the
    // reserved-target rule is exercised with a differently-cased target.
    assert_rejected("<a><?XmL foo?></a>", "the reserved target 'xml' (any case)");
}

#[test]
fn rejects_misplaced_or_duplicate_doctype() {
    assert_rejected("<a/><!DOCTYPE a>", "a DOCTYPE after the document element");
    assert_rejected(
        "<!DOCTYPE a><!DOCTYPE a><a/>",
        "a duplicate DOCTYPE declaration",
    );
}

// --- acceptances (no false positives) ---

#[test]
fn accepts_well_formed_prolog_and_epilog() {
    assert_accepted(
        "<?xml version=\"1.0\"?><!-- c --><?pi data?>\n<root/>\n<!-- after -->\n<?pi x?>",
        "comments, PIs and white space around the root",
    );
}

#[test]
fn accepts_escaped_gt_after_brackets_in_text() {
    // ']]&gt;' is the well-formed way to write two brackets then '>'.
    assert_accepted("<a>x]]&gt;y</a>", "']]&gt;' in character data");
}

#[test]
fn accepts_gt_inside_internal_subset_literal() {
    // quick-xml truncates the DOCTYPE at the '>' inside the entity value; the
    // parser must reassemble the internal subset rather than treat the tail as
    // prolog text.
    assert_accepted(
        "<!DOCTYPE foo [\n<!ELEMENT foo ANY>\n<!ENTITY gt \">\">\n]>\n<foo>ok</foo>",
        "a '>' inside an internal-subset entity value",
    );
}

#[test]
fn accepts_non_reserved_xml_prefixed_pi_target() {
    assert_accepted(
        "<a><?xml-stylesheet href=\"x\"?></a>",
        "a PI target that merely starts with 'xml'",
    );
}
