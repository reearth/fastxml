//! Read-only content-model position query for XML producers (#60).
//!
//! The schema set mirrors CityGML 2.0 + PLATEAU i-UR: a core namespace with
//! an abstract ADE hook, a building namespace extending the core city object
//! type and declaring its own hook plus an `##other` wildcard, and an ADE
//! namespace whose elements join both hooks through substitution groups.

use std::sync::Arc;

use fastxml::schema::Schema;
use fastxml::schema::xsd::content_automaton::ContentAutomaton;

const CORE: &str = "urn:core";
const BLDG: &str = "urn:bldg";
const URO: &str = "urn:uro";

const CORE_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:core="urn:core" targetNamespace="urn:core" elementFormDefault="qualified">
  <xs:element name="_GenericApplicationPropertyOfCityObject" type="xs:anyType" abstract="true"/>
  <xs:complexType name="AbstractCityObjectType">
    <xs:sequence>
      <xs:element name="creationDate" type="xs:date" minOccurs="0"/>
      <xs:element ref="core:_GenericApplicationPropertyOfCityObject" minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="_CityObject" type="core:AbstractCityObjectType" abstract="true"/>
  <xs:group name="CoreGroup">
    <xs:sequence>
      <xs:element name="groupMember" type="xs:string" minOccurs="0"/>
      <xs:any namespace="##targetNamespace" processContents="lax" minOccurs="0"/>
    </xs:sequence>
  </xs:group>
</xs:schema>"###;

const BLDG_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:core="urn:core" xmlns:bldg="urn:bldg" targetNamespace="urn:bldg" elementFormDefault="qualified">
  <xs:import namespace="urn:core" schemaLocation="core.xsd"/>
  <xs:element name="_GenericApplicationPropertyOfAbstractBuilding" type="xs:anyType" abstract="true"/>
  <xs:complexType name="AbstractBuildingType">
    <xs:complexContent>
      <xs:extension base="core:AbstractCityObjectType">
        <xs:sequence>
          <xs:element name="class" type="xs:string" minOccurs="0"/>
          <xs:element name="measuredHeight" type="xs:double" minOccurs="0"/>
          <xs:element ref="bldg:_GenericApplicationPropertyOfAbstractBuilding" minOccurs="0" maxOccurs="unbounded"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="BuildingType">
    <xs:complexContent>
      <xs:extension base="bldg:AbstractBuildingType">
        <xs:sequence>
          <xs:any namespace="##other" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:element name="Building" type="bldg:BuildingType" substitutionGroup="core:_CityObject"/>
  <xs:element name="BuildingAlias" substitutionGroup="bldg:Building"/>
  <xs:element name="Annex">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="label" type="xs:string"/>
        <xs:element ref="bldg:_GenericApplicationPropertyOfAbstractBuilding" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:complexType name="GroupUserType">
    <xs:sequence>
      <xs:group ref="core:CoreGroup"/>
      <xs:element name="own" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
  <xs:complexType name="PairsType">
    <xs:sequence minOccurs="2" maxOccurs="2">
      <xs:element name="a" type="xs:string"/>
      <xs:element name="b" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>"###;

const URO_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:core="urn:core" xmlns:bldg="urn:bldg" xmlns:uro="urn:uro" targetNamespace="urn:uro" elementFormDefault="qualified">
  <xs:import namespace="urn:core" schemaLocation="core.xsd"/>
  <xs:import namespace="urn:bldg" schemaLocation="bldg.xsd"/>
  <xs:element name="buildingIDAttribute" type="xs:string" substitutionGroup="bldg:_GenericApplicationPropertyOfAbstractBuilding"/>
  <xs:element name="class" type="xs:string" substitutionGroup="bldg:_GenericApplicationPropertyOfAbstractBuilding"/>
  <xs:element name="cityObjectAttr" type="xs:string" substitutionGroup="core:_GenericApplicationPropertyOfCityObject"/>
  <xs:complexType name="ExtendedBuildingType">
    <xs:complexContent>
      <xs:extension base="bldg:BuildingType"/>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="UnqualifiedLocalsType" xmlns:p="urn:uro">
    <xs:sequence>
      <xs:element name="x" type="xs:string" form="unqualified"/>
      <xs:element name="y" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>"###;

fn schema() -> Schema {
    Schema::builder()
        .add("uro.xsd", URO_XSD.as_bytes().to_vec())
        .add("bldg.xsd", BLDG_XSD.as_bytes().to_vec())
        .add("core.xsd", CORE_XSD.as_bytes().to_vec())
        .resolve()
        .expect("compile schema set")
}

fn type_automaton(ns: &str, local: &str) -> Arc<ContentAutomaton> {
    schema()
        .type_content_automaton(ns, local)
        .unwrap_or_else(|| panic!("automaton for {{{ns}}}{local}"))
}

#[test]
fn sequence_with_substitution_members_and_wildcard() {
    let a = type_automaton(BLDG, "BuildingType");
    // creationDate, core hook, class, measuredHeight, bldg hook, ##other.
    assert_eq!(a.len(), 6);
    assert!(!a.is_empty());

    assert_eq!(a.position_of(Some(BLDG), "class"), Some(2));
    assert_eq!(a.position_of(Some(BLDG), "measuredHeight"), Some(3));
    // The abstract head and its substitution-group members share its slot.
    assert_eq!(
        a.position_of(Some(BLDG), "_GenericApplicationPropertyOfAbstractBuilding"),
        Some(4)
    );
    assert_eq!(a.position_of(Some(URO), "buildingIDAttribute"), Some(4));
    // The wildcard admits other namespaces only (judged against urn:bldg).
    assert_eq!(a.position_of(Some("urn:elsewhere"), "anything"), Some(5));
    assert_eq!(a.position_of(Some(BLDG), "unknown"), None);
    assert_eq!(a.position_of(None, "unknown"), None);
}

#[test]
fn matching_is_namespace_exact() {
    let a = type_automaton(BLDG, "BuildingType");
    // Same local name, different namespaces: bldg:class is the declared
    // property, uro:class is an ADE property at the building hook.
    assert_eq!(a.position_of(Some(BLDG), "class"), Some(2));
    assert_eq!(a.position_of(Some(URO), "class"), Some(4));
    // A bare local name or a wrong namespace does not select a declared
    // element; here only the ##other wildcard can take it.
    assert_eq!(a.position_of(None, "measuredHeight"), None);
    assert_eq!(a.position_of(Some(""), "measuredHeight"), None);
    assert_eq!(a.position_of(Some(CORE), "measuredHeight"), Some(5));
}

#[test]
fn inherited_content_model_comes_first() {
    let a = type_automaton(BLDG, "AbstractBuildingType");
    assert_eq!(a.len(), 5);
    assert_eq!(a.position_of(Some(CORE), "creationDate"), Some(0));
    assert_eq!(a.position_of(Some(BLDG), "class"), Some(2));
    // A type from a third namespace inherits the whole chain, including the
    // wildcard, which stays judged against the namespace that declared it.
    let ext = type_automaton(URO, "ExtendedBuildingType");
    assert_eq!(ext.len(), 6);
    assert_eq!(ext.position_of(Some(URO), "notDeclared"), Some(5));
    assert_eq!(ext.position_of(Some(BLDG), "notDeclared"), None);
}

#[test]
fn ade_hook_across_namespaces() {
    let a = type_automaton(BLDG, "BuildingType");
    // core's abstract hook, substituted by an i-UR element from urn:uro.
    assert_eq!(
        a.position_of(Some(CORE), "_GenericApplicationPropertyOfCityObject"),
        Some(1)
    );
    assert_eq!(a.position_of(Some(URO), "cityObjectAttr"), Some(1));
    // Ordering a producer would derive: core properties, then building
    // properties, then building ADE properties.
    let mut children = vec![
        (URO, "buildingIDAttribute"),
        (BLDG, "measuredHeight"),
        (URO, "cityObjectAttr"),
        (CORE, "creationDate"),
    ];
    children.sort_by_key(|(ns, local)| a.position_of(Some(ns), local).unwrap());
    assert_eq!(
        children,
        [
            (CORE, "creationDate"),
            (URO, "cityObjectAttr"),
            (BLDG, "measuredHeight"),
            (URO, "buildingIDAttribute"),
        ]
    );
}

#[test]
fn expansion_copies_return_the_first_position() {
    let a = type_automaton(BLDG, "PairsType");
    // (a, b){2}: four positions, a and b each matched first by copy one.
    assert_eq!(a.len(), 4);
    assert_eq!(a.position_of(Some(BLDG), "a"), Some(0));
    assert_eq!(a.position_of(Some(BLDG), "b"), Some(1));
}

#[test]
fn group_from_another_document_keeps_its_namespace() {
    let a = type_automaton(BLDG, "GroupUserType");
    assert_eq!(a.len(), 3);
    // Local elements of core's group are in urn:core (core is qualified).
    assert_eq!(a.position_of(Some(CORE), "groupMember"), Some(0));
    assert_eq!(a.position_of(Some(BLDG), "groupMember"), None);
    // ##targetNamespace means core's namespace, not the referencing one.
    assert_eq!(a.position_of(Some(CORE), "other"), Some(1));
    assert_eq!(a.position_of(Some(BLDG), "own"), Some(2));
}

#[test]
fn unqualified_local_elements_have_no_namespace() {
    let a = type_automaton(URO, "UnqualifiedLocalsType");
    assert_eq!(a.position_of(None, "x"), Some(0));
    assert_eq!(a.position_of(Some(URO), "x"), None);
    assert_eq!(a.position_of(Some(URO), "y"), Some(1));
    assert_eq!(a.position_of(None, "y"), None);
}

#[test]
fn element_content_automaton() {
    let schema = schema();
    // Named type.
    let building = schema
        .element_content_automaton(BLDG, "Building")
        .expect("Building");
    assert_eq!(building.len(), 6);
    assert_eq!(building.position_of(Some(URO), "class"), Some(4));
    // An untyped substitution member takes its head's type.
    let alias = schema
        .element_content_automaton(BLDG, "BuildingAlias")
        .expect("BuildingAlias");
    assert_eq!(alias.len(), 6);
    // Anonymous complex type.
    let annex = schema
        .element_content_automaton(BLDG, "Annex")
        .expect("Annex");
    assert_eq!(annex.len(), 2);
    assert_eq!(annex.position_of(Some(BLDG), "label"), Some(0));
    assert_eq!(annex.position_of(Some(URO), "buildingIDAttribute"), Some(1));
    // Unknown names.
    assert!(schema.element_content_automaton(BLDG, "Nope").is_none());
    assert!(schema.type_content_automaton(CORE, "Nope").is_none());
}

#[test]
fn validation_still_accepts_the_ordered_instance() {
    let schema = Arc::new(schema());
    let xml = r#"<bldg:Building xmlns:bldg="urn:bldg" xmlns:core="urn:core" xmlns:uro="urn:uro">
  <core:creationDate>2024-01-01</core:creationDate>
  <uro:cityObjectAttr>x</uro:cityObjectAttr>
  <bldg:class>1</bldg:class>
  <bldg:measuredHeight>10.5</bldg:measuredHeight>
  <uro:buildingIDAttribute>id</uro:buildingIDAttribute>
  <uro:class>ade</uro:class>
</bldg:Building>"#;
    let doc = fastxml::Parser::from(xml).parse().expect("parse");
    let dom = fastxml::schema::Validator::from(&doc)
        .schema(Arc::clone(&schema))
        .run()
        .expect("validate");
    assert!(dom.is_valid(), "{:?}", dom.into_entries());
    let streaming = fastxml::schema::Validator::from(xml)
        .schema(schema)
        .run()
        .expect("validate");
    assert!(streaming.is_valid(), "{:?}", streaming.into_entries());
}

#[test]
fn nested_group_ref_inside_a_group_from_another_document() {
    let b = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns="urn:b"
    targetNamespace="urn:b" elementFormDefault="qualified">
  <xs:group name="Inner"><xs:sequence><xs:element name="inner" type="xs:string"/></xs:sequence></xs:group>
  <xs:group name="Outer"><xs:sequence><xs:group ref="Inner"/></xs:sequence></xs:group>
</xs:schema>"#;
    let a = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:b="urn:b"
    targetNamespace="urn:a" elementFormDefault="qualified">
  <xs:import namespace="urn:b" schemaLocation="b.xsd"/>
  <xs:complexType name="UserType">
    <xs:sequence>
      <xs:group ref="b:Outer"/>
      <xs:element name="own" type="xs:string"/>
    </xs:sequence>
  </xs:complexType>
</xs:schema>"#;
    let schema = Schema::builder()
        .add("a.xsd", a.as_bytes().to_vec())
        .add("b.xsd", b.as_bytes().to_vec())
        .resolve()
        .expect("compile");
    let model = schema
        .type_content_automaton("urn:a", "UserType")
        .expect("UserType automaton");
    assert_eq!(model.position_of(Some("urn:b"), "inner"), Some(0));
    assert_eq!(model.position_of(Some("urn:a"), "own"), Some(1));
}

#[test]
fn chameleon_components_are_queried_in_no_namespace() {
    let chameleon = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" elementFormDefault="qualified">
  <xs:complexType name="CT"><xs:sequence><xs:element name="a" type="xs:string"/></xs:sequence></xs:complexType>
</xs:schema>"#;
    let main = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:m">
  <xs:include schemaLocation="chameleon.xsd"/>
</xs:schema>"#;
    let schema = Schema::builder()
        .add("main.xsd", main.as_bytes().to_vec())
        .add("chameleon.xsd", chameleon.as_bytes().to_vec())
        .resolve()
        .expect("compile");
    // No fallback that would hand out no-namespace positions for urn:m.
    assert!(schema.type_content_automaton("urn:m", "CT").is_none());
    let model = schema.type_content_automaton("", "CT").expect("CT");
    assert_eq!(model.position_of(None, "a"), Some(0));
}

#[test]
fn long_substitution_chain_inherits_the_head_type() {
    // A chain of untyped substitution members has no depth limit in XSD.
    let mut decls = String::from(
        r#"<xs:complexType name="HeadType"><xs:sequence><xs:element name="c" type="xs:string"/></xs:sequence></xs:complexType>
  <xs:element name="E0" type="t:HeadType"/>"#,
    );
    for i in 1..=24 {
        decls.push_str(&format!(
            r#"<xs:element name="E{i}" substitutionGroup="t:E{}"/>"#,
            i - 1
        ));
    }
    let xsd = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:t" targetNamespace="urn:t" elementFormDefault="qualified">{decls}</xs:schema>"#
    );
    let schema = Schema::from_xsd(xsd.as_bytes()).expect("compile");
    let model = schema
        .element_content_automaton("urn:t", "E24")
        .expect("E24 takes HeadType through the chain");
    assert_eq!(model.position_of(Some("urn:t"), "c"), Some(0));
}
