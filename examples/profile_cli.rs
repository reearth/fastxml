//! CLI tool for profiling XML files.
//!
//! Usage: cargo run --features profile --example profile_cli -- <file.xml>

use std::env;
use std::path::Path;

use fastxml::profile;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <xml-file> [xpath-expression]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} sample.xml", args[0]);
        eprintln!("  {} sample.xml \"//*[name()='Building']\"", args[0]);
        std::process::exit(1);
    }

    let file_path = Path::new(&args[1]);

    if !file_path.exists() {
        eprintln!("Error: File not found: {}", file_path.display());
        std::process::exit(1);
    }

    println!("Profiling: {}", file_path.display());
    println!();

    match profile::profile_file(file_path) {
        Ok(result) => {
            println!("{}", result);

            println!("Detailed Metrics:");
            println!("  Element nodes:     {}", result.metrics.element_count);
            println!("  Text nodes:        {}", result.metrics.text_count);
            println!("  Attributes:        {}", result.metrics.attribute_count);
            println!("  Max depth:         {}", result.metrics.max_depth);
            println!("  Distinct elements: {}", result.metrics.distinct_elements);
            println!("  Namespace decls:   {}", result.metrics.namespace_count);

            // Run additional XPath if provided
            if args.len() > 2 {
                let xpath_expr = &args[2];
                println!();
                println!("Running XPath: {}", xpath_expr);

                let content = std::fs::read(file_path).unwrap();
                let doc = fastxml::Parser::from(content.as_slice()).parse().unwrap();

                let start = std::time::Instant::now();
                match fastxml::xpath::evaluate(&doc, xpath_expr) {
                    Ok(result) => {
                        let elapsed = start.elapsed();
                        let nodes = result.into_nodes();
                        println!("  Found {} nodes in {:?}", nodes.len(), elapsed);

                        if !nodes.is_empty() {
                            println!("  First 5 results:");
                            for (i, node) in nodes.iter().take(5).enumerate() {
                                println!("    {}. {}", i + 1, node.qname());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  XPath error: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error profiling file: {}", e);
            std::process::exit(1);
        }
    }
}
