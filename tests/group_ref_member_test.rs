//! Schema-validation bug: an element declared locally inside a named model
//! group (`<xs:group name="...">`) and pulled into a complex type via
//! `<xs:group ref="...">` is wrongly reported as "not declared in schema".
//!
//! When a complex type composes its content model by referencing a named group
//! instead of declaring the element inline, the element declaration lives one
//! level of indirection away (in the group, not in the type's own particle).
//! The validator must follow that `<xs:group ref>` to discover the element;
//! failing to do so makes a valid document look invalid.
//!
//! CityGML 3.0 is one real-world schema that uses this pattern:
//! `core:cityObjectMember` is a local element declared inside the named group
//! `core:CityModelMemberGroup`, which `core:CityModelType` pulls in via
//! `<xs:group ref="core:CityModelMemberGroup">`. fastxml accepts an empty
//! `CityModel` (Case A) but rejects a `CityModel` containing a
//! `cityObjectMember` (Case B) with
//! `element 'core:cityObjectMember' is not declared in schema` — a false
//! positive, since the element IS declared inside the group reached via
//! `<xs:group ref>`.
//!
//! The XSD below is a network-free distillation of the official OGC modules,
//! split across the real gml / core / building namespaces and compiled with
//! `parse_xsd_multiple`.

use std::sync::Arc;

use fastxml::Parser;
use fastxml::StructuredError;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::{Schema, Validator};

// --- GML 3.2 (simplified) -------------------------------------------------
// Provides the feature/geometry base types that CityGML extends: gml:id,
// gml:AbstractGMLType / AbstractFeatureType, gml:boundedBy + gml:Envelope, and
// gml:AbstractFeatureMemberType (the base of cityObjectMember's anonymous type).
const GML_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:gml="http://www.opengis.net/gml/3.2"
           targetNamespace="http://www.opengis.net/gml/3.2"
           elementFormDefault="qualified">

  <xs:attribute name="id" type="xs:ID"/>

  <xs:complexType name="AbstractGMLType" abstract="true">
    <xs:attribute ref="gml:id"/>
  </xs:complexType>

  <xs:element name="boundedBy" type="gml:BoundingShapeType"/>
  <xs:complexType name="BoundingShapeType">
    <xs:sequence>
      <xs:element ref="gml:Envelope" minOccurs="0"/>
    </xs:sequence>
  </xs:complexType>

  <xs:element name="Envelope" type="gml:EnvelopeType"/>
  <xs:complexType name="EnvelopeType">
    <xs:sequence>
      <xs:element name="lowerCorner" type="gml:DirectPositionType"/>
      <xs:element name="upperCorner" type="gml:DirectPositionType"/>
    </xs:sequence>
    <xs:attribute name="srsName" type="xs:anyURI"/>
    <xs:attribute name="srsDimension" type="xs:positiveInteger"/>
  </xs:complexType>
  <xs:complexType name="DirectPositionType">
    <xs:simpleContent>
      <xs:extension base="xs:string"/>
    </xs:simpleContent>
  </xs:complexType>

  <xs:complexType name="AbstractFeatureType" abstract="true">
    <xs:complexContent>
      <xs:extension base="gml:AbstractGMLType">
        <xs:sequence>
          <xs:element ref="gml:boundedBy" minOccurs="0"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <!-- Base of cityObjectMember's anonymous type. -->
  <xs:complexType name="AbstractFeatureMemberType" abstract="true">
    <xs:sequence/>
  </xs:complexType>
</xs:schema>"#;

// --- CityGML 3.0 core (simplified) ----------------------------------------
// The bug lives here: CityModelType reaches its members through
// <xs:group ref="core:CityModelMemberGroup">, and `cityObjectMember` is a LOCAL
// element declared inside that named group's <choice>.
const CORE_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:core="http://www.opengis.net/citygml/3.0"
           xmlns:gml="http://www.opengis.net/gml/3.2"
           targetNamespace="http://www.opengis.net/citygml/3.0"
           elementFormDefault="qualified">

  <xs:import namespace="http://www.opengis.net/gml/3.2"/>

  <xs:element name="CityModel" type="core:CityModelType"/>

  <xs:complexType name="CityModelType">
    <xs:complexContent>
      <xs:extension base="core:AbstractFeatureWithLifespanType">
        <xs:sequence>
          <!-- Members are pulled in via a named group reference, not via global
               element declarations. -->
          <xs:group ref="core:CityModelMemberGroup" minOccurs="0" maxOccurs="unbounded"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <!-- Extends gml:AbstractFeatureType, which supplies boundedBy. -->
  <xs:complexType name="AbstractFeatureWithLifespanType" abstract="true">
    <xs:complexContent>
      <xs:extension base="gml:AbstractFeatureType">
        <xs:sequence>
          <xs:element name="creationDate" type="xs:dateTime" minOccurs="0"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>

  <!-- The named model group. `cityObjectMember` is a local element declared
       inside this group's <choice>. -->
  <xs:group name="CityModelMemberGroup">
    <xs:choice>
      <xs:element name="cityObjectMember">
        <xs:complexType>
          <xs:complexContent>
            <xs:extension base="gml:AbstractFeatureMemberType">
              <xs:sequence minOccurs="0">
                <xs:element ref="core:AbstractCityObject"/>
              </xs:sequence>
            </xs:extension>
          </xs:complexContent>
        </xs:complexType>
      </xs:element>
    </xs:choice>
  </xs:group>

  <!-- Abstract head that thematic objects substitute for. -->
  <xs:element name="AbstractCityObject" type="core:AbstractCityObjectType" abstract="true"/>
  <xs:complexType name="AbstractCityObjectType" abstract="true">
    <xs:complexContent>
      <xs:extension base="core:AbstractFeatureWithLifespanType">
        <xs:sequence/>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>"#;

// --- CityGML 3.0 building (simplified) ------------------------------------
// bldg:Building substitutes for core:AbstractCityObject (the official chain
// Building -> AbstractBuilding -> AbstractConstruction -> ... -> AbstractCityObject
// is collapsed to a direct substitutionGroup for the test).
const BLDG_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:bldg="http://www.opengis.net/citygml/building/3.0"
           xmlns:core="http://www.opengis.net/citygml/3.0"
           targetNamespace="http://www.opengis.net/citygml/building/3.0"
           elementFormDefault="qualified">

  <xs:import namespace="http://www.opengis.net/citygml/3.0"/>

  <xs:element name="Building" type="bldg:BuildingType" substitutionGroup="core:AbstractCityObject"/>
  <xs:complexType name="BuildingType">
    <xs:complexContent>
      <xs:extension base="core:AbstractCityObjectType">
        <xs:sequence/>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>"#;

// Case A: empty CityModel (gml:boundedBy only, no members). Baseline that works.
const XML_CASE_A_EMPTY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/3.0"
                xmlns:gml="http://www.opengis.net/gml/3.2"
                xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                xsi:schemaLocation="http://www.opengis.net/citygml/3.0 https://schemas.opengis.net/citygml/3.0/core.xsd">
  <gml:boundedBy>
    <gml:Envelope srsName="http://www.opengis.net/def/crs/EPSG/0/6697" srsDimension="3">
      <gml:lowerCorner>34.7345 135.5020 0</gml:lowerCorner>
      <gml:upperCorner>34.7347 135.5022 12.5</gml:upperCorner>
    </gml:Envelope>
  </gml:boundedBy>
</core:CityModel>"#;

// Case B: CityModel with one cityObjectMember holding an (empty) bldg:Building.
// `cityObjectMember` is the LOCAL element reached via <xs:group ref>. This is
// valid CityGML 3.0, but fastxml reports `cityObjectMember` as not declared.
const XML_CASE_B_WITH_MEMBER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/3.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/3.0"
                xmlns:gml="http://www.opengis.net/gml/3.2"
                xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                xsi:schemaLocation="http://www.opengis.net/citygml/3.0 https://schemas.opengis.net/citygml/3.0/core.xsd http://www.opengis.net/citygml/building/3.0 https://schemas.opengis.net/citygml/building/3.0/building.xsd">
  <core:cityObjectMember>
    <bldg:Building gml:id="bldg_0001"/>
  </core:cityObjectMember>
</core:CityModel>"#;

/// Compiles the simplified gml / core / building modules into one schema using
/// the real CityGML 3.0 namespaces (no network fetch).
fn citygml_schema() -> Arc<CompiledSchema> {
    let schema = Schema::builder()
        .add(
            "https://schemas.opengis.net/gml/3.2.1/gml.xsd",
            GML_XSD.as_bytes(),
        )
        .add(
            "https://schemas.opengis.net/citygml/3.0/core.xsd",
            CORE_XSD.as_bytes(),
        )
        .add(
            "https://schemas.opengis.net/citygml/building/3.0/building.xsd",
            BLDG_XSD.as_bytes(),
        )
        .resolve()
        .expect("Failed to compile CityGML 3.0 schema modules");
    Arc::new(schema)
}

fn validate_dom(xml: &str) -> Vec<StructuredError> {
    let doc = Parser::from(xml).parse().expect("Failed to parse XML");
    Validator::from(&doc)
        .schema(citygml_schema())
        .run()
        .expect("Validation failed")
        .into_entries()
}

fn validate_onepass(xml: &str) -> Vec<StructuredError> {
    Validator::from(xml)
        .schema(citygml_schema())
        .run()
        .expect("Validation failed")
        .into_entries()
}

fn is_valid(errors: &[StructuredError]) -> bool {
    errors.iter().all(|e| !e.is_error())
}

/// Returns true if any error message claims an element is "not declared".
/// This is the precise symptom described in the bug report.
fn has_not_declared_error(errors: &[StructuredError]) -> bool {
    errors
        .iter()
        .filter(|e| e.is_error())
        .any(|e| e.to_string().contains("not declared"))
}

// ---------------------------------------------------------------------------
// Baseline: an empty container is accepted (this currently passes).
// ---------------------------------------------------------------------------

#[test]
fn case_a_empty_citymodel_is_valid_dom() {
    let errors = validate_dom(XML_CASE_A_EMPTY);
    assert!(
        is_valid(&errors),
        "DOM: empty CityModel should be valid, errors: {errors:?}"
    );
}

#[test]
fn case_a_empty_citymodel_is_valid_onepass() {
    let errors = validate_onepass(XML_CASE_A_EMPTY);
    assert!(
        is_valid(&errors),
        "OnePass: empty CityModel should be valid, errors: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// Bug reproduction: a CityModel with one cityObjectMember must be valid.
// `cityObjectMember` is a LOCAL element declared inside the named group
// `CityModelMemberGroup` and reached via `<xs:group ref>`.
//
// These tests are expected to FAIL until the group-ref / named-group local
// element resolution is fixed.
// ---------------------------------------------------------------------------

#[test]
fn case_b_member_via_group_ref_is_valid_onepass() {
    // OnePass == StreamValidator, the path used by the flow engine's XMLValidator.
    let errors = validate_onepass(XML_CASE_B_WITH_MEMBER);

    assert!(
        !has_not_declared_error(&errors),
        "OnePass: 'core:cityObjectMember' is a local element inside the named group reached via \
         <xs:group ref>, so it must NOT be reported as 'not declared'. errors: {errors:?}"
    );
    assert!(
        is_valid(&errors),
        "OnePass: CityModel with one cityObjectMember should be valid, errors: {errors:?}"
    );
}

#[test]
fn case_b_member_via_group_ref_is_valid_dom() {
    let errors = validate_dom(XML_CASE_B_WITH_MEMBER);

    assert!(
        !has_not_declared_error(&errors),
        "DOM: 'core:cityObjectMember' is a local element inside the named group reached via \
         <xs:group ref>, so it must NOT be reported as 'not declared'. errors: {errors:?}"
    );
    assert!(
        is_valid(&errors),
        "DOM: CityModel with one cityObjectMember should be valid, errors: {errors:?}"
    );
}
