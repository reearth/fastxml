//! A UTF-8 byte order mark is recognized however the reader splits it.
//!
//! Readers may return short reads (network streams, chained readers,
//! decompressors), so the BOM can arrive in pieces.

use std::io::BufReader;

use fastxml::Parser;

const XML: &[u8] = b"\xEF\xBB\xBF<root>\r\n<a/></root>";

#[test]
fn dom_parse_from_a_reader_with_any_buffer_size() {
    let expected = Parser::from(XML).parse().unwrap();
    let root = expected.get_root_element().unwrap();
    for capacity in 1..=8 {
        let doc = Parser::from_reader(BufReader::with_capacity(capacity, XML))
            .parse()
            .unwrap_or_else(|e| panic!("capacity {capacity}: {e}"));
        let r = doc.get_root_element().unwrap();
        assert_eq!(r.get_name(), root.get_name(), "capacity {capacity}");
        assert_eq!(
            (r.line(), r.column()),
            (root.line(), root.column()),
            "capacity {capacity}"
        );
        assert_eq!(
            r.get_child_elements().len(),
            root.get_child_elements().len(),
            "capacity {capacity}"
        );
    }
}

#[test]
fn events_from_a_reader_with_any_buffer_size() {
    let expected = Parser::from(XML).events().unwrap();
    for capacity in 1..=8 {
        let events = Parser::from_reader(BufReader::with_capacity(capacity, XML))
            .events()
            .unwrap_or_else(|e| panic!("capacity {capacity}: {e}"));
        assert_eq!(
            format!("{events:?}"),
            format!("{expected:?}"),
            "capacity {capacity}"
        );
    }
}
