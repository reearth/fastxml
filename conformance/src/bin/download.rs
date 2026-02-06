//! Download conformance test data.

use fastxml_conformance::downloader::{TEST_SUITES, download_all, list_suites};
use fastxml_conformance::get_conformance_data_dir;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--list" | "-l" => {
                list_suites();
                return;
            }
            "--help" | "-h" => {
                println!("Usage: download [options] [suite-name]");
                println!();
                println!("Options:");
                println!("  --list, -l    List available test suites");
                println!("  --help, -h    Show this help message");
                println!();
                println!("If no suite name is provided, all suites will be downloaded.");
                return;
            }
            suite_name => {
                // Download specific suite
                let dest_dir = get_conformance_data_dir();
                if let Some(suite) = TEST_SUITES.iter().find(|s| s.name == suite_name) {
                    if let Err(e) =
                        fastxml_conformance::downloader::download_test_suite(suite, &dest_dir)
                    {
                        eprintln!("Error downloading {}: {}", suite_name, e);
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("Unknown test suite: {}", suite_name);
                    eprintln!("Use --list to see available suites.");
                    std::process::exit(1);
                }
                return;
            }
        }
    }

    // Download all suites
    let dest_dir = get_conformance_data_dir();
    eprintln!("Downloading test data to: {}", dest_dir.display());

    if let Err(e) = download_all(&dest_dir) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    eprintln!("All test suites downloaded successfully.");
}
