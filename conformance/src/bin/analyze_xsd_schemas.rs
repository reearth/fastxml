//! Analysis tool for expected-invalid XSD schemas that fastxml wrongly compiles.
//!
//! Mirrors the conformance suite's schema-test path exactly (`Schema::from_xsd`
//! per schema document, like `runner::xsd::eval_schema`) so its counts match
//! the `schema-invalid` category of the suite. Lists every expected-invalid
//! schema document that compiles cleanly, with the test group's documentation,
//! plus a summary grouped by test-group family prefix.
//!
//! It also compiles every expected-valid schema document and lists those that
//! are rejected, so a stricter check can be tried without the full suite.
//!
//! Usage: cargo run -p fastxml-conformance --release --bin analyze_xsd_schemas

use fastxml_conformance::catalog::xsdtests::{SchemaValidity, XsdTestSuite};
use fastxml_conformance::outcome::{Outcome, classify_invalid_schema_rejection};
use fastxml_conformance::runner::xsd::find_suite_xml;
use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

/// The "family" of a test group: its leading alphabetic prefix
/// (e.g. `particlesIc004` -> `particlesIc`, `mgO001` -> `mgO`).
fn family(group_name: &str) -> String {
    group_name
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect()
}

fn main() {
    let data_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/w3c-xsd");
    let suite_path = find_suite_xml(&data_path).expect("suite.xml not found");
    let suite = XsdTestSuite::parse(&suite_path).expect("parse suite");

    let mut pass = 0usize;
    let mut blocked = 0usize;
    let mut other_fail = 0usize; // rejected for the wrong reason (parse-level)
    let mut compiled: Vec<String> = Vec::new(); // wrongly compiled detail lines
    // family -> (compiled, total-invalid)
    let mut by_family: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut valid_total = 0usize;
    let mut rejected: Vec<String> = Vec::new(); // expected-valid but rejected

    for test_set in &suite.test_sets {
        for group in &test_set.groups {
            for schema_doc in &group.schemas {
                if schema_doc.expected == SchemaValidity::Valid {
                    let Ok(content) = fs::read(&schema_doc.path) else {
                        continue;
                    };
                    valid_total += 1;
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        fastxml::schema::Schema::from_xsd(&content)
                    }));
                    let error = match result {
                        Ok(Ok(_)) => continue,
                        Ok(Err(e)) => e.to_string(),
                        Err(_) => "panic".to_string(),
                    };
                    rejected.push(format!(
                        "REJECTED\t{}\t{}\t{}\t{}",
                        test_set.name,
                        group.name,
                        schema_doc.path.display(),
                        error
                    ));
                    continue;
                }
                if schema_doc.expected != SchemaValidity::Invalid {
                    continue;
                }
                let Ok(content) = fs::read(&schema_doc.path) else {
                    blocked += 1;
                    continue;
                };
                let fam = family(&group.name);
                let entry = by_family.entry(fam).or_insert((0, 0));
                entry.1 += 1;

                let result = catch_unwind(AssertUnwindSafe(|| {
                    fastxml::schema::Schema::from_xsd(&content)
                }));
                match result {
                    Ok(Ok(_)) => {
                        entry.0 += 1;
                        compiled.push(format!(
                            "COMPILED\t{}\t{}\t{}\t{}",
                            test_set.name,
                            group.name,
                            schema_doc.path.display(),
                            group.description.as_deref().unwrap_or("-")
                        ));
                    }
                    Ok(Err(e)) => match classify_invalid_schema_rejection(&e) {
                        Outcome::Pass => pass += 1,
                        Outcome::Blocked => blocked += 1,
                        _ => other_fail += 1,
                    },
                    Err(_) => other_fail += 1,
                }
            }
        }
    }

    let wrongly_compiled = compiled.len();
    let fail = wrongly_compiled + other_fail;
    println!(
        "TOTAL\tpass={}\tfail={}\t(compiled={} wrong-reason={})\tblocked={}\trate={:.1}%",
        pass,
        fail,
        wrongly_compiled,
        other_fail,
        blocked,
        100.0 * pass as f64 / (pass + fail) as f64
    );
    println!("VALID\trejected={}\tof={}", rejected.len(), valid_total);
    for line in &rejected {
        println!("{line}");
    }
    println!();
    println!("=== Wrongly-compiled by family prefix (sorted by count) ===");
    let mut rows: Vec<_> = by_family
        .iter()
        .filter(|(_, (compiled, _))| *compiled > 0)
        .collect();
    rows.sort_by_key(|(_, (compiled, _))| std::cmp::Reverse(*compiled));
    for (fam, (compiled, total)) in rows {
        println!("{fam}\tcompiled={compiled}\tof={total}");
    }

    println!();
    for line in &compiled {
        println!("{line}");
    }
}
