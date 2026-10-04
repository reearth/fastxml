//! Validation with the schema auto-detected from the instance document's
//! `xsi:schemaLocation` / `xsi:noNamespaceSchemaLocation` hints.
//!
//! The rule these tests pin: a run that could not load the schema the
//! document asks for must not report `is_valid() == true`, and the DOM and
//! streaming engines must reach the same verdict.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fastxml::ValidationErrorType;
use fastxml::schema::{DefaultFetcher, Report, ValidationMode, Validator};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

const SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns="urn:t" elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
      <xs:element name="ref" type="xs:IDREF" minOccurs="0"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

const NO_NS_SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let f = Self { dir };
        f.write("s.xsd", SCHEMA);
        f.write("nons.xsd", NO_NS_SCHEMA);
        f.write(
            "broken.xsd",
            "<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'><xs:element",
        );
        f.write(
            "bad_import.xsd",
            r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns="urn:t" elementFormDefault="qualified">
  <xs:import namespace="urn:other" schemaLocation="does-not-exist.xsd"/>
  <xs:element name="root"><xs:complexType><xs:sequence><xs:element name="n" type="xs:int" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
        );
        f
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn loc(&self, name: &str) -> String {
        self.path(name).display().to_string()
    }

    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.path(name), content).unwrap();
    }

    fn doc(&self, schema: &str, body: &str) -> String {
        format!(
            r#"<root xmlns="urn:t" xmlns:xsi="{XSI}" xsi:schemaLocation="urn:t {}">{body}</root>"#,
            self.loc(schema)
        )
    }
}

fn stream(xml: &str) -> Report {
    Validator::from(xml)
        .run_with(DefaultFetcher::new())
        .unwrap()
}

fn dom(xml: &str) -> Report {
    let doc = fastxml::Parser::from(xml).parse().unwrap();
    Validator::from(&doc)
        .run_with(DefaultFetcher::new())
        .unwrap()
}

/// Both engines; panics with the engine name on failure.
fn both(xml: &str, check: impl Fn(&str, &Report)) {
    check("stream", &stream(xml));
    check("dom", &dom(xml));
}

fn assert_schema_not_loaded(engine: &str, report: &Report, needle: &str) {
    assert!(
        !report.is_valid(),
        "[{engine}] a schema that could not be loaded must not yield a valid verdict: {:?}",
        report.entries()
    );
    let err = report
        .errors()
        .into_iter()
        .find(|e| e.error_type == ValidationErrorType::SchemaNotFound)
        .unwrap_or_else(|| panic!("[{engine}] no SchemaNotFound error: {:?}", report.entries()));
    assert!(
        err.message.contains(needle),
        "[{engine}] message should name {needle:?}: {}",
        err.message
    );
}

#[test]
fn control_loaded_schema_reports_content_errors() {
    let f = Fixture::new();
    both(&f.doc("s.xsd", "<n>abc</n>"), |engine, r| {
        assert!(!r.is_valid(), "[{engine}] {:?}", r.entries());
        assert!(
            r.errors()
                .iter()
                .all(|e| e.error_type != ValidationErrorType::SchemaNotFound),
            "[{engine}] {:?}",
            r.entries()
        );
    });
    both(&f.doc("s.xsd", "<n>1</n>"), |engine, r| {
        assert!(r.is_valid(), "[{engine}] {:?}", r.entries());
    });
}

#[test]
fn unreachable_schema_location_is_an_error() {
    let f = Fixture::new();
    let missing = f.loc("missing.xsd");
    both(&f.doc("missing.xsd", "<n>1</n>"), |engine, r| {
        assert_schema_not_loaded(engine, r, &missing)
    });
}

#[test]
fn unparseable_schema_is_an_error() {
    let f = Fixture::new();
    let broken = f.loc("broken.xsd");
    both(&f.doc("broken.xsd", "<n>1</n>"), |engine, r| {
        assert_schema_not_loaded(engine, r, &broken)
    });
}

#[test]
fn unreachable_import_is_an_error() {
    let f = Fixture::new();
    both(&f.doc("bad_import.xsd", "<n>1</n>"), |engine, r| {
        assert_schema_not_loaded(engine, r, "does-not-exist.xsd")
    });
}

#[test]
fn document_without_schema_hints_is_an_error() {
    both(r#"<root xmlns="urn:t"><n>1</n></root>"#, |engine, r| {
        assert_schema_not_loaded(engine, r, "xsi:schemaLocation")
    });
}

#[test]
fn no_namespace_schema_location_is_honored() {
    let f = Fixture::new();
    let doc = |body: &str| {
        format!(
            r#"<root xmlns:xsi="{XSI}" xsi:noNamespaceSchemaLocation="{}">{body}</root>"#,
            f.loc("nons.xsd")
        )
    };
    both(&doc("<n>abc</n>"), |engine, r| {
        assert!(!r.is_valid(), "[{engine}] {:?}", r.entries());
        assert!(
            r.errors()
                .iter()
                .all(|e| e.error_type != ValidationErrorType::SchemaNotFound),
            "[{engine}] {:?}",
            r.entries()
        );
    });
    both(&doc("<n>1</n>"), |engine, r| {
        assert!(r.is_valid(), "[{engine}] {:?}", r.entries())
    });
}

#[test]
fn xsi_attributes_are_recognized_by_namespace_not_prefix() {
    let f = Fixture::new();
    // Any prefix bound to the XSI namespace works.
    let other_prefix = format!(
        r#"<root xmlns="urn:t" xmlns:i="{XSI}" i:schemaLocation="urn:t {}"><n>abc</n></root>"#,
        f.loc("s.xsd")
    );
    both(&other_prefix, |engine, r| {
        assert!(!r.is_valid(), "[{engine}] {:?}", r.entries());
        assert!(
            r.errors()
                .iter()
                .all(|e| e.error_type != ValidationErrorType::SchemaNotFound),
            "[{engine}] schema should have loaded: {:?}",
            r.entries()
        );
    });
    // `xsi:` bound to some other namespace is not a schema hint.
    let wrong_ns = format!(
        r#"<root xmlns="urn:t" xmlns:xsi="urn:not-xsi" xsi:schemaLocation="urn:t {}"><n>1</n></root>"#,
        f.loc("s.xsd")
    );
    both(&wrong_ns, |engine, r| {
        assert_schema_not_loaded(engine, r, "xsi:schemaLocation")
    });
}

#[test]
fn odd_schema_location_token_count_is_an_error() {
    let xml = format!(r#"<root xmlns="urn:t" xmlns:xsi="{XSI}" xsi:schemaLocation="urn:t"/>"#);
    both(&xml, |engine, r| {
        assert_schema_not_loaded(engine, r, "urn:t")
    });
}

#[test]
fn streaming_autodetect_reports_finish_time_errors() {
    let f = Fixture::new();
    let r = stream(&f.doc("s.xsd", "<n>1</n><ref>nope</ref>"));
    assert!(
        r.errors()
            .iter()
            .any(|e| e.message.contains("IDREF 'nope'")),
        "{:?}",
        r.entries()
    );
}

#[test]
fn streaming_autodetect_keeps_every_occurrence() {
    let f = Fixture::new();
    // Lines 2 and 4 produce identical messages; both must be kept.
    let xml = f.doc("s.xsd", "\n<n>a</n>\n<n>b</n>\n<n>a</n>\n");
    let s = stream(&xml);
    let d = dom(&xml);
    assert_eq!(s.error_count(), 3, "{:?}", s.entries());
    assert_eq!(d.error_count(), 3, "{:?}", d.entries());
}

#[test]
fn streaming_autodetect_is_linear_in_error_count() {
    let f = Fixture::new();
    let n = 4000;
    let body: String = (0..n).map(|i| format!("<n>x{i}</n>")).collect();
    let xml = f.doc("s.xsd", &body);
    let start = Instant::now();
    let r = stream(&xml);
    let elapsed = start.elapsed();
    assert_eq!(r.error_count(), n);
    assert!(
        elapsed < Duration::from_secs(20),
        "{n} errors took {elapsed:?}"
    );
}

#[test]
fn autodetect_honors_max_errors_mode_and_aggregation() {
    let f = Fixture::new();
    let xml = f.doc("s.xsd", "<n>a</n><n>a</n><n>a</n>");
    let doc = fastxml::Parser::from(xml.as_str()).parse().unwrap();

    let capped_s = Validator::from(xml.as_str())
        .max_errors(1)
        .run_with(DefaultFetcher::new())
        .unwrap();
    let capped_d = Validator::from(&doc)
        .max_errors(1)
        .run_with(DefaultFetcher::new())
        .unwrap();
    assert_eq!(capped_s.error_count(), 1, "{:?}", capped_s.entries());
    assert_eq!(capped_d.error_count(), 1, "{:?}", capped_d.entries());

    let agg_s = Validator::from(xml.as_str())
        .aggregate_errors()
        .run_with(DefaultFetcher::new())
        .unwrap();
    let agg_d = Validator::from(&doc)
        .aggregate_errors()
        .run_with(DefaultFetcher::new())
        .unwrap();
    assert_eq!(agg_s.error_count(), 1, "{:?}", agg_s.entries());
    assert_eq!(agg_s.errors()[0].count, 3);
    assert_eq!(agg_d.error_count(), 1, "{:?}", agg_d.entries());
    assert_eq!(agg_d.errors()[0].count, 3);

    // Lenient mode suppresses undeclared-element reports on both paths,
    // exactly as with an explicit schema.
    let undeclared = f.doc("s.xsd", "<n>1</n><bogus/>");
    let udoc = fastxml::Parser::from(undeclared.as_str()).parse().unwrap();
    let explicit = fastxml::schema::Schema::from_xsd(SCHEMA).unwrap();
    let want = Validator::from(undeclared.as_str())
        .schema(explicit)
        .mode(ValidationMode::Lenient)
        .run()
        .unwrap()
        .error_count();
    let got_s = Validator::from(undeclared.as_str())
        .mode(ValidationMode::Lenient)
        .run_with(DefaultFetcher::new())
        .unwrap()
        .error_count();
    let got_d = Validator::from(&udoc)
        .mode(ValidationMode::Lenient)
        .run_with(DefaultFetcher::new())
        .unwrap()
        .error_count();
    assert_eq!(got_s, want);
    assert_eq!(got_d, want);
}

#[test]
fn relative_hint_resolves_against_fetcher_base_dir() {
    let f = Fixture::new();
    let xml = r#"<root xmlns="urn:t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:t s.xsd"><n>abc</n></root>"#;
    let base: &Path = f.dir.path();
    let r = Validator::from(xml)
        .run_with(DefaultFetcher::with_base_dir(base))
        .unwrap();
    assert!(!r.is_valid());
    assert!(
        r.errors()
            .iter()
            .all(|e| e.error_type != ValidationErrorType::SchemaNotFound),
        "{:?}",
        r.entries()
    );
}
