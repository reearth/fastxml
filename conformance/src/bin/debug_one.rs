//! Debug helper: validate one instance against one schema and print errors.
//!
//! Usage: cargo run -p fastxml-conformance --bin debug_one -- <schema.xsd> <instance.xml>

use std::fs;
use std::sync::Arc;

fn main() {
    let mut args = std::env::args().skip(1);
    let schema_path = args.next().expect("schema path");
    let instance_path = args.next().expect("instance path");

    let schema_content = fs::read(&schema_path).expect("read schema");
    let schema = match fastxml::schema::Schema::from_xsd(&schema_content) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            println!("schema compile error: {e}");
            return;
        }
    };

    let instance_content = fs::read(&instance_path).expect("read instance");
    let doc = fastxml::Parser::from(instance_content.as_slice())
        .parse()
        .expect("parse instance");

    match fastxml::schema::Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .map(|r| r.into_entries())
    {
        Ok(errors) if errors.is_empty() => println!("VALID (no errors)"),
        Ok(errors) => {
            println!("INVALID ({} errors):", errors.len());
            for e in errors.iter().take(10) {
                println!("  {e}");
            }
        }
        Err(e) => println!("validator error: {e}"),
    }
}
