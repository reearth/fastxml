//! Streaming validation example.
//!
//! Demonstrates validating large XML files while parsing in a single pass.
//! This is memory-efficient as it doesn't build a full DOM tree.
//!
//! ```ignore
//! use fastxml::schema::{Schema, Validator};
//!
//! let report = Validator::from_reader(reader)
//!     .schema(Schema::builtin())
//!     .max_errors(100)
//!     .run()?;
//! ```
//!
//! Run with: cargo run --example streaming_validation
//! Run with file: cargo run --example streaming_validation -- path/to/file.xml
//! Run with schema fetching: cargo run --example streaming_validation --features ureq -- path/to/file.xml

use fastxml::Parser;
use fastxml::error::Result;
use fastxml::event::XmlEvent;
use fastxml::schema::{Schema, Validator};

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
</CityModel>
"#;

    println!("=== Streaming Validation Example (Demo) ===\n");
    println!("Starting streaming parse with validation...\n");

    // Validate in a single streaming pass against the built-in schema.
    // (In real usage, load a schema with Schema::from_xsd / Schema::builder.)
    let report = Validator::from(xml).schema(Schema::builtin()).run()?;

    // Separately, stream the events to count elements with constant memory.
    let mut element_count = 0usize;
    Parser::from(xml).for_each_event(|event| {
        if matches!(event, XmlEvent::StartElement { .. }) {
            element_count += 1;
        }
        Ok(())
    })?;

    println!("\n=== Results ===\n");
    println!("Total elements processed: {element_count}");
    println!(
        "Validation: {} (using built-in schema)",
        if report.is_valid() {
            "PASSED"
        } else {
            "FAILED"
        }
    );

    println!("\nStreaming validation complete!");
    println!("Note: Memory usage stays constant regardless of file size.");

    Ok(())
}

/// Validate a file from command line argument
#[cfg(feature = "ureq")]
fn validate_file(file_path: &str) -> Result<()> {
    use fastxml::schema::DefaultFetcher;
    use std::fs::File;
    use std::io::BufReader;
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
    // so relative schema paths resolve.
    let base_dir = Path::new(file_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let fetcher = DefaultFetcher::with_base_dir(base_dir);

    // No .schema(..) => the schema is resolved from xsi:schemaLocation.
    let report = Validator::from_reader(reader).run_with(fetcher)?;
    let elapsed = start.elapsed();

    for err in report.entries() {
        if err.is_error() {
            println!("[ERROR] {}", err.message);
        } else {
            println!("[WARN] {}", err.message);
        }
    }

    println!("\n=== Results ===\n");
    println!("Time: {:.2?}", elapsed);
    println!(
        "Throughput: {:.2} MB/s",
        file_size as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64()
    );
    println!(
        "Errors: {}, Warnings: {}",
        report.error_count(),
        report.warning_count()
    );

    if report.is_valid() {
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
