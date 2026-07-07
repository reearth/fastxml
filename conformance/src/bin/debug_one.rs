//! Debug helper: validate one instance against one schema and print errors.
//!
//! Usage: cargo run -p fastxml-conformance --bin debug_one -- <schema.xsd> <instance.xml> [streaming|dom|both]

use std::fs;
use std::io::Cursor;
use std::sync::Arc;

fn report(label: &str, res: fastxml::Result<Vec<fastxml::StructuredError>>) {
    match res {
        Ok(errors) if errors.is_empty() => println!("[{label}] VALID (no errors)"),
        Ok(errors) => {
            println!("[{label}] INVALID ({} errors):", errors.len());
            for e in errors.iter().take(10) {
                println!("  {e}");
            }
        }
        Err(e) => println!("[{label}] validator error: {e}"),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let schema_path = args.next().expect("schema path");
    let instance_path = args.next().expect("instance path");
    let mode = args.next().unwrap_or_else(|| "both".to_string());

    let schema_content = fs::read(&schema_path).expect("read schema");
    let schema = match fastxml::schema::Schema::from_xsd(&schema_content) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            println!("schema compile error: {e}");
            return;
        }
    };

    let instance_content = fs::read(&instance_path).expect("read instance");

    if mode == "dom" || mode == "both" {
        let doc = fastxml::Parser::from(instance_content.as_slice())
            .parse()
            .expect("parse instance");
        report(
            "dom",
            fastxml::schema::Validator::from(&doc)
                .schema(Arc::clone(&schema))
                .run()
                .map(|r| r.into_entries()),
        );
    }
    if mode == "streaming" || mode == "both" {
        report(
            "streaming",
            fastxml::schema::Validator::from_reader(Cursor::new(instance_content.as_slice()))
                .schema(Arc::clone(&schema))
                .run()
                .map(|r| r.into_entries()),
        );
    }
}
