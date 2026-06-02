//! Tests for the redesigned `Transformer` entry point.

use std::io::BufReader;

use fastxml::transform::Transformer;

const XML: &str = r#"<root><item id="1">a</item><item id="2">b</item></root>"#;

#[test]
fn in_memory_to_string() {
    let out = Transformer::from(XML)
        .on("//item[@id='2']", |node| {
            node.set_attribute("done", "1");
        })
        .to_string()
        .unwrap();
    assert!(out.contains(r#"done="1""#), "output was: {out}");
    // The untouched item is preserved verbatim.
    assert!(
        out.contains(r#"<item id="1">a</item>"#),
        "output was: {out}"
    );
}

#[test]
fn in_memory_write_to_returns_count() {
    let mut buf = Vec::new();
    let count = Transformer::from(XML)
        .on("//item", |node| {
            node.set_attribute("seen", "1");
        })
        .write_to(&mut buf)
        .unwrap();
    assert_eq!(count, 2);
    let s = String::from_utf8(buf).unwrap();
    assert_eq!(s.matches(r#"seen="1""#).count(), 2, "output was: {s}");
}

#[test]
fn reader_into_bytes() {
    let out = Transformer::from_reader(BufReader::new(XML.as_bytes()))
        .on("//item", |node| {
            node.set_attribute("seen", "1");
        })
        .into_bytes()
        .unwrap();
    let s = String::from_utf8(out).unwrap();
    assert_eq!(s.matches(r#"seen="1""#).count(), 2, "output was: {s}");
}

#[test]
fn reader_write_to_counts_matches() {
    let mut buf = Vec::new();
    let count = Transformer::from_reader(BufReader::new(XML.as_bytes()))
        .on("//item", |node| {
            node.set_attribute("seen", "1");
        })
        .write_to(&mut buf)
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn for_each_collects_side_effects() {
    let mut contents = Vec::new();
    Transformer::from(XML)
        .on("//item", |node| {
            contents.push(node.get_content().unwrap_or_default());
        })
        .for_each()
        .unwrap();
    assert_eq!(contents, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn namespaces_bind_prefix() {
    let xml = r#"<r xmlns:n="urn:x"><n:item>v</n:item></r>"#;
    let mut hits = 0;
    Transformer::from(xml)
        .namespace("n", "urn:x")
        .on("//n:item", |_node| {
            hits += 1;
        })
        .for_each()
        .unwrap();
    assert_eq!(hits, 1);
}
