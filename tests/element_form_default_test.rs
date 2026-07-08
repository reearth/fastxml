//! elementFormDefault="unqualified" end-to-end regression tests (C4).
//!
//! With the namespace-qualified element lookup, global elements live in the
//! schema's target namespace while local element declarations under
//! elementFormDefault="unqualified" (the XSD default) match *unqualified*
//! instance children. The ns-first lookup must not force local children into
//! the target namespace, and the no-namespace probe must not shadow them.

use std::sync::Arc;

use fastxml::Parser;
use fastxml::schema::{Schema, Validator};

const TNS: &str = "urn:example:forms";

/// targetNamespace + elementFormDefault unqualified (omitted = unqualified):
/// the root is qualified, its declared children are NOT.
fn unqualified_schema() -> Arc<Schema> {
    let xsd = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
               xmlns:t="{TNS}"
               targetNamespace="{TNS}">
        <xs:complexType name="RootType">
            <xs:sequence>
                <xs:element name="child" type="xs:string"/>
                <xs:element name="count" type="xs:integer" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
        <xs:element name="root" type="t:RootType"/>
    </xs:schema>"#
    );
    Arc::new(Schema::from_xsd(xsd.as_bytes()).expect("schema should compile"))
}

const VALID_INSTANCE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<t:root xmlns:t="urn:example:forms">
    <child>hello</child>
    <count>3</count>
</t:root>"#;

/// A bad value in an unqualified child must still be caught (proves the
/// child is actually validated, not skipped by the ns-first lookup).
const INVALID_INSTANCE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<t:root xmlns:t="urn:example:forms">
    <child>hello</child>
    <count>NOT_AN_INT</count>
</t:root>"#;

#[test]
fn unqualified_local_children_validate_streaming() {
    let schema = unqualified_schema();

    let report = Validator::from(VALID_INSTANCE)
        .schema(Arc::clone(&schema))
        .run()
        .expect("validation should run");
    assert!(
        report.is_valid(),
        "unqualified local children must validate under a target-namespaced \
         schema (streaming). Errors: {:?}",
        report
            .errors()
            .iter()
            .map(|e| &*e.message)
            .collect::<Vec<_>>()
    );

    let report = Validator::from(INVALID_INSTANCE)
        .schema(schema)
        .run()
        .expect("validation should run");
    assert!(
        !report.is_valid(),
        "a bad integer in an unqualified child must still be reported (streaming)"
    );
}

#[test]
fn unqualified_local_children_validate_dom() {
    let schema = unqualified_schema();

    let doc = Parser::from(VALID_INSTANCE).parse().expect("parse");
    let report = Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .expect("validation should run");
    assert!(
        report.is_valid(),
        "unqualified local children must validate under a target-namespaced \
         schema (DOM). Errors: {:?}",
        report
            .errors()
            .iter()
            .map(|e| &*e.message)
            .collect::<Vec<_>>()
    );

    let doc = Parser::from(INVALID_INSTANCE).parse().expect("parse");
    let report = Validator::from(&doc).schema(schema).run().expect("run");
    assert!(
        !report.is_valid(),
        "a bad integer in an unqualified child must still be reported (DOM)"
    );
}

/// A no-namespace schema (no targetNamespace at all): everything unqualified.
#[test]
fn no_namespace_schema_still_validates_both_engines() {
    let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
    <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="item" type="xs:string" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;
    let schema = Arc::new(Schema::from_xsd(xsd.as_bytes()).expect("schema should compile"));

    let xml = r#"<root><item>a</item><item>b</item></root>"#;

    let report = Validator::from(xml)
        .schema(Arc::clone(&schema))
        .run()
        .expect("run");
    assert!(
        report.is_valid(),
        "no-namespace document should validate (streaming). Errors: {:?}",
        report
            .errors()
            .iter()
            .map(|e| &*e.message)
            .collect::<Vec<_>>()
    );

    let doc = Parser::from(xml).parse().expect("parse");
    let report = Validator::from(&doc).schema(schema).run().expect("run");
    assert!(
        report.is_valid(),
        "no-namespace document should validate (DOM). Errors: {:?}",
        report
            .errors()
            .iter()
            .map(|e| &*e.message)
            .collect::<Vec<_>>()
    );
}
