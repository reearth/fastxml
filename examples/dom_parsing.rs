//! Basic DOM parsing example.
//!
//! Demonstrates parsing XML into a DOM tree and querying with XPath.
//!
//! Run with: cargo run --example dom_parsing

use fastxml::{evaluate, get_root_node, parse};

fn main() -> fastxml::error::Result<()> {
    let xml = r#"
<root>
    <item id="1">Hello</item>
    <item id="2">World</item>
    <item id="3">!</item>
</root>
"#;

    // Parse XML into DOM
    let doc = parse(xml.as_bytes())?;
    println!("Node count: {}", doc.node_count());

    // Get root element
    let root = get_root_node(&doc)?;
    println!("Root element: {}", root.get_name());

    // XPath query - find all items
    let result = evaluate(&doc, "//item")?;
    println!("\nFound {} items:", result.clone().into_nodes().len());

    for node in result.into_nodes() {
        let id = node.get_attribute("id").unwrap_or_default();
        let text = node.get_content().unwrap_or_default();
        println!("  item[id={}]: {}", id, text);
    }

    // XPath with predicate
    let result = evaluate(&doc, "//item[@id='2']")?;
    println!("\nItem with id=2:");
    for node in result.into_nodes() {
        println!("  {}", node.get_content().unwrap_or_default());
    }

    // Get text content
    let result = evaluate(&doc, "//item/text()")?;
    let texts: Vec<String> = fastxml::xpath::collect_text_values(&result);
    println!("\nAll text content: {:?}", texts);

    Ok(())
}
