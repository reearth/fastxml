//! Generate conformance report.

use fastxml_conformance::get_test_data_path;
use fastxml_conformance::reporter::{ConformanceReport, SuiteReport};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let output_json = args.iter().any(|a| a == "--json");

    let mut report = ConformanceReport::new();

    // Check which test suites are available
    if let Some(path) = get_test_data_path("w3c-xml") {
        eprintln!("Found W3C XML tests at: {}", path.display());
        // In a real implementation, we would run the tests here
        // For now, just add a placeholder report
        let mut suite = SuiteReport::new();
        suite.record_skip(); // Placeholder
        report.add_suite("w3c-xml", suite);
    } else {
        eprintln!("W3C XML tests not available");
    }

    if let Some(path) = get_test_data_path("w3c-xsd") {
        eprintln!("Found W3C XSD tests at: {}", path.display());
        let mut suite = SuiteReport::new();
        suite.record_skip();
        report.add_suite("w3c-xsd", suite);
    } else {
        eprintln!("W3C XSD tests not available");
    }

    eprintln!();

    if output_json {
        match report.to_json() {
            Ok(json) => println!("{}", json),
            Err(e) => {
                eprintln!("Error generating JSON: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        report.print_summary();
    }
}
