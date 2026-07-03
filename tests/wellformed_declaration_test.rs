//! Well-formedness of the XML declaration, in both the DOM and streaming
//! engines.

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
fn accepts_well_formed_declarations() {
    assert_accepted("<?xml version=\"1.0\"?><a/>", "a minimal declaration");
    assert_accepted(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><a/>",
        "a full declaration",
    );
}

#[test]
fn rejects_malformed_declarations() {
    assert_rejected("<?xml version=\"2.0\"?><a/>", "an unsupported version");
    assert_rejected("<?xml encoding=\"UTF-8\"?><a/>", "a missing version");
    assert_rejected(
        "<?xml version=\"1.0\" standalone=\"maybe\"?><a/>",
        "a bad standalone value",
    );
    assert_rejected(
        "<?xml version=\"1.0\" standalone=\"yes\" encoding=\"UTF-8\"?><a/>",
        "pseudo-attributes in the wrong order",
    );
    assert_rejected(
        "<?xml version=\"1.0\" encoding=\"UTF 8\"?><a/>",
        "an illegal encoding name",
    );
}

#[test]
fn rejects_declaration_not_at_start() {
    assert_rejected(
        " <?xml version=\"1.0\"?><a/>",
        "white space before the declaration",
    );
    assert_rejected(
        "<!-- c --><?xml version=\"1.0\"?><a/>",
        "a comment before the declaration",
    );
}
