//! Well-formedness: the XML `Char` production forbids C0 control characters
//! (other than tab, LF, CR) and other non-`Char` codepoints anywhere in a
//! document. Both the DOM and streaming parsers must reject them.
//!
//! Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]

use fastxml::Parser;

/// Parses via both the DOM and the streaming path, returning whether each
/// rejected the input.
fn rejected_by_both(xml: &str) -> (bool, bool) {
    let dom = Parser::from(xml).parse().is_err();
    let streaming = Parser::from(xml).for_each_event(|_| Ok(())).is_err();
    (dom, streaming)
}

#[test]
fn rejects_illegal_control_char_in_text() {
    let xml = "<root>a\u{0C}b</root>"; // form feed in text
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject form feed in text");
    assert!(streaming, "streaming should reject form feed in text");
}

#[test]
fn rejects_null_char_in_text() {
    let xml = "<root>\u{0}</root>";
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject NUL in text");
    assert!(streaming, "streaming should reject NUL in text");
}

#[test]
fn rejects_illegal_control_char_in_attribute_value() {
    let xml = "<root a=\"x\u{0B}y\"/>"; // vertical tab in attribute value
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject vertical tab in attribute value");
    assert!(
        streaming,
        "streaming should reject vertical tab in attribute value"
    );
}

#[test]
fn rejects_illegal_control_char_in_element_name() {
    let xml = "<ro\u{0C}ot></ro\u{0C}ot>"; // form feed inside a name
    let (dom, streaming) = rejected_by_both(xml);
    assert!(dom, "DOM should reject form feed in element name");
    assert!(
        streaming,
        "streaming should reject form feed in element name"
    );
}

// --- Valid documents must keep parsing (no false positives) ---

#[test]
fn accepts_tab_lf_cr_and_unicode_text() {
    // Tab, LF, CR are legal Chars; so are ordinary Unicode letters.
    let xml = "<root attr=\"a\tb\">line1\nline2\r\né 日本語</root>";
    let dom = Parser::from(xml).parse();
    let streaming = Parser::from(xml).for_each_event(|_| Ok(()));
    assert!(dom.is_ok(), "DOM rejected a valid document: {dom:?}");
    assert!(
        streaming.is_ok(),
        "streaming rejected a valid document: {streaming:?}"
    );
}

#[test]
fn accepts_high_unicode_in_content() {
    // Astral-plane char (U+1F600) is a legal Char.
    let xml = "<root>\u{1F600}</root>";
    assert!(Parser::from(xml).parse().is_ok());
    assert!(Parser::from(xml).for_each_event(|_| Ok(())).is_ok());
}
