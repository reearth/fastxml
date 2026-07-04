//! W3C XML Conformance Test Suite tests.
//!
//! Thin wrapper: load the catalog, run the suite via the runner library, then
//! diff against a committed baseline. All the logic lives in
//! `fastxml_conformance::runner::xml` and `::baseline`.

use fastxml_conformance::baseline::Baseline;
use fastxml_conformance::reporter::print_suite_run;
use fastxml_conformance::runner::Engine;
use fastxml_conformance::runner::xml::{load_catalog, run_xml_suite};
use fastxml_conformance::{baselines_dir, require_test_data, should_update_baseline};

/// Pinned number of real `<TEST>` elements reachable from `xmlconf.xml`.
///
/// The referenced sub-catalogs (james-clark, sun, ibm, oasis, eduni, japanese,
/// ...) contain 2586 `<TEST` occurrences, but one of them lives inside an XML
/// comment in `ibm/xml-1.1/ibm_not-wf.xml`, so 2585 real test elements are
/// parsed. If the test data changes and this assertion fires, update this
/// constant DELIBERATELY and regenerate the baselines with
/// `FASTXML_UPDATE_BASELINE=1`.
const XML_TOTAL: usize = 2585;

fn run_and_check(engine: Engine, baseline_name: &str) {
    let data_path = require_test_data!("w3c-xml");
    let Some(catalog) = load_catalog(&data_path) else {
        eprintln!("Catalog unavailable; skipping {baseline_name}");
        return;
    };

    let total = catalog.all_tests().count();
    assert_eq!(
        total, XML_TOTAL,
        "catalog test count changed ({total} != {XML_TOTAL}); update XML_TOTAL and \
         regenerate baselines deliberately"
    );

    let run = run_xml_suite(engine, &catalog);
    print_suite_run(&format!("W3C XML Conformance ({})", engine.as_str()), &run);

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
fn w3c_xml_conformance_dom() {
    run_and_check(Engine::Dom, "w3c-xml-dom");
}

#[test]
fn w3c_xml_conformance_streaming() {
    run_and_check(Engine::Streaming, "w3c-xml-streaming");
}
