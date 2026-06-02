//! Streaming parser example.
//!
//! Demonstrates processing XML with minimal memory using event-based parsing.
//!
//! Run with: cargo run --example streaming_parser

use fastxml::Parser;
use fastxml::error::Result;
use fastxml::event::XmlEvent;

fn main() -> Result<()> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- This is a sample document -->
<library>
    <book id="1" category="fiction">
        <title>The Great Gatsby</title>
        <author>F. Scott Fitzgerald</author>
        <year>1925</year>
    </book>
    <book id="2" category="science">
        <title>A Brief History of Time</title>
        <author>Stephen Hawking</author>
        <year>1988</year>
    </book>
    <metadata><![CDATA[Some <raw> data]]></metadata>
</library>
"#;

    println!("=== Streaming Parse Events ===\n");

    // The callback streams every event with constant memory and can mutate
    // local state directly — no shared counters or custom handler types needed.
    let mut element_count = 0usize;
    let mut depth = 0usize;

    Parser::from(xml).for_each_event(|event| {
        match event {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                element_count += 1;
                let indent = "  ".repeat(depth);
                depth += 1;
                if attributes.is_empty() {
                    println!("{indent}Start: {name}");
                } else {
                    let attr_names: Vec<_> = attributes.iter().map(|(k, _)| k.as_str()).collect();
                    println!("{indent}Start: {name} (attrs: {attr_names:?})");
                }
            }
            XmlEvent::EndElement { name, .. } => {
                depth = depth.saturating_sub(1);
                println!("{}End: {}", "  ".repeat(depth), name);
            }
            XmlEvent::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    println!("{}Text: {:?}", "  ".repeat(depth), trimmed);
                }
            }
            XmlEvent::CData(data) => {
                println!("{}CDATA: {:?}", "  ".repeat(depth), data);
            }
            XmlEvent::Comment(comment) => {
                println!("{}Comment: {:?}", "  ".repeat(depth), comment);
            }
            XmlEvent::ProcessingInstruction { target, content } => {
                println!("{}PI: {} {:?}", "  ".repeat(depth), target, content);
            }
            _ => {}
        }
        Ok(())
    })?;

    println!("\n=== Statistics ===");
    println!("Total elements: {element_count}");

    Ok(())
}
