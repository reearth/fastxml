//! Auto-detected schemas fetched through a `RestrictedFetcher`.

use std::path::Path;

use fastxml::Parser;
use fastxml::schema::{FileFetcher, RestrictedFetcher, Validator};

const SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns="urn:t" elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

fn doc_naming(schema: &Path) -> String {
    format!(
        r#"<root xmlns="urn:t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:t {}"><n>1</n></root>"#,
        schema.display()
    )
}

/// `<tmp>/allowed/s.xsd` and `<tmp>/outside/s.xsd` (same content).
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for dir in ["allowed", "outside"] {
        std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
        std::fs::write(tmp.path().join(dir).join("s.xsd"), SCHEMA).unwrap();
    }
    tmp
}

#[test]
fn schema_inside_allowed_dir_validates() {
    let tmp = fixture();
    let allowed = tmp.path().join("allowed");
    let xml = doc_naming(&allowed.join("s.xsd"));
    let fetcher = || RestrictedFetcher::new(FileFetcher::new()).allow_dir(&allowed);

    let doc = Parser::from(xml.as_bytes()).parse().unwrap();
    let report = Validator::from(&doc).run_with(fetcher()).unwrap();
    assert!(report.is_valid(), "{:?}", report.errors());

    let report = Validator::from(xml.as_str()).run_with(fetcher()).unwrap();
    assert!(report.is_valid(), "{:?}", report.errors());
}

#[test]
fn schema_outside_allowed_dir_is_a_load_failure() {
    let tmp = fixture();
    let allowed = tmp.path().join("allowed");
    let escape = allowed.join("../outside/s.xsd");
    let xml = doc_naming(&escape);
    let fetcher = || RestrictedFetcher::new(FileFetcher::new()).allow_dir(&allowed);

    let doc = Parser::from(xml.as_bytes()).parse().unwrap();
    let report = Validator::from(&doc).run_with(fetcher()).unwrap();
    assert!(!report.is_valid());
    let messages: Vec<String> = report.errors().iter().map(|e| e.to_string()).collect();
    assert!(
        messages.iter().any(|m| m.contains("not allowed")),
        "{messages:?}"
    );

    let report = Validator::from(xml.as_str()).run_with(fetcher()).unwrap();
    assert!(!report.is_valid());
}
