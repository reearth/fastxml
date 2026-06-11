//! XSD 1.1 datatype validation (dateTimeStamp, dayTimeDuration,
//! yearMonthDuration, explicitTimezone facet).

use std::sync::Arc;

use fastxml::schema::{Schema, Validator};

fn validate(schema: &[u8], xml: &str) -> bool {
    let schema = Arc::new(Schema::from_xsd(schema).expect("schema"));
    Validator::from(xml.as_bytes())
        .schema(schema)
        .run()
        .expect("validate")
        .into_entries()
        .is_empty()
}

const SCHEMA: &[u8] = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="stamp" type="xs:dateTimeStamp"/>
  <xs:element name="dt" type="xs:dayTimeDuration"/>
  <xs:element name="ym" type="xs:yearMonthDuration"/>
  <xs:element name="noTz">
    <xs:simpleType>
      <xs:restriction base="xs:dateTime">
        <xs:explicitTimezone value="prohibited"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

#[test]
fn datetimestamp_requires_timezone() {
    assert!(validate(SCHEMA, "<stamp>2026-06-11T10:00:00Z</stamp>"));
    assert!(!validate(SCHEMA, "<stamp>2026-06-11T10:00:00</stamp>"));
}

#[test]
fn duration_subtypes() {
    assert!(validate(SCHEMA, "<dt>P1DT2H</dt>"));
    assert!(!validate(SCHEMA, "<dt>P1Y</dt>"));
    assert!(validate(SCHEMA, "<ym>P2Y6M</ym>"));
    assert!(!validate(SCHEMA, "<ym>P1D</ym>"));
}

#[test]
fn explicit_timezone_facet() {
    assert!(validate(SCHEMA, "<noTz>2026-06-11T10:00:00</noTz>"));
    assert!(!validate(SCHEMA, "<noTz>2026-06-11T10:00:00Z</noTz>"));
}
