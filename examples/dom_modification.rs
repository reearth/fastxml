//! DOM modification example.
//!
//! Demonstrates modifying XML documents using the mutable DOM API, querying
//! with `QueryExt` and serializing with `Printer`.
//!
//! Run with: cargo run --example dom_modification

use fastxml::{Parser, Printer, QueryExt};

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

    let doc = Parser::from(xml.as_bytes()).parse()?;
    println!("=== DOM Modification Example ===\n");

    let root = doc.get_root_element()?;

    println!("Original document:");
    println!("{}\n", Printer::from(&root).to_string()?);

    // 1. Modify an attribute.
    println!("1. Adding attribute to root...");
    root.set_attribute("version", "1.0");

    // 2. Modify element content.
    println!("2. Modifying book title...");
    for node in doc.query_nodes("//book[@id='1']/title")? {
        node.set_content("Modified Title");
    }

    // 3. Add an attribute to several elements.
    println!("3. Adding 'currency' attribute to prices...");
    for node in doc.query_nodes("//price")? {
        node.set_attribute("currency", "USD");
    }

    // 4. Modify multiple elements.
    println!("4. Adding discount attribute to all books...");
    for node in doc.query_nodes("//book")? {
        node.set_attribute("discount", "10%");
    }

    // Print the modified document (pretty-printed).
    println!("\nModified document:");
    println!("{}", Printer::from(&doc).pretty().to_string()?);

    // 5. For element removal, use the Transformer API.
    println!("\n5. For element removal, use the Transformer API:");
    println!("   See transform_example.rs for examples.");

    Ok(())
}
