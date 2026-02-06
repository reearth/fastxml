//! Tests for context-aware handlers.

use crate::transform::StreamTransformer;

#[test]
fn test_on_with_context_parent() {
    let xml = r#"<root><items id="list1"><item>A</item><item>B</item></items></root>"#;

    let mut parent_names = Vec::new();
    let mut parent_ids = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            if let Some(parent) = ctx.parent() {
                parent_names.push(parent.name.clone());
                if let Some(id) = parent.attributes.get("id") {
                    parent_ids.push(id.clone());
                }
            }
        })
        .for_each()
        .unwrap();

    assert_eq!(parent_names, vec!["items", "items"]);
    assert_eq!(parent_ids, vec!["list1", "list1"]);
}

#[test]
fn test_on_with_context_position() {
    let xml = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

    let mut positions = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            positions.push(ctx.position());
        })
        .for_each()
        .unwrap();

    assert_eq!(positions, vec![1, 2, 3]);
}

#[test]
fn test_on_with_context_depth() {
    let xml = r#"<root><level1><level2><target/></level2></level1></root>"#;

    let mut depths = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//target", |_node, ctx| {
            depths.push(ctx.depth());
        })
        .for_each()
        .unwrap();

    // root=1, level1=2, level2=3, target=4
    assert_eq!(depths, vec![4]);
}

#[test]
fn test_on_with_context_ancestors() {
    let xml = r#"<root><a><b><target/></b></a></root>"#;

    let mut ancestor_names = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//target", |_node, ctx| {
            ancestor_names = ctx.ancestors().iter().map(|a| a.name.clone()).collect();
        })
        .for_each()
        .unwrap();

    assert_eq!(ancestor_names, vec!["root", "a", "b"]);
}

#[test]
fn test_on_with_context_path_id() {
    let xml = r#"<root><items><item/><item/></items><items><item/></items></root>"#;

    let mut paths = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            paths.push(ctx.path_id());
        })
        .for_each()
        .unwrap();

    // First items group: position 1
    // Second items group: position 2
    assert_eq!(paths, vec!["root/items", "root/items", "root/items[2]"]);
}

#[test]
fn test_on_with_context_transform() {
    let xml = r#"<root><items id="list1"><item/><item/></items></root>"#;

    let result = StreamTransformer::new(xml)
        .on_with_context("//item", |node, ctx| {
            let path = ctx.path_id();
            let pos = ctx.position();
            node.set_attribute("path", &format!("{}/item[{}]", path, pos));

            if let Some(parent_id) = ctx.parent_attribute("id") {
                node.set_attribute("parent_id", parent_id);
            }
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"path="root/items/item[1]""#));
    assert!(result.contains(r#"path="root/items/item[2]""#));
    assert!(result.contains(r#"parent_id="list1""#));
}

#[test]
fn test_on_with_context_empty_element() {
    let xml = r#"<root><item/></root>"#;

    let mut depths = Vec::new();
    let mut positions = Vec::new();

    StreamTransformer::new(xml)
        .on_with_context("//item", |_node, ctx| {
            depths.push(ctx.depth());
            positions.push(ctx.position());
        })
        .for_each()
        .unwrap();

    assert_eq!(depths, vec![2]);
    assert_eq!(positions, vec![1]);
}

#[test]
fn test_for_each_multi_handler_with_context_call_order() {
    use std::cell::RefCell;

    // Test call order with context-aware handlers
    let xml = r#"<root>
        <a id="1"/>
        <b id="2"/>
        <a id="3"/>
    </root>"#;

    let call_order = RefCell::new(Vec::new());

    StreamTransformer::new(xml)
        .on_with_context("//a", |node, ctx| {
            if let Some(id) = node.get_attribute("id") {
                call_order
                    .borrow_mut()
                    .push(format!("a:{} pos:{}", id, ctx.position()));
            }
        })
        .on("//b", |node| {
            if let Some(id) = node.get_attribute("id") {
                call_order.borrow_mut().push(format!("b:{}", id));
            }
        })
        .for_each()
        .unwrap();

    assert_eq!(*call_order.borrow(), vec!["a:1 pos:1", "b:2", "a:3 pos:2"]);
}

#[test]
fn test_run_multi_handler_with_context() {
    // Test multi-handler transform with context
    let xml = r#"<root><item>A</item><item>B</item></root>"#;

    let result = StreamTransformer::new(xml)
        .on_with_context("//item", |node, ctx| {
            node.set_attribute("pos", &ctx.position().to_string());
        })
        .run()
        .unwrap()
        .to_string()
        .unwrap();

    assert!(result.contains(r#"pos="1""#));
    assert!(result.contains(r#"pos="2""#));
}
