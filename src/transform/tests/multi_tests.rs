//! Tests for collect_multi functionality.

use crate::transform::{EditableNode, StreamTransformer, TransformError, TransformResult};

#[test]
fn test_collect_multi_2_same_xpath() {
    let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

    let (ids, contents): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |node: &mut EditableNode| {
                node.get_attribute("id").unwrap_or_default()
            }),
            ("//item", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(ids, vec!["1", "2"]);
    assert_eq!(contents, vec!["A", "B"]);
}

#[test]
fn test_collect_multi_2_different_xpaths() {
    let xml = r#"<root><item>A</item><other>X</other><item>B</item></root>"#;

    let (items, others): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
            ("//other", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(items, vec!["A", "B"]);
    assert_eq!(others, vec!["X"]);
}

#[test]
fn test_collect_multi_3() {
    let xml = r#"<root><a>1</a><b>2</b><c>3</c></root>"#;

    let (a, b, c): (Vec<String>, Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//a", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
            ("//b", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
            ("//c", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(a, vec!["1"]);
    assert_eq!(b, vec!["2"]);
    assert_eq!(c, vec!["3"]);
}

#[test]
fn test_collect_multi_with_namespaces() {
    let xml = r#"<root xmlns:ns="http://example.com"><ns:item>A</ns:item><other>B</other></root>"#;

    let (items, others): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .namespace("ns", "http://example.com")
        .collect_multi((
            ("//ns:item", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
            ("//other", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(items, vec!["A"]);
    assert_eq!(others, vec!["B"]);
}

#[test]
fn test_collect_multi_different_types() {
    let xml = r#"<root><item id="1">true</item><item id="2">false</item></root>"#;

    let (ids, flags): (Vec<i32>, Vec<bool>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |node: &mut EditableNode| {
                node.get_attribute("id")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
            }),
            ("//item", |node: &mut EditableNode| {
                node.get_content().map(|s| s == "true").unwrap_or(false)
            }),
        ))
        .unwrap();

    assert_eq!(ids, vec![1, 2]);
    assert_eq!(flags, vec![true, false]);
}

#[test]
fn test_collect_multi_empty_results() {
    let xml = r#"<root><other>X</other></root>"#;

    let (items, others): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
            ("//other", |node: &mut EditableNode| {
                node.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert!(items.is_empty());
    assert_eq!(others, vec!["X"]);
}

#[test]
fn test_collect_multi_not_streamable_error() {
    let xml = r#"<root><item>A</item><item>B</item></root>"#;

    let result: TransformResult<(Vec<String>, Vec<String>)> = StreamTransformer::new(xml)
        .collect_multi((
            ("//item", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
            ("//item[last()]", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
        ));

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, TransformError::NotStreamable { .. }));
}

#[test]
fn test_collect_multi_with_fallback() {
    let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

    let (all, last): (Vec<String>, Vec<String>) = StreamTransformer::new(xml)
        .allow_fallback()
        .collect_multi((
            ("//item", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
            ("//item[last()]", |n: &mut EditableNode| {
                n.get_content().unwrap_or_default()
            }),
        ))
        .unwrap();

    assert_eq!(all, vec!["A", "B", "C"]);
    assert_eq!(last, vec!["C"]);
}
