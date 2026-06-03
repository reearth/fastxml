//! libxml-compatible API (`fastxml::compat`).
//!
//! These free functions mirror libxml's shape to ease migration. New code should
//! prefer the modern front doors (`Parser`, `QueryExt`/`Query`, `Printer`); each
//! call below notes the modern equivalent.
//!
//! Run with: cargo run --example compat

use fastxml::Parser;
use fastxml::compat::{
    create_context, evaluate, find_readonly_nodes_by_xpath, get_node_tag, get_root_node,
    get_root_readonly_node, node_to_xml_string,
};

fn main() -> fastxml::error::Result<()> {
    let doc = Parser::from(r#"<root><item id="1">a</item><item id="2">b</item></root>"#).parse()?;

    println!("=== libxml-compatible API (fastxml::compat) ===\n");

    // get_root_node  (modern: doc.get_root_element())
    let mut root = get_root_node(&doc)?;
    // get_node_tag   (modern: node.qname())
    println!("root tag: {}", get_node_tag(&root));

    // evaluate       (modern: doc.query(..))
    let items = evaluate(&doc, "//item")?;
    println!("items found: {}", items.into_nodes().len());

    // create_context + find_readonly_nodes_by_xpath
    // (modern: XmlContext::new(&doc) + ctx.find_nodes(..) / Query)
    let ctx = create_context(&doc)?;
    let ro_root = get_root_readonly_node(&doc)?;
    let found = find_readonly_nodes_by_xpath(&ctx, "//item", &ro_root)?;
    println!("via context: {} items", found.len());

    // node_to_xml_string  (modern: Printer::from(node).to_string())
    println!("serialized root:\n{}", node_to_xml_string(&doc, &mut root)?);

    Ok(())
}
