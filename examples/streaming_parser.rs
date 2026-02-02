//! Streaming parser example.
//!
//! Demonstrates processing XML with minimal memory using event-based parsing.
//!
//! Run with: cargo run --example streaming_parser

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Custom handler that prints events and collects statistics.
struct EventPrinter {
    element_count: Arc<AtomicUsize>,
    current_depth: usize,
}

impl EventPrinter {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self {
            element_count: counter,
            current_depth: 0,
        }
    }

    fn indent(&self) -> String {
        "  ".repeat(self.current_depth)
    }
}

impl XmlEventHandler for EventPrinter {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                self.element_count.fetch_add(1, Ordering::SeqCst);
                self.current_depth += 1;

                if !attributes.is_empty() {
                    let attr_names: Vec<_> = attributes.iter().map(|(k, _)| k.as_str()).collect();
                    println!("{}Start: {} (attrs: {:?})", self.indent(), name, attr_names);
                } else {
                    println!("{}Start: {}", self.indent(), name);
                }
            }
            XmlEvent::EndElement { name, .. } => {
                println!("{}End: {}", self.indent(), name);
                self.current_depth = self.current_depth.saturating_sub(1);
            }
            XmlEvent::Text(text) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    println!("{}Text: {:?}", self.indent(), trimmed);
                }
            }
            XmlEvent::CData(data) => {
                println!("{}CDATA: {:?}", self.indent(), data);
            }
            XmlEvent::Comment(comment) => {
                println!("{}Comment: {:?}", self.indent(), comment);
            }
            XmlEvent::ProcessingInstruction { target, content } => {
                println!("{}PI: {} {:?}", self.indent(), target, content);
            }
            _ => {}
        }
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

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

    // Use Arc<AtomicUsize> to share counter between main and handler
    let element_count = Arc::new(AtomicUsize::new(0));

    let reader = BufReader::new(xml.as_bytes());
    let mut parser = StreamingParser::new(reader);

    parser.add_handler(Box::new(EventPrinter::new(Arc::clone(&element_count))));

    parser.parse()?;

    println!("\n=== Statistics ===");
    println!("Total elements: {}", element_count.load(Ordering::SeqCst));

    Ok(())
}
