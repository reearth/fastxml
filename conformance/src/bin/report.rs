//! Generate a real conformance report by running the suites through the runner
//! library and emitting per-suite/engine counts, pass rates, coverage, and the
//! full non-pass record list.

use fastxml_conformance::get_test_data_path;
use fastxml_conformance::reporter::{ConformanceReport, SuiteReport, print_suite_run};
use fastxml_conformance::runner::Engine;
use fastxml_conformance::runner::xml::{load_catalog, run_xml_suite};
use fastxml_conformance::runner::xsd::{load_suite, run_xsd_suite};
use std::env;

fn main() {
    let output_json = env::args().any(|a| a == "--json");
    let mut report = ConformanceReport::new();

    if let Some(path) = get_test_data_path("w3c-xml") {
        eprintln!("Running W3C XML suite from: {}", path.display());
        if let Some(catalog) = load_catalog(&path) {
            for engine in [Engine::Dom, Engine::Streaming] {
                let run = run_xml_suite(engine, &catalog);
                print_suite_run(&format!("W3C XML ({})", engine.as_str()), &run);
                report.add(SuiteReport::from_run("w3c-xml", engine.as_str(), &run));
            }
        }
    } else {
        eprintln!("W3C XML tests not available");
    }

    if let Some(path) = get_test_data_path("w3c-xsd") {
        eprintln!("Running W3C XSD suite from: {}", path.display());
        if let Some(suite) = load_suite(&path) {
            for engine in [Engine::Dom, Engine::Streaming] {
                let run = run_xsd_suite(engine, &suite);
                print_suite_run(&format!("W3C XSD ({})", engine.as_str()), &run);
                report.add(SuiteReport::from_run("w3c-xsd", engine.as_str(), &run));
            }
        }
    } else {
        eprintln!("W3C XSD tests not available");
    }

    if output_json {
        match report.to_json() {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Error generating JSON: {e}");
                std::process::exit(1);
            }
        }
    }
}
