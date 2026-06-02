//! Tests for the redesigned `Validator` entry point and its `Report`.

use std::io::BufReader;
use std::sync::Arc;

use fastxml::parse;
use fastxml::schema::{Schema, Validator};

const XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="age" type="xs:int"/>
</xs:schema>"#;

const VALID: &str = r#"<?xml version="1.0"?><age>42</age>"#;
const INVALID: &str = r#"<?xml version="1.0"?><age>oops</age>"#;

fn schema() -> Arc<Schema> {
    Arc::new(Schema::from_xsd(XSD.as_bytes()).unwrap())
}

#[test]
fn streaming_explicit_schema_valid() {
    let report = Validator::from(VALID).schema(schema()).run().unwrap();
    assert!(report.is_valid(), "errors: {:?}", report.errors());
    assert!(report.is_clean());
}

#[test]
fn streaming_explicit_schema_invalid() {
    let report = Validator::from(INVALID).schema(schema()).run().unwrap();
    assert!(!report.is_valid());
    assert!(!report.errors().is_empty());
    assert!(report.error_count() >= 1);
}

#[test]
fn dom_explicit_schema_valid() {
    let doc = parse(VALID.as_bytes()).unwrap();
    let report = Validator::from(&doc).schema(schema()).run().unwrap();
    assert!(report.is_valid(), "errors: {:?}", report.errors());
}

#[test]
fn dom_explicit_schema_invalid() {
    let doc = parse(INVALID.as_bytes()).unwrap();
    let report = Validator::from(&doc).schema(schema()).run().unwrap();
    assert!(!report.is_valid());
}

#[test]
fn from_reader_valid() {
    let report = Validator::from_reader(BufReader::new(VALID.as_bytes()))
        .schema(schema())
        .run()
        .unwrap();
    assert!(report.is_valid(), "errors: {:?}", report.errors());
}

#[test]
fn from_bytes_input() {
    let report = Validator::from(VALID.as_bytes())
        .schema(schema())
        .run()
        .unwrap();
    assert!(report.is_valid());
}

#[test]
fn dom_and_streaming_agree() {
    let s = schema();
    for (xml, expected_valid) in [(VALID, true), (INVALID, false)] {
        let doc = parse(xml.as_bytes()).unwrap();
        let dom = Validator::from(&doc).schema(Arc::clone(&s)).run().unwrap();
        let stream = Validator::from(xml).schema(Arc::clone(&s)).run().unwrap();
        assert_eq!(dom.is_valid(), expected_valid, "DOM disagreed for {xml}");
        assert_eq!(
            stream.is_valid(),
            expected_valid,
            "stream disagreed for {xml}"
        );
        assert_eq!(
            dom.is_valid(),
            stream.is_valid(),
            "DOM and streaming disagreed for {xml}"
        );
    }
}

#[test]
fn max_errors_caps_collection() {
    // Two bad children → cap at 1 should collect at most 1 error.
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = r#"<?xml version="1.0"?><root><n>a</n><n>b</n><n>c</n></root>"#;
    let s = Arc::new(Schema::from_xsd(xsd.as_bytes()).unwrap());

    let capped = Validator::from(xml)
        .schema(Arc::clone(&s))
        .max_errors(1)
        .run()
        .unwrap();
    assert!(!capped.is_valid());
    assert!(
        capped.error_count() <= 1,
        "expected cap of 1, got {}",
        capped.error_count()
    );
}
