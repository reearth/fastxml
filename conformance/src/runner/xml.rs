//! W3C XML Conformance Test Suite runner.

use super::{
    AuditHistogram, EncodingVerdict, Engine, Eval, SuiteRun, panic_message, rel, sniff_encoding,
    xml_edition_applies, xml_namespace_applies, xml_version_applies,
};
use crate::catalog::xmlconf::{TestType, XmlConfCatalog, XmlConfTest};
use crate::outcome::{Outcome, TestRecord, classify_notwf_rejection, error_variant_name};
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// The category name for a test type.
fn category_of(test_type: TestType) -> &'static str {
    match test_type {
        TestType::Valid => "valid",
        TestType::Invalid => "invalid",
        TestType::NotWellFormed => "not-wf",
        TestType::Error => "error",
    }
}

/// Whether the document should be accepted (valid/invalid) or rejected (not-wf).
fn expected_label(test_type: TestType) -> &'static str {
    match test_type {
        TestType::Valid | TestType::Invalid => "should-accept",
        TestType::NotWellFormed => "should-reject",
        TestType::Error => "optional-error",
    }
}

/// Run the whole W3C XML suite with one engine.
pub fn run_xml_suite(engine: Engine, catalog: &XmlConfCatalog) -> SuiteRun {
    let mut run = SuiteRun::default();
    let mut audit = AuditHistogram::default();
    let base = &catalog.base_path;

    for test in catalog.all_tests() {
        let category = category_of(test.test_type);
        // Each test body is isolated: a parser panic becomes Outcome::Panic
        // instead of aborting the whole suite.
        let eval = catch_unwind(AssertUnwindSafe(|| evaluate_xml_test(engine, test, base)))
            .unwrap_or_else(|payload| {
                Eval::new(Outcome::Panic, Some(panic_message(payload.as_ref())))
            });

        if let Some(variant) = eval.audit_variant {
            audit.record(category, expected_label(test.test_type), variant);
        }
        run.push(TestRecord::new(
            category,
            test.id.clone(),
            eval.outcome,
            eval.detail,
        ));
    }

    if crate::should_audit() && !audit.is_empty() {
        audit.print(&format!("W3C XML ({})", engine.as_str()));
    }
    run
}

fn evaluate_xml_test(engine: Engine, test: &XmlConfTest, base: &Path) -> Eval {
    // Applicability gates (deliberately-unsupported features).
    if test.test_type == TestType::Error {
        // Optional error tests: a conforming processor MAY but need not report
        // these. We record them as Unsupported so they stay visible.
        return Eval::new(Outcome::Unsupported, Some("optional-error-test".into()));
    }
    if !xml_version_applies(test.version.as_deref()) {
        return Eval::new(Outcome::Unsupported, Some("xml-1.1".into()));
    }
    if !xml_edition_applies(test.edition.as_deref()) {
        return Eval::new(Outcome::Unsupported, Some("5th-edition".into()));
    }
    if !xml_namespace_applies(test.namespace.as_deref()) {
        // fastxml is always namespace-aware; a test that assumes namespace
        // processing is off (e.g. a bare colon as an ordinary name character)
        // cannot be run faithfully.
        return Eval::new(
            Outcome::Unsupported,
            Some("namespace-processing-off not supported".into()),
        );
    }

    let bytes = match std::fs::read(&test.uri) {
        Ok(b) => b,
        Err(e) => {
            return Eval::new(
                Outcome::Blocked,
                Some(format!("read {}: {}", rel(&test.uri, base), e)),
            );
        }
    };

    if let EncodingVerdict::Unsupported(reason) = sniff_encoding(&bytes) {
        return Eval::new(Outcome::Unsupported, Some(reason));
    }

    let result = parse(engine, bytes);

    match test.test_type {
        // A non-validating processor accepts both well-formed valid and
        // (DTD-)invalid documents; both expect a successful parse.
        TestType::Valid | TestType::Invalid => match result {
            Ok(()) => Eval::new(Outcome::Pass, None),
            Err(e) => {
                Eval::new(Outcome::Fail, Some(e.to_string())).with_audit(error_variant_name(&e))
            }
        },
        TestType::NotWellFormed => match result {
            Ok(()) => Eval::new(
                Outcome::Fail,
                Some("parser accepted not-well-formed input".into()),
            ),
            Err(e) => {
                let outcome = classify_notwf_rejection(&e);
                let detail = if outcome == Outcome::Pass {
                    None
                } else {
                    Some(e.to_string())
                };
                Eval::new(outcome, detail).with_audit(error_variant_name(&e))
            }
        },
        TestType::Error => unreachable!("error tests handled above"),
    }
}

/// Parse `bytes` with the selected engine, discarding the tree/events.
fn parse(engine: Engine, bytes: Vec<u8>) -> fastxml::Result<()> {
    match engine {
        Engine::Dom => fastxml::Parser::from(bytes.as_slice()).parse().map(|_| ()),
        Engine::Streaming => {
            fastxml::Parser::from_reader(Cursor::new(bytes)).for_each_event(|_| Ok(()))
        }
    }
}

/// Load the XML conformance catalog from the standard data location.
///
/// Returns `None` (with a message on stderr) when the data is not present, so
/// tests can skip cleanly.
pub fn load_catalog(data_path: &Path) -> Option<XmlConfCatalog> {
    let xmlconf_path = data_path.join("xmlconf");
    let catalog_path = if xmlconf_path.exists() {
        xmlconf_path.join("xmlconf.xml")
    } else {
        data_path.join("xmlconf.xml")
    };
    if !catalog_path.exists() {
        eprintln!("Catalog not found at {}", catalog_path.display());
        return None;
    }
    match XmlConfCatalog::parse(&catalog_path) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("Failed to parse catalog: {e}");
            None
        }
    }
}
