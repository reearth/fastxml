//! Analysis tool for W3C XSD conformance failures.
//!
//! Dumps per-instance validation failures with test set, group, and error
//! details so failure clusters can be identified.
//!
//! Usage: cargo run -p fastxml-conformance --release --bin analyze_xsd

use fastxml_conformance::catalog::xsdtests::{InstanceValidity, SchemaValidity, XsdTestSuite};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn find_suite_xml(data_path: &Path) -> Option<PathBuf> {
    let direct = data_path.join("suite.xml");
    if direct.exists() {
        return Some(direct);
    }
    for entry in data_path.read_dir().ok()?.flatten() {
        if entry.path().is_dir() {
            let candidate = entry.path().join("suite.xml");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn main() {
    let data_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/w3c-xsd");
    let suite_path = find_suite_xml(&data_path).expect("suite.xml not found");
    let suite = XsdTestSuite::parse(&suite_path).expect("parse suite");

    // (test_set, kind) -> (pass, fail)
    let mut by_set: BTreeMap<(String, &'static str), (usize, usize)> = BTreeMap::new();
    // detailed failure lines
    let mut failures: Vec<String> = Vec::new();

    for test_set in &suite.test_sets {
        for group in &test_set.groups {
            // Compile all valid-expected schema documents of the group
            // together so cross-document declarations resolve.
            let mut builder = fastxml::schema::Schema::builder();
            let mut have_docs = false;
            for schema_doc in &group.schemas {
                if schema_doc.expected != SchemaValidity::Valid || !schema_doc.path.exists() {
                    continue;
                }
                if let Ok(content) = fs::read(&schema_doc.path) {
                    builder = builder.add(format!("file://{}", schema_doc.path.display()), content);
                    have_docs = true;
                }
            }
            let mut compiled_schema = if have_docs {
                builder
                    .resolve_with(&fastxml::schema::FileFetcher::new())
                    .ok()
                    .map(Arc::new)
            } else {
                None
            };
            // Fall back to individually compiled documents when the combined
            // resolve fails (e.g. an import points outside the test data).
            if compiled_schema.is_none() {
                for schema_doc in &group.schemas {
                    if schema_doc.expected != SchemaValidity::Valid || !schema_doc.path.exists() {
                        continue;
                    }
                    if let Ok(content) = fs::read(&schema_doc.path)
                        && let Ok(schema) = fastxml::schema::Schema::from_xsd(&content)
                    {
                        compiled_schema = Some(Arc::new(schema));
                    }
                }
            }

            let Some(schema) = compiled_schema else {
                continue;
            };

            for instance in &group.instances {
                if !instance.path.exists() {
                    continue;
                }
                let content = match fs::read(&instance.path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let streaming = std::env::args().any(|a| a == "--streaming");
                let result = if streaming {
                    fastxml::schema::Validator::from(content.as_slice())
                        .schema(Arc::clone(&schema))
                        .run()
                        .map(|r| r.into_entries())
                } else {
                    let doc = match fastxml::Parser::from(content.as_slice()).parse() {
                        Ok(d) => d,
                        Err(_) => continue,
                    };
                    fastxml::schema::Validator::from(&doc)
                        .schema(Arc::clone(&schema))
                        .run()
                        .map(|r| r.into_entries())
                };

                let (actual_valid, first_error) = match &result {
                    Ok(errors) => (errors.is_empty(), errors.first().map(|e| e.to_string())),
                    Err(e) => (false, Some(format!("validator error: {e}"))),
                };

                let kind = match instance.expected {
                    InstanceValidity::Valid => "valid",
                    InstanceValidity::Invalid => "invalid",
                    _ => continue,
                };

                let pass = actual_valid == (instance.expected == InstanceValidity::Valid);
                let entry = by_set
                    .entry((test_set.name.clone(), kind))
                    .or_insert((0, 0));
                if pass {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                    let detail = if actual_valid {
                        "expected INVALID but validated clean".to_string()
                    } else {
                        format!(
                            "expected VALID but got error: {}",
                            first_error.unwrap_or_default()
                        )
                    };
                    failures.push(format!(
                        "FAILDETAIL\t{}\t{}\t{}\t{}\t{}",
                        test_set.name,
                        group.name,
                        instance.name,
                        instance.path.display(),
                        detail
                    ));
                }
            }
        }
    }

    let (total_pass, total_fail) = by_set
        .values()
        .fold((0usize, 0usize), |(p, f), (pass, fail)| {
            (p + pass, f + fail)
        });
    println!(
        "TOTAL\tpass={}\tfail={}\trate={:.2}%",
        total_pass,
        total_fail,
        100.0 * total_pass as f64 / (total_pass + total_fail) as f64
    );
    println!();
    println!("=== Per test-set results (failures > 0, sorted by fail count) ===");
    let mut rows: Vec<_> = by_set.iter().collect();
    rows.sort_by_key(|(_, (_, fail))| std::cmp::Reverse(*fail));
    for ((set, kind), (pass, fail)) in rows {
        if *fail > 0 {
            println!(
                "{set}\t[{kind}]\tpass={pass}\tfail={fail}\trate={:.1}%",
                100.0 * *pass as f64 / (*pass + *fail) as f64
            );
        }
    }

    println!();
    for line in &failures {
        println!("{line}");
    }
}
