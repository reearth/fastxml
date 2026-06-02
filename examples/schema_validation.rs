//! Schema validation example.
//!
//! Demonstrates validating XML documents against XSD schemas.
//!
//! Run with: cargo run --example schema_validation
//! Run with schema fetching: cargo run --example schema_validation --features ureq

use fastxml::Parser;
use fastxml::error::{ErrorLevel, StructuredError, ValidationErrorType};
use fastxml::schema::{Schema, Validator};

fn main() -> fastxml::error::Result<()> {
    // Example 1: Validate with built-in types
    println!("=== Example 1: Built-in Type Validation ===\n");

    let xml = r#"<?xml version="1.0"?>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <name>Test Document</name>
    <count>42</count>
    <active>true</active>
</root>
"#;

    let doc = Parser::from(xml).parse()?;
    println!("Parsed document with {} nodes", doc.node_count());

    // Validate against the built-in schema via the `Validator` front door.
    let report = Validator::from(&doc).schema(Schema::builtin()).run()?;
    if report.is_valid() {
        println!("Document is valid!\n");
    } else {
        println!("Validation errors:");
        for error in report.errors() {
            println!("  - {}", error);
        }
        println!();
    }

    // Example 2: Validate using xsi:schemaLocation (auto-fetch schemas)
    #[cfg(feature = "ureq")]
    {
        println!("=== Example 2: Validate with xsi:schemaLocation ===\n");
        demonstrate_schema_location_validation()?;
    }

    // Example 3: Error handling with detailed information
    println!("=== Example 3: Error Handling ===\n");

    // Demonstrate how to handle validation errors
    demonstrate_error_handling();

    Ok(())
}

fn demonstrate_error_handling() {
    // Simulated validation errors for demonstration
    let errors = vec![
        StructuredError::new(
            "Attribute 'optional' is not declared",
            ValidationErrorType::UnknownAttribute,
        )
        .with_line(5)
        .with_column(10)
        .with_level(ErrorLevel::Warning)
        .with_element_path("/root/item[1]")
        .with_node_name("optional"),
        StructuredError::new(
            "Element 'unknown' is not expected",
            ValidationErrorType::UnknownElement,
        )
        .with_line(8)
        .with_column(5)
        .with_level(ErrorLevel::Error)
        .with_element_path("/root/unknown")
        .with_node_name("unknown")
        .with_expected("name, count, or active")
        .with_found("unknown"),
    ];

    for error in &errors {
        // Format based on severity
        let prefix = match error.level {
            ErrorLevel::Warning => "[WARN]",
            ErrorLevel::Error => "[ERROR]",
            ErrorLevel::Fatal => "[FATAL]",
        };

        print!("{} ", prefix);

        // Location information
        if let Some(path) = error.element_path() {
            print!("{}", path);
        }
        if let Some(line) = error.line() {
            print!(" (line {})", line);
        }
        print!(": ");

        // Error message
        println!("{}", error.message);

        // Expected/found values if available
        if let (Some(expected), Some(found)) = (&error.expected, &error.found) {
            println!("         expected: {}, found: {}", expected, found);
        }
    }

    // Filter by severity
    let warnings: Vec<_> = errors
        .iter()
        .filter(|e| e.level == ErrorLevel::Warning)
        .collect();
    let errors_only: Vec<_> = errors
        .iter()
        .filter(|e| e.level == ErrorLevel::Error)
        .collect();

    println!(
        "\nSummary: {} warnings, {} errors",
        warnings.len(),
        errors_only.len()
    );
}

/// Demonstrates validation using xsi:schemaLocation attribute.
///
/// This function shows how to validate XML documents by automatically
/// fetching schemas referenced in the xsi:schemaLocation attribute.
#[cfg(feature = "ureq")]
fn demonstrate_schema_location_validation() -> fastxml::error::Result<()> {
    // Example XML with schemaLocation pointing to a real schema
    // In practice, this would reference actual schema URLs
    let xml = r#"<?xml version="1.0"?>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="http://www.w3.org/2001/XMLSchema http://www.w3.org/2001/XMLSchema.xsd">
    <element>content</element>
</root>
"#;

    let doc = Parser::from(xml).parse()?;
    println!("Parsed document with {} nodes", doc.node_count());
    println!("Validating with schemas from xsi:schemaLocation...\n");

    // Without .schema(..), the schema is resolved from the document's
    // xsi:schemaLocation and fetched with the default fetcher.
    let report = Validator::from(&doc).run()?;

    if report.is_clean() {
        println!("Document is valid!\n");
    } else {
        println!("Validation results:");
        for error in report.entries() {
            let prefix = match error.level {
                ErrorLevel::Warning => "[WARN]",
                ErrorLevel::Error => "[ERROR]",
                ErrorLevel::Fatal => "[FATAL]",
            };
            println!("  {} {}", prefix, error.message);
        }
        println!();
    }

    Ok(())
}
