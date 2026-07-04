//! W3C XML Schema Test Suite tests.
//!
//! Thin wrapper: load the suite, run it via the runner library, then diff
//! against a committed baseline. Logic lives in
//! `fastxml_conformance::runner::xsd` and `::baseline`.

use fastxml_conformance::baseline::Baseline;
use fastxml_conformance::reporter::print_suite_run;
use fastxml_conformance::runner::Engine;
use fastxml_conformance::runner::xsd::{load_suite, run_xsd_suite};
use fastxml_conformance::{baselines_dir, require_test_data, should_update_baseline};
use std::sync::Arc;

fn run_and_check(engine: Engine, baseline_name: &str) {
    let data_path = require_test_data!("w3c-xsd");
    let Some(suite) = load_suite(&data_path) else {
        eprintln!("XSD suite unavailable; skipping {baseline_name}");
        return;
    };

    // Pinned catalog shape. Update DELIBERATELY (and regenerate baselines) if
    // the test data changes.
    // suite.xml references 95 testSetRefs; 2 point to files absent from the
    // clone, so 93 test sets parse.
    const TEST_SETS: usize = 93;
    const SCHEMA_TOTAL: usize = 14505;
    const INSTANCE_TOTAL: usize = 25108;

    assert_eq!(
        suite.test_sets.len(),
        TEST_SETS,
        "testSetRef count changed; update TEST_SETS and regenerate baselines"
    );

    let run = run_xsd_suite(engine, &suite);
    print_suite_run(&format!("W3C XSD Conformance ({})", engine.as_str()), &run);

    let schema_total: usize = run
        .categories
        .iter()
        .filter(|(k, _)| k.starts_with("schema-"))
        .map(|(_, c)| c.total())
        .sum();
    let instance_total: usize = run
        .categories
        .iter()
        .filter(|(k, _)| k.starts_with("instance-"))
        .map(|(_, c)| c.total())
        .sum();
    assert_eq!(
        schema_total, SCHEMA_TOTAL,
        "schema test count changed; update SCHEMA_TOTAL and regenerate baselines"
    );
    assert_eq!(
        instance_total, INSTANCE_TOTAL,
        "instance test count changed; update INSTANCE_TOTAL and regenerate baselines"
    );

    let path = baselines_dir().join(format!("{baseline_name}.tsv"));
    if should_update_baseline() {
        Baseline::from_records(&run.records)
            .write(&path)
            .expect("write baseline");
        eprintln!("Updated baseline: {}", path.display());
        return;
    }

    let baseline = Baseline::load(&path).unwrap_or_else(|e| {
        panic!(
            "cannot load baseline {}: {e}\nRun `FASTXML_UPDATE_BASELINE=1 cargo test \
             -p fastxml-conformance` to create it.",
            path.display()
        )
    });
    let diff = baseline.diff(&run.records);
    assert!(diff.is_clean(), "{}", diff.message(baseline_name));
}

#[test]
fn w3c_xsd_conformance_dom() {
    run_and_check(Engine::Dom, "w3c-xsd-dom");
}

#[test]
fn w3c_xsd_conformance_streaming() {
    run_and_check(Engine::Streaming, "w3c-xsd-streaming");
}

/// Basic XSD validation smoke test with both DOM and streaming validators.
/// Does not depend on external test data.
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

    let schema = Arc::new(fastxml::schema::Schema::from_xsd(schema_str).expect("parse schema"));

    // DOM validator.
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

    // Streaming validator.
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

/// DOM and streaming validators must agree, and match the expected verdict.
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
        (br#"<root><a>hello</a></root>"#.as_slice(), true),
        (br#"<root><a>hello</a><b>42</b></root>"#.as_slice(), true),
        (br#"<root><b>42</b></root>"#.as_slice(), false),
        (
            br#"<root><a>hello</a><c>extra</c></root>"#.as_slice(),
            false,
        ),
    ];

    let schema = Arc::new(fastxml::schema::Schema::from_xsd(schema_str).expect("parse schema"));

    for (xml, expected_valid) in test_cases {
        let doc = fastxml::Parser::from(xml).parse().expect("parse xml");
        let dom_valid = fastxml::schema::Validator::from(&doc)
            .schema(Arc::clone(&schema))
            .run()
            .expect("dom validate")
            .into_entries()
            .is_empty();

        let stream_valid = fastxml::schema::Validator::from(xml)
            .schema(Arc::clone(&schema))
            .run()
            .expect("stream validate")
            .into_entries()
            .is_empty();

        assert_eq!(
            dom_valid,
            stream_valid,
            "DOM and streaming validators should agree for {:?}",
            std::str::from_utf8(xml).unwrap_or("<invalid utf8>")
        );
        assert_eq!(
            dom_valid,
            expected_valid,
            "Validation result mismatch for {:?}",
            std::str::from_utf8(xml).unwrap_or("<invalid utf8>")
        );
    }
}
