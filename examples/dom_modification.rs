//! DOM modification example.
//!
//! Demonstrates modifying XML documents using the mutable DOM API.
//!
//! Run with: cargo run --example dom_modification

use fastxml::{evaluate, get_root_node, node_to_xml_string, parse};

fn main() -> fastxml::error::Result<()> {
    let xml = r#"<?xml version="1.0"?>
<catalog>
    <book id="1">
        <title>Original Title</title>
        <price>29.99</price>
    </book>
    <book id="2">
        <title>Another Book</title>
        <price>19.99</price>
    </book>
</catalog>
"#;

    let doc = parse(xml.as_bytes())?;
    println!("=== DOM Modification Example ===\n");

    // Get mutable root
    let mut root = get_root_node(&doc)?;

    println!("Original document:");
    println!("{}\n", node_to_xml_string(&doc, &mut root)?);

    // 1. Modify attribute
    println!("1. Adding attribute to root...");
    root.set_attribute("version", "1.0");

    // 2. Modify element content
    println!("2. Modifying book title...");
    let result = evaluate(&doc, "//book[@id='1']/title")?;
    for node in result.into_nodes() {
        node.set_content("Modified Title");
    }

    // 3. Add new attribute to elements
    println!("3. Adding 'currency' attribute to prices...");
    let result = evaluate(&doc, "//price")?;
    for node in result.into_nodes() {
        node.set_attribute("currency", "USD");
    }

    // 4. Modify multiple elements
    println!("4. Adding discount attribute to all books...");
    let result = evaluate(&doc, "//book")?;
    for node in result.into_nodes() {
        node.set_attribute("discount", "10%");
    }

    // Print modified document
    let mut root = get_root_node(&doc)?;
    println!("\nModified document:");
    println!("{}", node_to_xml_string(&doc, &mut root)?);

    // 5. For element removal, use StreamTransformer
    println!("\n5. For element removal, use StreamTransformer API:");
    println!("   See transform_example.rs for examples.");

    Ok(())
}
