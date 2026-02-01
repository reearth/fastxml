//! Example demonstrating the streaming XML transform API.
//!
//! Usage: cargo run --example transform_example

use fastxml::transform::{StreamTransformer, stream_transform};

fn main() {
    println!("=== Streaming XML Transform Example ===\n");

    // Example 1: Builder pattern with attribute modification
    println!("1. Builder pattern - modify specific element:");
    let xml = r#"<root><item id="1">A</item><item id="2">B</item><item id="3">C</item></root>"#;

    let result = StreamTransformer::new(xml)
        .xpath("//item[@id='2']")
        .transform(|node| {
            node.set_attribute("modified", "true");
            node.set_attribute("status", "processed");
        })
        .to_string()
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Output: {}\n", result);

    // Example 2: Function API
    println!("2. Function API - add attribute to all items:");
    let xml = "<root><item>1</item><item>2</item><item>3</item></root>";
    let mut output = Vec::new();

    let count = stream_transform(
        xml,
        "//item",
        |node| {
            node.set_attribute("processed", "true");
        },
        &mut output,
    )
    .unwrap();

    println!("  Input:  {}", xml);
    println!("  Output: {}", String::from_utf8(output).unwrap());
    println!("  Transformed {} nodes\n", count);

    // Example 3: Remove elements
    println!("3. Remove elements:");
    let xml = r#"<root><keep>A</keep><remove>B</remove><keep>C</keep><remove>D</remove></root>"#;

    let result = StreamTransformer::new(xml)
        .xpath("//remove")
        .transform(|node| {
            node.remove();
        })
        .to_string()
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Output: {}\n", result);

    // Example 4: Using last() - falls back to two-pass
    println!("4. Using last() (two-pass fallback):");
    let xml = "<root><item>A</item><item>B</item><item>C</item></root>";

    let result = StreamTransformer::new(xml)
        .xpath("//item[last()]")
        .transform(|node| {
            node.set_attribute("last", "true");
        })
        .to_string()
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Output: {}\n", result);

    // Example 5: Preserve XML structure
    println!("5. Preserve XML structure (declaration, whitespace):");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <item id="1">First</item>
  <item id="2">Second</item>
</root>"#;

    let result = StreamTransformer::new(xml)
        .xpath("//item[@id='1']")
        .transform(|node| {
            node.set_attribute("selected", "true");
        })
        .to_string()
        .unwrap();

    println!("  Input:");
    for line in xml.lines() {
        println!("    {}", line);
    }
    println!("  Output:");
    for line in result.lines() {
        println!("    {}", line);
    }

    // Example 6: Extract data with collect
    println!("\n6. Extract data with collect:");
    let xml = r#"<root><item id="1">Apple</item><item id="2">Banana</item><item id="3">Cherry</item></root>"#;

    let contents: Vec<String> = StreamTransformer::new(xml)
        .xpath("//item")
        .collect(|node| node.get_content().unwrap_or_default())
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Extracted contents: {:?}\n", contents);

    // Example 7: Extract attributes with collect
    println!("7. Extract attributes with collect:");
    let ids: Vec<String> = StreamTransformer::new(xml)
        .xpath("//item")
        .collect(|node| node.get_attribute("id").unwrap_or_default())
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Extracted IDs: {:?}\n", ids);

    // Example 8: Iterate with for_each
    println!("8. Iterate with for_each:");
    let xml = r#"<products><product price="100">Widget</product><product price="200">Gadget</product><product price="150">Gizmo</product></products>"#;

    let mut total_price = 0;
    let count = StreamTransformer::new(xml)
        .xpath("//product")
        .for_each(|node| {
            if let Some(price) = node
                .get_attribute("price")
                .and_then(|s| s.parse::<i32>().ok())
            {
                total_price += price;
            }
        })
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Found {} products, total price: {}\n", count, total_price);

    // Example 9: for_each with last() (fallback)
    println!("9. for_each with last() (uses two-pass fallback):");
    let xml = "<scores><score>85</score><score>92</score><score>78</score></scores>";

    let mut last_score = String::new();
    StreamTransformer::new(xml)
        .xpath("//score[last()]")
        .for_each(|node| {
            last_score = node.get_content().unwrap_or_default();
        })
        .unwrap();

    println!("  Input:  {}", xml);
    println!("  Last score: {}", last_score);

    println!("\n=== Done ===");
}
