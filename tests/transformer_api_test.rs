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

#[test]
fn collect_extracts_values() {
    let ids: Vec<String> = Transformer::from(XML)
        .collect("//item", |node| {
            node.get_attribute("id").unwrap_or_default()
        })
        .unwrap();
    assert_eq!(ids, vec!["1".to_string(), "2".to_string()]);
}

#[test]
fn collect_multi_extracts_tuples() {
    let (ids, contents): (Vec<String>, Vec<String>) = Transformer::from(XML)
        .collect_multi((
            ("//item", |node: &mut fastxml::transform::EditableNode| {
                node.get_attribute("id").unwrap_or_default()
            }),
            ("//item", |node: &mut fastxml::transform::EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();
    assert_eq!(ids, vec!["1".to_string(), "2".to_string()]);
    assert_eq!(contents, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn on_with_context_receives_context() {
    let out = Transformer::from(XML)
        .on_with_context("//item", |node, ctx| {
            node.set_attribute("depth", &ctx.depth().to_string());
        })
        .to_string()
        .unwrap();
    assert!(out.contains("depth="), "output was: {out}");
}

#[test]
fn with_root_namespaces_auto_registers() {
    let xml = r#"<r xmlns:n="urn:x"><n:item>v</n:item></r>"#;
    let mut hits = 0;
    Transformer::from(xml)
        .with_root_namespaces()
        .unwrap()
        .on("//n:item", |_node| hits += 1)
        .for_each()
        .unwrap();
    assert_eq!(hits, 1);
}

#[test]
fn reader_rejects_collect() {
    let result = Transformer::from_reader(BufReader::new(XML.as_bytes()))
        .collect("//item", |node| {
            node.get_attribute("id").unwrap_or_default()
        });
    assert!(
        result.is_err(),
        "collect should be unsupported on reader input"
    );
}

#[test]
fn reader_defers_in_memory_only_op_to_terminal() {
    // on_with_context is in-memory only; on reader input the error surfaces
    // at the terminal rather than panicking mid-chain.
    let result = Transformer::from_reader(BufReader::new(XML.as_bytes()))
        .on_with_context("//item", |_node, _ctx| {})
        .to_string();
    assert!(result.is_err());
}
