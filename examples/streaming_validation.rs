//! Streaming validation example.
//!
//! Demonstrates validating large XML files while parsing in a single pass.
//! This is memory-efficient as it doesn't build a full DOM tree.
//!
//! Run with: cargo run --example streaming_validation
//! Run with file: cargo run --example streaming_validation -- path/to/file.xml
//! Run with schema fetching: cargo run --example streaming_validation --features ureq -- path/to/file.xml

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::schema::validator::OnePassSchemaValidator;
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

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // If file argument provided, validate that file
    if args.len() > 1 {
        let file_path = &args[1];
        return validate_file(file_path);
    }

    // Otherwise run the demo with embedded XML
    run_demo()
}

fn run_demo() -> Result<()> {
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

    println!("=== Streaming Validation Example (Demo) ===\n");

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
    let validator = OnePassSchemaValidator::new(Arc::clone(&schema));
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

    Ok(())
}

/// Validate a file from command line argument
#[cfg(feature = "ureq")]
fn validate_file(file_path: &str) -> Result<()> {
    use fastxml::schema::DefaultFetcher;
    use fastxml::schema::validator::streaming_validate_with_schema_location_and_fetcher;
    use std::fs::File;
    use std::path::Path;

    println!("=== Streaming Validation: {} ===\n", file_path);

    let file = File::open(file_path).map_err(fastxml::error::Error::Io)?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    println!("File size: {:.2} MB", file_size as f64 / 1024.0 / 1024.0);

    println!("Starting streaming parse with validation...");
    println!("(Schema will be fetched from xsi:schemaLocation if present)\n");

    let start = std::time::Instant::now();
    let reader = BufReader::new(file);

    // Use DefaultFetcher with base directory from the XML file's location
    // This allows resolving relative schema paths
    let base_dir = Path::new(file_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let fetcher = DefaultFetcher::with_base_dir(base_dir);

    let errors = streaming_validate_with_schema_location_and_fetcher(reader, fetcher)?;
    let elapsed = start.elapsed();

    let mut error_count = 0;
    let mut warning_count = 0;

    for err in &errors {
        if err.is_error() {
            error_count += 1;
            println!("[ERROR] {}", err.message);
        } else {
            warning_count += 1;
            println!("[WARN] {}", err.message);
        }
    }

    println!("\n=== Results ===\n");
    println!("Time: {:.2?}", elapsed);
    println!(
        "Throughput: {:.2} MB/s",
        file_size as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64()
    );
    println!("Errors: {}, Warnings: {}", error_count, warning_count);

    if error_count == 0 {
        println!("\nValidation: PASSED");
    } else {
        println!("\nValidation: FAILED");
    }

    Ok(())
}

#[cfg(not(feature = "ureq"))]
fn validate_file(file_path: &str) -> Result<()> {
    eprintln!("Error: File validation requires the 'ureq' feature for schema fetching.");
    eprintln!(
        "Run with: cargo run --example streaming_validation --features ureq -- {}",
        file_path
    );
    std::process::exit(1);
}
