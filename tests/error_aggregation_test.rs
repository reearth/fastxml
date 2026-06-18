//! Error aggregation: identical errors collapse into one entry with a count.

use fastxml::Parser;
use fastxml::schema::{Schema, Validator};

const SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

/// Five identical type violations plus one distinct one.
const XML: &str = r#"<root>
  <n>x</n>
  <n>x</n>
  <n>x</n>
  <n>x</n>
  <n>x</n>
  <n>999999999999999999999</n>
</root>"#;

fn schema() -> Schema {
    Schema::from_xsd(SCHEMA.as_bytes()).unwrap()
}

#[test]
fn streaming_aggregates_identical_errors() {
    let report = Validator::from(XML)
        .schema(schema())
        .aggregate_errors()
        .run()
        .unwrap();

    assert!(!report.is_valid());
    // The five identical "not a valid xs:int" errors collapse into one
    // entry with count 5; the out-of-range error stays separate.
    let errors = report.errors();
    assert!(
        errors.len() < 6,
        "expected aggregation, got {} entries: {:#?}",
        errors.len(),
        errors
    );
    let max_count = errors.iter().map(|e| e.count).max().unwrap();
    assert_eq!(max_count, 5, "errors: {:#?}", errors);
    // Total occurrences are preserved through the counts.
    let total: u32 = errors.iter().map(|e| e.count).sum();
    assert_eq!(total, 6, "errors: {:#?}", errors);
}

#[test]
fn dom_aggregates_identical_errors() {
    let doc = Parser::from(XML).parse().unwrap();
    let report = Validator::from(&doc)
        .schema(schema())
        .aggregate_errors()
        .run()
        .unwrap();

    assert!(!report.is_valid());
    let errors = report.errors();
    assert!(
        errors.len() < 6,
        "expected aggregation, got {} entries: {:#?}",
        errors.len(),
        errors
    );
    let max_count = errors.iter().map(|e| e.count).max().unwrap();
    assert_eq!(max_count, 5, "errors: {:#?}", errors);
}

#[test]
fn without_aggregation_all_errors_are_kept() {
    let report = Validator::from(XML).schema(schema()).run().unwrap();
    assert_eq!(report.error_count(), 6);
    assert!(report.errors().iter().all(|e| e.count == 1));
}

#[test]
fn aggregated_errors_keep_all_positions() {
    let report = Validator::from(XML)
        .schema(schema())
        .aggregate_errors()
        .run()
        .unwrap();
    let repeated = report
        .errors()
        .into_iter()
        .find(|e| e.count == 5)
        .expect("aggregated entry");

    // First occurrence's position is in `location`; the other four are
    // recorded as (line, column) pairs.
    assert_eq!(repeated.location.line, Some(2));
    let more = repeated.more_positions.as_deref().expect("positions");
    assert_eq!(more.len(), 4);
    let lines: Vec<u32> = more.iter().map(|&(l, _)| l).collect();
    assert_eq!(lines, vec![3, 4, 5, 6]);
    assert_eq!(repeated.line_range(), Some((2, 6)));
}

#[test]
fn aggregated_error_display_shows_count() {
    let report = Validator::from(XML)
        .schema(schema())
        .aggregate_errors()
        .run()
        .unwrap();
    let repeated = report
        .errors()
        .into_iter()
        .find(|e| e.count == 5)
        .expect("aggregated entry");
    assert!(
        repeated.to_string().contains("×5, lines 2-6"),
        "display: {}",
        repeated
    );
}
