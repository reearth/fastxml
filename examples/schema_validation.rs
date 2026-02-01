//! Schema validation example.
//!
//! Demonstrates validating XML documents against XSD schemas.
//!
//! Run with: cargo run --example schema_validation

use fastxml::error::{ErrorLevel, StructuredError, ValidationErrorType};
use fastxml::schema::validator::XmlSchemaValidationContext;
use fastxml::schema::xsd::create_builtin_schema;
use fastxml::parse;

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

    let doc = parse(xml.as_bytes())?;
    println!("Parsed document with {} nodes", doc.node_count());

    // Create validation context with built-in schema
    let schema = create_builtin_schema();
    let ctx = XmlSchemaValidationContext::new(schema);

    let errors = ctx.validate(&doc)?;
    if errors.is_empty() {
        println!("Document is valid!\n");
    } else {
        println!("Validation errors:");
        for error in &errors {
            println!("  - {}", error);
        }
        println!();
    }

    // Example 2: Error handling with detailed information
    println!("=== Example 2: Error Handling ===\n");

    // Demonstrate how to handle validation errors
    demonstrate_error_handling();

    Ok(())
}

fn demonstrate_error_handling() {
    // Simulated validation errors for demonstration
    let errors = vec![
        StructuredError {
            message: "Attribute 'optional' is not declared".to_string(),
            line: Some(5),
            column: Some(10),
            error_type: ValidationErrorType::UnknownAttribute,
            level: ErrorLevel::Warning,
            element_path: Some("/root/item[1]".to_string()),
            node_name: Some("optional".to_string()),
            expected: None,
            found: None,
        },
        StructuredError {
            message: "Element 'unknown' is not expected".to_string(),
            line: Some(8),
            column: Some(5),
            error_type: ValidationErrorType::UnknownElement,
            level: ErrorLevel::Error,
            element_path: Some("/root/unknown".to_string()),
            node_name: Some("unknown".to_string()),
            expected: Some("name, count, or active".to_string()),
            found: Some("unknown".to_string()),
        },
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
        if let Some(path) = &error.element_path {
            print!("{}", path);
        }
        if let Some(line) = error.line {
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

    println!("\nSummary: {} warnings, {} errors", warnings.len(), errors_only.len());
}
