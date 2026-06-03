//! Basic DOM parsing example.
//!
//! Demonstrates parsing XML into a DOM tree and querying it with the modern
//! `Parser` / `QueryExt` / `Query` front doors.
//!
//! Run with: cargo run --example dom_parsing

use fastxml::{Parser, Query, QueryExt};

fn main() -> fastxml::error::Result<()> {
    let xml = r#"
<root>
    <item id="1">Hello</item>
    <item id="2">World</item>
    <item id="3">!</item>
</root>
"#;

    // Parse XML into a DOM.
    let doc = Parser::from(xml.as_bytes()).parse()?;
    println!("Node count: {}", doc.node_count());

    // Get the root element (a plain method on the document).
    let root = doc.get_root_element()?;
    println!("Root element: {}", root.get_name());

    // XPath query via the `QueryExt` method on the document.
    let items = doc.query_nodes("//item")?;
    println!("\nFound {} items:", items.len());
    for node in &items {
        let id = node.get_attribute("id").unwrap_or_default();
        let text = node.get_content().unwrap_or_default();
        println!("  item[id={}]: {}", id, text);
    }

    // XPath with a predicate.
    println!("\nItem with id=2:");
    for node in doc.query_nodes("//item[@id='2']")? {
        println!("  {}", node.get_content().unwrap_or_default());
    }

    // Compile a query once and reuse it (here just once, but it can run against
    // many documents without re-parsing the expression).
    let text_query = Query::compile("//item/text()")?;
    let texts = text_query.eval(&doc)?.into_nodes();
    let texts: Vec<String> = texts
        .iter()
        .map(|n| n.get_content().unwrap_or_default())
        .collect();
    println!("\nAll text content: {:?}", texts);

    Ok(())
}
