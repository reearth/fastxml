//! Tests for the reference-integrity pass (public-API level: every case
//! compiles a schema through `Schema::from_xsd`).

use crate::schema::Schema;

fn assert_dangling(xsd: &str, needle: &str) {
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("schema should be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("undeclared") || msg.contains("reference"),
        "expected dangling-reference error, got: {msg}"
    );
    assert!(
        msg.contains(needle),
        "error should mention '{needle}': {msg}"
    );
}

#[test]
fn dangling_group_ref_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:group ref="noSuchGroup"/>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchGroup");
}

#[test]
fn dangling_element_ref_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element ref="noSuchElement"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchElement");
}

#[test]
fn dangling_local_type_ref_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="child" type="NoSuchType"/>
                </xs:sequence>
            </xs:complexType>
        </xs:element>
    </xs:schema>"#;
    assert_dangling(xsd, "NoSuchType");
}

#[test]
fn top_level_dangling_type_tolerated() {
    // Lazy component resolution: a dangling type on a top-level element
    // is only an error when the element is used (saxonData/Missing).
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="unused" type="NoSuchType"/>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("lazy resolution tolerates unused dangling type");
}

#[test]
fn dangling_xsd_builtin_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root" type="xs:noSuchBuiltin"/>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchBuiltin");
}

#[test]
fn dangling_base_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="t">
            <xs:restriction base="NoSuchBase"/>
        </xs:simpleType>
    </xs:schema>"#;
    assert_dangling(xsd, "NoSuchBase");
}

#[test]
fn dangling_item_type_tolerated_but_xsd_ns_rejected() {
    // Lazy: a dangling itemType is tolerated (saxonData/Missing/missing006)…
    let lazy = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="t">
            <xs:list itemType="NoSuchItem"/>
        </xs:simpleType>
    </xs:schema>"#;
    Schema::from_xsd(lazy.as_bytes()).expect("lazy resolution tolerates dangling itemType");
    // …but a miss in the closed XSD namespace can never resolve.
    let impossible = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="t">
            <xs:list itemType="xs:noSuchItem"/>
        </xs:simpleType>
    </xs:schema>"#;
    assert_dangling(impossible, "noSuchItem");
}

#[test]
fn dangling_member_types_in_xsd_ns_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="t">
            <xs:union memberTypes="xs:string xs:timeDuration"/>
        </xs:simpleType>
    </xs:schema>"#;
    assert_dangling(xsd, "timeDuration");
}

#[test]
fn dangling_attribute_group_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:attributeGroup ref="noSuchAttrGroup"/>
        </xs:complexType>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchAttrGroup");
}

#[test]
fn dangling_attribute_ref_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:attribute ref="noSuchAttr"/>
        </xs:complexType>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchAttr");
}

#[test]
fn dangling_substitution_group_tolerated() {
    // Lazy resolution (saxonData/Missing/missing002): only an error when
    // the head is needed for validation.
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="e" substitutionGroup="noSuchHead" type="xs:string"/>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("lazy resolution tolerates dangling head");
}

#[test]
fn whitespace_in_qname_is_collapsed() {
    // QName attribute values are whitespace-collapsed (msData addB106).
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="t">
            <xs:restriction base="   xs:string "/>
        </xs:simpleType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("whitespace around QNames is not significant");
}

#[test]
fn dangling_keyref_refer_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root" type="xs:string">
            <xs:keyref name="kr" refer="noSuchKey">
                <xs:selector xpath="a"/>
                <xs:field xpath="@b"/>
            </xs:keyref>
        </xs:element>
    </xs:schema>"#;
    assert_dangling(xsd, "noSuchKey");
}

#[test]
fn keyref_field_count_mismatch_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
            <xs:key name="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
                <xs:field xpath="@y"/>
            </xs:key>
            <xs:keyref name="kr" refer="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:keyref>
        </xs:element>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("field counts must match");
    assert!(err.to_string().contains("field"), "got: {err}");
}

#[test]
fn keyref_referring_to_keyref_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
            <xs:key name="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:key>
            <xs:keyref name="kr1" refer="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:keyref>
            <xs:keyref name="kr2" refer="kr1">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:keyref>
        </xs:element>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("refer to keyref is invalid");
    assert!(err.to_string().contains("keyref"), "got: {err}");
}

#[test]
fn matching_keyref_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root">
            <xs:complexType>
                <xs:sequence>
                    <xs:element name="a" type="xs:string" maxOccurs="unbounded"/>
                </xs:sequence>
            </xs:complexType>
            <xs:key name="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:key>
            <xs:keyref name="kr" refer="k">
                <xs:selector xpath="a"/>
                <xs:field xpath="@x"/>
            </xs:keyref>
        </xs:element>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("valid keyref must compile");
}

#[test]
fn final_restriction_blocks_simple_restriction() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="base" final="restriction">
            <xs:restriction base="xs:string"/>
        </xs:simpleType>
        <xs:simpleType name="derived">
            <xs:restriction base="base"/>
        </xs:simpleType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("final restriction");
    assert!(err.to_string().contains("final"), "got: {err}");
}

#[test]
fn final_extension_blocks_complex_extension() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="base" final="extension">
            <xs:sequence/>
        </xs:complexType>
        <xs:complexType name="derived">
            <xs:complexContent>
                <xs:extension base="base">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("final extension");
    assert!(err.to_string().contains("final"), "got: {err}");
}

#[test]
fn final_all_blocks_list_and_union() {
    let list = r##"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="item" final="#all">
            <xs:restriction base="xs:string"/>
        </xs:simpleType>
        <xs:simpleType name="l">
            <xs:list itemType="item"/>
        </xs:simpleType>
    </xs:schema>"##;
    assert!(Schema::from_xsd(list.as_bytes()).is_err(), "final list");

    let union = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:simpleType name="member" final="union">
            <xs:restriction base="xs:string"/>
        </xs:simpleType>
        <xs:simpleType name="u">
            <xs:union memberTypes="member xs:int"/>
        </xs:simpleType>
    </xs:schema>"#;
    assert!(Schema::from_xsd(union.as_bytes()).is_err(), "final union");
}

#[test]
fn final_does_not_block_other_derivations() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="base" final="restriction">
            <xs:sequence/>
        </xs:complexType>
        <xs:complexType name="derived">
            <xs:complexContent>
                <xs:extension base="base">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
        <xs:simpleType name="s" final="list union">
            <xs:restriction base="xs:string"/>
        </xs:simpleType>
        <xs:simpleType name="s2">
            <xs:restriction base="s"/>
        </xs:simpleType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("unblocked derivations must compile");
}

#[test]
fn circular_type_derivation_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="a">
            <xs:complexContent>
                <xs:extension base="b">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
        <xs:complexType name="b">
            <xs:complexContent>
                <xs:extension base="a">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular derivation");
    assert!(err.to_string().contains("circular"), "got: {err}");
}

#[test]
fn recursive_content_is_not_a_derivation_cycle() {
    // An element of its own containing type is legal recursion.
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="node">
            <xs:sequence>
                <xs:element name="child" type="node" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("recursive content must compile");
}

#[test]
fn circular_attribute_group_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:attributeGroup name="a">
            <xs:attributeGroup ref="b"/>
        </xs:attributeGroup>
        <xs:attributeGroup name="b">
            <xs:attributeGroup ref="a"/>
        </xs:attributeGroup>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular attributeGroup");
    assert!(err.to_string().contains("circular"), "got: {err}");
}

#[test]
fn circular_group_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:group name="g1">
            <xs:sequence>
                <xs:group ref="g2"/>
            </xs:sequence>
        </xs:group>
        <xs:group name="g2">
            <xs:sequence>
                <xs:group ref="g1"/>
            </xs:sequence>
        </xs:group>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("circular group");
    assert!(err.to_string().contains("circular"), "got: {err}");
}

#[test]
fn acyclic_groups_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:group name="g1">
            <xs:sequence>
                <xs:group ref="g2"/>
            </xs:sequence>
        </xs:group>
        <xs:group name="g2">
            <xs:sequence>
                <xs:element name="e" type="xs:string"/>
            </xs:sequence>
        </xs:group>
        <xs:attributeGroup name="a">
            <xs:attributeGroup ref="b"/>
        </xs:attributeGroup>
        <xs:attributeGroup name="b">
            <xs:attribute name="x" type="xs:string"/>
        </xs:attributeGroup>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("acyclic groups must compile");
}

#[test]
fn attribute_with_complex_type_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="CT">
            <xs:sequence/>
        </xs:complexType>
        <xs:attribute name="a" type="CT"/>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("attribute type must be simple");
    assert!(err.to_string().contains("simple"), "got: {err}");
}

#[test]
fn simple_restriction_of_complex_type_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="CT">
            <xs:sequence/>
        </xs:complexType>
        <xs:simpleType name="st">
            <xs:restriction base="CT"/>
        </xs:simpleType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("simple restriction of complex");
    assert!(err.to_string().contains("simple"), "got: {err}");
}

#[test]
fn complex_content_base_must_be_complex() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="CT">
            <xs:complexContent>
                <xs:extension base="xs:string">
                    <xs:sequence/>
                </xs:extension>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("complexContent base simple");
    assert!(err.to_string().contains("complex"), "got: {err}");
}

#[test]
fn undeclared_prefix_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:element name="root" type="undeclared:Type"/>
    </xs:schema>"#;
    assert_dangling(xsd, "undeclared:Type");
}

#[test]
fn ref_into_unloaded_import_accepted() {
    // The import has no schemaLocation, so its namespace is poisoned and
    // references into it stay lenient (real-world partial schema sets).
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            xmlns:other="http://example.com/other">
        <xs:import namespace="http://example.com/other"/>
        <xs:element name="root" type="other:SomeType"/>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("lenient toward unresolved imports");
}

#[test]
fn ref_into_own_ns_with_include_accepted() {
    // xs:include is present but its document was never fetched (single-doc
    // compilation): the schema's own namespace is poisoned.
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:include schemaLocation="other.xsd"/>
        <xs:element name="root" type="TypeFromInclude"/>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("lenient when includes are unresolved");
}

#[test]
fn valid_same_ns_refs_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            xmlns:tns="http://example.com/t" targetNamespace="http://example.com/t">
        <xs:element name="root" type="tns:RootType"/>
        <xs:complexType name="RootType">
            <xs:sequence>
                <xs:element ref="tns:child"/>
                <xs:group ref="tns:g"/>
            </xs:sequence>
            <xs:attributeGroup ref="tns:ag"/>
        </xs:complexType>
        <xs:element name="child" type="xs:string"/>
        <xs:group name="g">
            <xs:sequence>
                <xs:element name="x" type="xs:string"/>
            </xs:sequence>
        </xs:group>
        <xs:attributeGroup name="ag">
            <xs:attribute name="a" type="xs:string"/>
        </xs:attributeGroup>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("valid schema must compile");
}

#[test]
fn unprefixed_ref_falls_back_to_target_namespace() {
    // Mirrors the compiler's leniency: unprefixed references resolve
    // against the target namespace even without a default xmlns.
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://example.com/t">
        <xs:element name="root" type="RootType"/>
        <xs:complexType name="RootType">
            <xs:sequence/>
        </xs:complexType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("unprefixed same-ns ref must compile");
}

#[test]
fn xml_namespace_refs_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
        <xs:complexType name="t">
            <xs:attribute ref="xml:lang"/>
        </xs:complexType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("xml namespace is always lenient");
}
