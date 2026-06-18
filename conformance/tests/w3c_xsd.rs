//! W3C XML Schema Test Suite tests.
//!
//! These tests run the W3C XSD Test Suite against fastxml's schema validator.
//! Both DOM-based validation and streaming validation are tested.

use fastxml_conformance::catalog::xsdtests::{InstanceValidity, SchemaValidity, XsdTestSuite};
use fastxml_conformance::reporter::SuiteReport;
use fastxml_conformance::require_test_data;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

/// Run W3C XSD conformance tests with DOM validator.
#[test]
fn w3c_xsd_conformance_dom() {
    let data_path = require_test_data!("w3c-xsd");

    let suite_path = find_suite_xml(&data_path);
    let suite_path = match suite_path {
        Some(p) => p,
        None => {
            eprintln!(
                "suite.xml not found in {}. Skipping tests.",
                data_path.display()
            );
            return;
        }
    };

    eprintln!("Loading XSD test suite from: {}", suite_path.display());

    let suite = match XsdTestSuite::parse(&suite_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse suite: {}", e);
            return;
        }
    };

    let stats = suite.stats();
    eprintln!(
        "Found {} test groups with {} schemas and {} instance tests",
        stats.groups, stats.schemas, stats.instances
    );
    eprintln!(
        "  Valid instances: {}, Invalid instances: {}",
        stats.valid_instances, stats.invalid_instances
    );

    let mut report = SuiteReport::new();
    let mut schema_pass = 0;
    let mut schema_fail = 0;
    let mut schema_valid_pass = 0;
    let mut schema_valid_total = 0;
    let mut schema_invalid_pass = 0;
    let mut schema_invalid_total = 0;
    let mut instance_pass = 0;
    let mut instance_fail = 0;

    for group in suite.all_groups() {
        let mut compiled_schema = None;

        for schema_doc in &group.schemas {
            if !schema_doc.path.exists() {
                continue;
            }

            let content = match fs::read(&schema_doc.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match schema_doc.expected {
                SchemaValidity::Valid => schema_valid_total += 1,
                SchemaValidity::Invalid => schema_invalid_total += 1,
                _ => {}
            }

            match fastxml::schema::Schema::from_xsd(&content) {
                Ok(schema) => {
                    if schema_doc.expected == SchemaValidity::Valid {
                        schema_pass += 1;
                        schema_valid_pass += 1;
                        compiled_schema = Some(Arc::new(schema));
                    } else if schema_doc.expected == SchemaValidity::Invalid {
                        schema_fail += 1;
                        eprintln!(
                            "FAIL [schema/dom] {}: expected invalid but compiled",
                            group.name
                        );
                    }
                }
                Err(e) => {
                    if schema_doc.expected == SchemaValidity::Invalid {
                        schema_pass += 1;
                        schema_invalid_pass += 1;
                    } else if schema_doc.expected == SchemaValidity::Valid {
                        schema_fail += 1;
                        eprintln!("FAIL [schema/dom] {}: {}", group.name, e);
                    }
                }
            }
        }

        // Prefer compiling all valid-expected documents together so
        // cross-document declarations resolve (imports, strict wildcards).
        if let Some(combined) = compile_group_schemas(group) {
            compiled_schema = Some(combined);
        }

        if let Some(ref schema) = compiled_schema {
            for instance in &group.instances {
                if !instance.path.exists() {
                    report.record_skip();
                    continue;
                }

                let content = match fs::read(&instance.path) {
                    Ok(c) => c,
                    Err(_) => {
                        report.record_skip();
                        continue;
                    }
                };

                let doc = match fastxml::Parser::from(content.as_slice()).parse() {
                    Ok(d) => d,
                    Err(_) => {
                        report.record_skip();
                        continue;
                    }
                };

                match fastxml::schema::Validator::from(&doc)
                    .schema(Arc::clone(schema))
                    .run()
                    .map(|r| r.into_entries())
                {
                    Ok(errors) => {
                        let is_valid = errors.is_empty();
                        if is_valid && instance.expected == InstanceValidity::Valid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if !is_valid && instance.expected == InstanceValidity::Invalid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if is_valid && instance.expected == InstanceValidity::Invalid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else if !is_valid && instance.expected == InstanceValidity::Valid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else {
                            report.record_skip();
                        }
                    }
                    Err(_) => {
                        if instance.expected == InstanceValidity::Invalid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if instance.expected == InstanceValidity::Valid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else {
                            report.record_skip();
                        }
                    }
                }
            }
        } else {
            for _ in &group.instances {
                report.record_skip();
            }
        }
    }

    eprintln!();
    eprintln!("=== W3C XSD Conformance (DOM) Results ===");
    eprintln!(
        "Schema compilation: {} passed, {} failed",
        schema_pass, schema_fail
    );
    eprintln!(
        "  valid schemas accepted: {}/{} ({:.1}%)",
        schema_valid_pass,
        schema_valid_total,
        100.0 * schema_valid_pass as f64 / schema_valid_total.max(1) as f64
    );
    eprintln!(
        "  invalid schemas rejected: {}/{} ({:.1}%)",
        schema_invalid_pass,
        schema_invalid_total,
        100.0 * schema_invalid_pass as f64 / schema_invalid_total.max(1) as f64
    );
    eprintln!(
        "Instance validation: {} passed, {} failed",
        instance_pass, instance_fail
    );
    eprintln!("Total: {}", report.total);
    eprintln!("Skipped: {}", report.skipped);
}

/// Run W3C XSD conformance tests with streaming validator.
#[test]
fn w3c_xsd_conformance_streaming() {
    let data_path = require_test_data!("w3c-xsd");

    let suite_path = find_suite_xml(&data_path);
    let suite_path = match suite_path {
        Some(p) => p,
        None => {
            eprintln!("suite.xml not found. Skipping streaming tests.");
            return;
        }
    };

    let suite = match XsdTestSuite::parse(&suite_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to parse suite: {}", e);
            return;
        }
    };

    let mut report = SuiteReport::new();
    let mut schema_pass = 0;
    let mut schema_fail = 0;
    let mut instance_pass = 0;
    let mut instance_fail = 0;

    for group in suite.all_groups() {
        let mut compiled_schema = None;

        for schema_doc in &group.schemas {
            if !schema_doc.path.exists() {
                continue;
            }

            let content = match fs::read(&schema_doc.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match fastxml::schema::Schema::from_xsd(&content) {
                Ok(schema) => {
                    if schema_doc.expected == SchemaValidity::Valid {
                        schema_pass += 1;
                        compiled_schema = Some(Arc::new(schema));
                    } else if schema_doc.expected == SchemaValidity::Invalid {
                        schema_fail += 1;
                    }
                }
                Err(_) => {
                    if schema_doc.expected == SchemaValidity::Invalid {
                        schema_pass += 1;
                    } else if schema_doc.expected == SchemaValidity::Valid {
                        schema_fail += 1;
                    }
                }
            }
        }

        // Prefer compiling all valid-expected documents together so
        // cross-document declarations resolve (imports, strict wildcards).
        if let Some(combined) = compile_group_schemas(group) {
            compiled_schema = Some(combined);
        }

        if let Some(ref schema) = compiled_schema {
            for instance in &group.instances {
                if !instance.path.exists() {
                    report.record_skip();
                    continue;
                }

                let file = match fs::File::open(&instance.path) {
                    Ok(f) => f,
                    Err(_) => {
                        report.record_skip();
                        continue;
                    }
                };

                let reader = BufReader::new(file);

                match fastxml::schema::Validator::from_reader(reader)
                    .schema(Arc::clone(schema))
                    .run()
                    .map(|r| r.into_entries())
                {
                    Ok(errors) => {
                        let is_valid = errors.is_empty();
                        if is_valid && instance.expected == InstanceValidity::Valid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if !is_valid && instance.expected == InstanceValidity::Invalid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if is_valid && instance.expected == InstanceValidity::Invalid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else if !is_valid && instance.expected == InstanceValidity::Valid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else {
                            report.record_skip();
                        }
                    }
                    Err(_) => {
                        if instance.expected == InstanceValidity::Invalid {
                            instance_pass += 1;
                            report.record_pass(Some("instance"));
                        } else if instance.expected == InstanceValidity::Valid {
                            instance_fail += 1;
                            report.record_fail(&instance.name, Some("instance"));
                        } else {
                            report.record_skip();
                        }
                    }
                }
            }
        } else {
            for _ in &group.instances {
                report.record_skip();
            }
        }
    }

    eprintln!();
    eprintln!("=== W3C XSD Conformance (Streaming) Results ===");
    eprintln!(
        "Schema compilation: {} passed, {} failed",
        schema_pass, schema_fail
    );
    eprintln!(
        "Instance validation: {} passed, {} failed",
        instance_pass, instance_fail
    );
    eprintln!("Total: {}", report.total);
    eprintln!("Skipped: {}", report.skipped);
}

/// Compiles all valid-expected schema documents of a group together,
/// resolving file imports, so cross-document declarations are visible.
fn compile_group_schemas(
    group: &fastxml_conformance::catalog::xsdtests::SchemaTestGroup,
) -> Option<Arc<fastxml::schema::Schema>> {
    let mut builder = fastxml::schema::Schema::builder();
    let mut have_docs = false;
    for schema_doc in &group.schemas {
        if schema_doc.expected != SchemaValidity::Valid || !schema_doc.path.exists() {
            continue;
        }
        if let Ok(content) = fs::read(&schema_doc.path) {
            builder = builder.add(schema_doc.path.to_string_lossy(), content);
            have_docs = true;
        }
    }
    if !have_docs {
        return None;
    }
    builder
        .resolve_with(&fastxml::schema::FileFetcher::new())
        .ok()
        .map(Arc::new)
}

/// Find suite.xml in the test data directory.
fn find_suite_xml(data_path: &Path) -> Option<std::path::PathBuf> {
    let direct = data_path.join("suite.xml");
    if direct.exists() {
        return Some(direct);
    }

    let xsdtests = data_path.join("xsdtests");
    if xsdtests.exists() {
        let in_xsdtests = xsdtests.join("suite.xml");
        if in_xsdtests.exists() {
            return Some(in_xsdtests);
        }
    }

    for entry in data_path.read_dir().ok()? {
        if let Ok(entry) = entry {
            if entry.path().is_dir() {
                let candidate = entry.path().join("suite.xml");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

/// Test basic XSD validation functionality with both DOM and streaming validators.
#[test]
fn basic_xsd_validation() {
    let schema_str = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="child" type="xs:string"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let valid_xml = br#"<?xml version="1.0"?>
<root>
  <child>Hello</child>
</root>"#;

    let invalid_xml = br#"<?xml version="1.0"?>
<root>
  <wrongchild>Hello</wrongchild>
</root>"#;

    let schema = fastxml::schema::Schema::from_xsd(schema_str).expect("parse schema");
    let schema = Arc::new(schema);

    // Test DOM validator
    {
        let valid_doc = fastxml::Parser::from(valid_xml.as_slice())
            .parse()
            .expect("parse valid xml");
        let invalid_doc = fastxml::Parser::from(invalid_xml.as_slice())
            .parse()
            .expect("parse invalid xml");

        let errors = fastxml::schema::Validator::from(&valid_doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("validate")
            .into_entries();
        assert!(errors.is_empty(), "Valid document should validate (DOM)");

        let errors = fastxml::schema::Validator::from(&invalid_doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("validate")
            .into_entries();
        assert!(
            !errors.is_empty(),
            "Invalid document should not validate (DOM)"
        );
    }

    // Test streaming validation
    {
        let errors = fastxml::schema::Validator::from(valid_xml.as_slice())
            .schema(Arc::clone(&schema))
            .run()
            .expect("validate")
            .into_entries();
        assert!(
            errors.is_empty(),
            "Valid document should validate (Streaming)"
        );

        let errors = fastxml::schema::Validator::from(invalid_xml.as_slice())
            .schema(Arc::clone(&schema))
            .run()
            .expect("validate")
            .into_entries();
        assert!(
            !errors.is_empty(),
            "Invalid document should not validate (Streaming)"
        );
    }
}

/// Test that DOM and streaming validators produce consistent results.
#[test]
fn dom_streaming_consistency() {
    let schema_str = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="a" type="xs:string" minOccurs="1"/>
        <xs:element name="b" type="xs:integer" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

    let test_cases = [
        // (xml, expected_valid)
        (br#"<root><a>hello</a></root>"#.as_slice(), true),
        (br#"<root><a>hello</a><b>42</b></root>"#.as_slice(), true),
        (br#"<root><b>42</b></root>"#.as_slice(), false), // missing required 'a'
        (
            br#"<root><a>hello</a><c>extra</c></root>"#.as_slice(),
            false,
        ), // unexpected element
    ];

    let schema = Arc::new(fastxml::schema::Schema::from_xsd(schema_str).expect("parse schema"));

    for (xml, expected_valid) in test_cases {
        // DOM validation
        let doc = fastxml::Parser::from(xml).parse().expect("parse xml");
        let dom_errors = fastxml::schema::Validator::from(&doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("dom validate")
            .into_entries();
        let dom_valid = dom_errors.is_empty();

        // Streaming validation
        let stream_errors = fastxml::schema::Validator::from(xml)
            .schema(Arc::clone(&schema))
            .run()
            .expect("stream validate")
            .into_entries();
        let stream_valid = stream_errors.is_empty();

        // Check both validators agree
        assert_eq!(
            dom_valid,
            stream_valid,
            "DOM and streaming validators should agree for {:?}",
            std::str::from_utf8(xml).unwrap_or("<invalid utf8>")
        );

        // Check against expected
        assert_eq!(
            dom_valid,
            expected_valid,
            "Validation result mismatch for {:?}",
            std::str::from_utf8(xml).unwrap_or("<invalid utf8>")
        );
    }
}
