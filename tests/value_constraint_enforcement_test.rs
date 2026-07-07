//! Element `fixed`/`default` value-constraint enforcement, and cross-type
//! identity-constraint key comparison.
//!
//! Semantics under test (XSD 1.0 §3.3.4 Element Locally Valid):
//! - An element with `fixed="V"` and non-empty content is valid only when the
//!   content equals `V` in the *value space* of the element's type (e.g.
//!   fixed `1.0` matches content `1.00` for decimal).
//! - An empty element with a `default`/`fixed` value constraint takes that
//!   value as its schema-normalized content; the value must itself satisfy the
//!   type (so a value violating a narrowing `xsi:type` is rejected).
//! - A `default` (unlike `fixed`) never constrains present content.
//!
//! Every rule is pinned on BOTH the DOM and streaming (OnePass) engines.

mod common;

use common::{validate_dom, validate_onepass};

fn assert_both_valid(xml: &str, xsd: &str, ctx: &str) {
    let dom = validate_dom(xml, xsd);
    let onepass = validate_onepass(xml, xsd);
    assert!(
        dom.is_valid(),
        "DOM: expected valid ({ctx}), errors: {:?}",
        dom.errors()
    );
    assert!(
        onepass.is_valid(),
        "streaming: expected valid ({ctx}), errors: {:?}",
        onepass.errors()
    );
}

fn assert_both_invalid(xml: &str, xsd: &str, ctx: &str) {
    let dom = validate_dom(xml, xsd);
    let onepass = validate_onepass(xml, xsd);
    assert!(!dom.is_valid(), "DOM: expected invalid ({ctx})");
    assert!(!onepass.is_valid(), "streaming: expected invalid ({ctx})");
}

// ---------------------------------------------------------------------------
// Element fixed value: match / mismatch in the value space
// ---------------------------------------------------------------------------

const FIXED_STRING_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="foo" type="xs:string" fixed="fixed"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

#[test]
fn element_fixed_string_match() {
    assert_both_valid(
        "<root><foo>fixed</foo></root>",
        FIXED_STRING_XSD,
        "string fixed matches",
    );
}

#[test]
fn element_fixed_string_mismatch() {
    assert_both_invalid(
        "<root><foo>other</foo></root>",
        FIXED_STRING_XSD,
        "string fixed mismatch",
    );
}

const FIXED_DECIMAL_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:decimal" fixed="1.0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

#[test]
fn element_fixed_decimal_matches_in_value_space() {
    // fixed="1.0" must accept "1.00" and "01" — equal decimals — but reject "2".
    assert_both_valid(
        "<root><n>1.00</n></root>",
        FIXED_DECIMAL_XSD,
        "decimal 1.00 == fixed 1.0",
    );
    assert_both_valid(
        "<root><n>01</n></root>",
        FIXED_DECIMAL_XSD,
        "decimal 01 == fixed 1.0",
    );
    assert_both_invalid(
        "<root><n>2</n></root>",
        FIXED_DECIMAL_XSD,
        "decimal 2 != fixed 1.0",
    );
}

/// A fixed constraint on an untyped (anyType) element compares lexically and
/// must still be enforced — this element never reaches the type-driven text
/// path, so it exercises the type-independent fixed check.
#[test]
fn element_fixed_untyped_mismatch() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="a" fixed="fixed_value"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    assert_both_invalid(
        "<root><a>not fixed</a></root>",
        xsd,
        "anyType fixed mismatch",
    );
    assert_both_valid(
        "<root><a>fixed_value</a></root>",
        xsd,
        "anyType fixed match",
    );
}

// ---------------------------------------------------------------------------
// Empty element + value constraint: the constraint value is the content
// ---------------------------------------------------------------------------

const EMPTY_FIXED_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:integer" fixed="123"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;

#[test]
fn empty_element_with_fixed_is_valid() {
    // Empty <n/> takes fixed "123" (a valid integer) as its content.
    assert_both_valid("<root><n/></root>", EMPTY_FIXED_XSD, "empty + fixed int");
    assert_both_valid(
        "<root><n></n></root>",
        EMPTY_FIXED_XSD,
        "empty tags + fixed int",
    );
}

#[test]
fn empty_element_with_default_is_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:integer" default="1"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    assert_both_valid("<root><n/></root>", xsd, "empty + default int");
}

/// A present (non-empty) `default` value never constrains content — even a
/// value that differs from the default is fine.
#[test]
fn default_does_not_constrain_present_content() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:integer" default="1"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    assert_both_valid("<root><n>999</n></root>", xsd, "default + other content");
}

/// An empty element whose value constraint violates a narrowing `xsi:type`
/// must be rejected: the constraint value is validated against the type.
#[test]
fn empty_element_value_constraint_checked_against_xsi_type() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns="vc" xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="vc">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Element" type="Float" fixed="-1"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
  <xs:simpleType name="Float">
    <xs:restriction base="xs:float"><xs:maxInclusive value="0"/></xs:restriction>
  </xs:simpleType>
  <xs:simpleType name="derivedType">
    <xs:restriction base="Float"><xs:minInclusive value="0"/></xs:restriction>
  </xs:simpleType>
</xs:schema>"#;
    // fixed="-1" is valid for Float but the element's xsi:type narrows to
    // derivedType (>= 0), so the effective value -1 is invalid.
    let xml = r#"<t:root xmlns:t="vc" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Element xsi:type="t:derivedType"/>
</t:root>"#;
    assert_both_invalid(xml, xsd, "empty fixed -1 violates xsi:type derivedType");
}

// ---------------------------------------------------------------------------
// nil interaction: a nilled empty element is not checked against fixed
// ---------------------------------------------------------------------------

#[test]
fn nilled_element_with_fixed_is_not_rejected() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="n" type="xs:integer" nillable="true" fixed="123"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    // A nilled element contributes no value, so the fixed constraint is not
    // applied to its (empty) content.
    let xml = r#"<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <n xsi:nil="true"/>
</root>"#;
    assert_both_valid(xml, xsd, "nilled + fixed");
}

// ---------------------------------------------------------------------------
// Same-named positional elements with different fixed values
// ---------------------------------------------------------------------------

/// A sequence declaring the same element name twice with different `fixed`
/// values: the constraint is positional — the Nth occurrence is checked
/// against the Nth declaration.
#[test]
fn same_name_elements_distinct_fixed_are_positional() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="ct">
    <xs:sequence>
      <xs:element name="op" type="xs:string" fixed="delete"/>
      <xs:element name="op" type="xs:string" fixed="write"/>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="root" type="ct"/>
</xs:schema>"#;
    // In declaration order: valid.
    assert_both_valid(
        "<root><op>delete</op><op>write</op></root>",
        xsd,
        "positional fixed in order",
    );
    // Second occurrence violates the second declaration's fixed: invalid.
    assert_both_invalid(
        "<root><op>delete</op><op>delete</op></root>",
        xsd,
        "positional fixed second occurrence mismatch",
    );
}

/// A base type's `default` shadowed by an extension's `fixed` on the same
/// name: occurrence 1 gets the base default (no constraint on content),
/// occurrence 2 gets the extension's fixed.
#[test]
fn extension_same_name_fixed_is_positional() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="base">
    <xs:sequence>
      <xs:element name="el" type="xs:string" default="foo"/>
    </xs:sequence>
  </xs:complexType>
  <xs:complexType name="derived">
    <xs:complexContent>
      <xs:extension base="base">
        <xs:sequence>
          <xs:element name="el" type="xs:string" fixed="goo"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:element name="root" type="derived"/>
</xs:schema>"#;
    assert_both_valid(
        "<root><el>anything</el><el>goo</el></root>",
        xsd,
        "base default + extension fixed satisfied",
    );
    assert_both_invalid(
        "<root><el>anything</el><el>not-goo</el></root>",
        xsd,
        "extension fixed violated on second occurrence",
    );
}
