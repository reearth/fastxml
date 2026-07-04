//! Schema parsing error tests.

mod common;

use fastxml::error::Error;
use fastxml::schema::Schema;
use fastxml::schema::error::SchemaError;

#[test]
fn test_invalid_xsd_syntax() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:nonexistent"/>
        </xs:schema>"#;

    // xs:nonexistent is not a built-in XSD type; the reference-integrity
    // pass rejects the schema at compile time.
    let result = Schema::from_xsd(xsd.as_bytes());
    assert!(
        result.is_err(),
        "reference to xs:nonexistent must be rejected"
    );
}

#[test]
fn test_xsd_missing_namespace() {
    let xsd = r#"<schema>
            <element name="test" type="string"/>
        </schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser is lenient and accepts missing namespace
    assert!(result.is_ok(), "Parser accepts schema without namespace");
}

#[test]
fn test_xsd_invalid_min_occurs() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="child" minOccurs="-1"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser rejects invalid minOccurs values
    assert!(
        matches!(&result, Err(e) if format!("{:?}", e).contains("minOccurs") || format!("{:?}", e).contains("negative")),
        "Expected error about invalid minOccurs, got: {:?}",
        result
    );
}

#[test]
fn test_xsd_min_greater_than_max() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="child" minOccurs="5" maxOccurs="3"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser rejects minOccurs > maxOccurs
    assert!(
        matches!(
            &result,
            Err(Error::Schema(SchemaError::MinOccursGreaterThanMaxOccurs {
                min: 5,
                max: 3
            }))
        ),
        "Expected error about minOccurs > maxOccurs, got: {:?}",
        result
    );
}

#[test]
fn test_xsd_circular_type_reference() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="TypeA">
                <xs:complexContent>
                    <xs:extension base="TypeB"/>
                </xs:complexContent>
            </xs:complexType>
            <xs:complexType name="TypeB">
                <xs:complexContent>
                    <xs:extension base="TypeA"/>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // Circular type derivation violates ct-props-correct; the compiler
    // rejects the schema.
    assert!(result.is_err(), "circular type derivation must be rejected");
}

#[test]
fn test_xsd_duplicate_element_name() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
            <xs:element name="test" type="xs:integer"/>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // Duplicate global element names
    // Later definition may override earlier
    if let Ok(schema) = result {
        assert!(schema.elements.contains_key("test"));
    }
}

#[test]
fn test_xsd_invalid_facet_value() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="BadLength">
                <xs:restriction base="xs:string">
                    <xs:minLength value="-5"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser rejects negative length values
    assert!(
        matches!(&result, Err(e) if format!("{:?}", e).contains("minLength") || format!("{:?}", e).contains("negative")),
        "Expected error about negative length, got: {:?}",
        result
    );
}

#[test]
fn test_xsd_conflicting_facets() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="Conflicting">
                <xs:restriction base="xs:string">
                    <xs:minLength value="10"/>
                    <xs:maxLength value="5"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

    let result = Schema::from_xsd(xsd.as_bytes());
    // XSD parser rejects conflicting facets (minLength > maxLength)
    assert!(
        matches!(
            &result,
            Err(Error::Schema(SchemaError::MinLengthGreaterThanMaxLength {
                min_length: 10,
                max_length: 5
            }))
        ),
        "Expected error about conflicting facets, got: {:?}",
        result
    );
}
