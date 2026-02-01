//! Streaming validation example.
//!
//! Demonstrates validating large XML files while parsing in a single pass.
//! This is memory-efficient as it doesn't build a full DOM tree.
//!
//! Run with: cargo run --example streaming_validation
//! Run with schema fetching: cargo run --example streaming_validation --features ureq

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::schema::validator::StreamingSchemaValidator;
use fastxml::schema::xsd::create_builtin_schema;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Custom handler that counts elements while validation happens
struct CountingHandler {
    element_count: Arc<AtomicUsize>,
}

impl CountingHandler {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self {
            element_count: counter,
        }
    }
}

impl XmlEventHandler for CountingHandler {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        if let XmlEvent::StartElement { .. } = event {
            self.element_count.fetch_add(1, Ordering::SeqCst);

            // Progress indicator for large files
            let count = self.element_count.load(Ordering::SeqCst);
            if count.is_multiple_of(1000) {
                println!("Processed {} elements...", count);
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    // Sample CityGML-like document
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<CityModel xmlns="http://www.opengis.net/citygml/2.0"
           xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
           xmlns:gml="http://www.opengis.net/gml">
    <cityObjectMember>
        <bldg:Building gml:id="BLDG_001">
            <bldg:measuredHeight uom="m">25.5</bldg:measuredHeight>
            <bldg:storeysAboveGround>8</bldg:storeysAboveGround>
            <bldg:yearOfConstruction>1995</bldg:yearOfConstruction>
        </bldg:Building>
    </cityObjectMember>
    <cityObjectMember>
        <bldg:Building gml:id="BLDG_002">
            <bldg:measuredHeight uom="m">32.0</bldg:measuredHeight>
            <bldg:storeysAboveGround>10</bldg:storeysAboveGround>
            <bldg:yearOfConstruction>2005</bldg:yearOfConstruction>
        </bldg:Building>
    </cityObjectMember>
    <cityObjectMember>
        <bldg:Building gml:id="BLDG_003">
            <bldg:measuredHeight uom="m">18.0</bldg:measuredHeight>
            <bldg:storeysAboveGround>5</bldg:storeysAboveGround>
            <bldg:yearOfConstruction>2010</bldg:yearOfConstruction>
        </bldg:Building>
    </cityObjectMember>
</CityModel>
"#;

    println!("=== Streaming Validation Example ===\n");

    // Create schema (in real usage, load from XSD file)
    let schema = Arc::new(create_builtin_schema());

    // Shared counter for element count
    let element_count = Arc::new(AtomicUsize::new(0));

    // Create streaming parser
    let reader = BufReader::new(xml.as_bytes());
    let mut parser = StreamingParser::new(reader);

    // Add counting handler
    parser.add_handler(Box::new(CountingHandler::new(Arc::clone(&element_count))));

    // Add streaming validator
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(validator));

    println!("Starting streaming parse with validation...\n");

    // Parse and validate in single pass
    parser.parse()?;

    println!("\n=== Results ===\n");
    println!(
        "Total elements processed: {}",
        element_count.load(Ordering::SeqCst)
    );
    println!("Validation: PASSED (using built-in schema)");

    println!("\nStreaming validation complete!");
    println!("Note: Memory usage stays constant regardless of file size.");

    // Example 2: Streaming validation with xsi:schemaLocation
    #[cfg(feature = "ureq")]
    {
        println!("\n=== Streaming Validation with xsi:schemaLocation ===\n");
        streaming_validation_with_schema_location()?;
    }

    Ok(())
}

/// Demonstrates single-pass streaming validation using schemas from xsi:schemaLocation.
///
/// Uses `LazySchemaValidator` which fetches the schema on the first StartElement event,
/// enabling true single-pass streaming validation.
#[cfg(feature = "ureq")]
fn streaming_validation_with_schema_location() -> Result<()> {
    use fastxml::schema::{LazySchemaValidator, UreqFetcher};

    // Sample XML with schemaLocation
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://www.w3.org/2001/XMLSchema http://www.w3.org/2001/XMLSchema.xsd">
    <element>content</element>
</root>
"#;

    println!("Running single-pass streaming validation with LazySchemaValidator...");
    println!("(Schema will be fetched on first StartElement)\n");

    let element_count = Arc::new(AtomicUsize::new(0));
    let reader = BufReader::new(xml.as_bytes());
    let mut parser = StreamingParser::new(reader);

    // Add counting handler
    parser.add_handler(Box::new(CountingHandler::new(Arc::clone(&element_count))));

    // LazySchemaValidator fetches schema lazily on first StartElement
    let validator = LazySchemaValidator::new(UreqFetcher::new());
    parser.add_handler(Box::new(validator));

    parser.parse()?;

    println!(
        "\nValidation complete! Processed {} elements.",
        element_count.load(Ordering::SeqCst)
    );

    Ok(())
}
