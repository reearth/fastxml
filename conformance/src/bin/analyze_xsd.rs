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
            let mut compiled_schema = None;

            for schema_doc in &group.schemas {
                if !schema_doc.path.exists() {
                    continue;
                }
                let content = match fs::read(&schema_doc.path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if let Ok(schema) = fastxml::schema::Schema::from_xsd(&content)
                    && schema_doc.expected == SchemaValidity::Valid
                {
                    compiled_schema = Some(Arc::new(schema));
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
                let doc = match fastxml::Parser::from(content.as_slice()).parse() {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                let result = fastxml::schema::Validator::from(&doc)
                    .schema(Arc::clone(&schema))
                    .run()
                    .map(|r| r.into_entries());

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
