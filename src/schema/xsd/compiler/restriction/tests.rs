use crate::schema::Schema;

#[test]
fn widening_element_type_rejected() {
    // Derived c1 uses xs:string, which is not derived from the base's
    // restricted string type (particlesIk shape).
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:simpleType name="rString">
            <xs:restriction base="xs:string">
                <xs:maxLength value="4"/>
            </xs:restriction>
        </xs:simpleType>
        <xs:complexType name="B">
            <xs:choice>
                <xs:element name="c1" type="x:rString"/>
            </xs:choice>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:choice>
                        <xs:element name="c1" type="xs:string"/>
                    </xs:choice>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("widening type");
    assert!(err.to_string().contains("restriction"), "got: {err}");
}

#[test]
fn anonymous_restrictions_are_checked() {
    // The restriction is the anonymous type of a global element, and of
    // a local element nested in another anonymous type.
    let base = r#"<xs:complexType name="B">
            <xs:sequence>
                <xs:element name="e1" maxOccurs="2"/>
            </xs:sequence>
        </xs:complexType>"#;
    let restriction = |max: &str| {
        format!(
            r#"<xs:complexType><xs:complexContent><xs:restriction base="x:B">
               <xs:sequence><xs:element name="e1" maxOccurs="{max}"/></xs:sequence>
               </xs:restriction></xs:complexContent></xs:complexType>"#
        )
    };
    for (max, ok) in [("2", true), ("3", false)] {
        for element in [
            format!(
                r#"<xs:element name="doc">{}</xs:element>"#,
                restriction(max)
            ),
            format!(
                r#"<xs:element name="doc"><xs:complexType><xs:sequence>
                   <xs:element name="inner">{}</xs:element>
                   </xs:sequence></xs:complexType></xs:element>"#,
                restriction(max)
            ),
        ] {
            let xsd = format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                    targetNamespace="http://t" xmlns:x="http://t">{base}{element}</xs:schema>"#
            );
            let result = Schema::from_xsd(xsd.as_bytes());
            assert_eq!(
                result.is_ok(),
                ok,
                "maxOccurs {max}: {element}: {:?}",
                result.err()
            );
        }
    }
}

/// Compiles a schema with complex type `B` holding `base` and `R`
/// restricting it with `derived`.
fn restrict_result(base: &str, derived: &str) -> crate::error::Result<Schema> {
    let xsd = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t" elementFormDefault="qualified">
        <xs:element name="g"/>
        <xs:simpleType name="S"><xs:restriction base="xs:string"/></xs:simpleType>
        <xs:complexType name="C"/>
        <xs:complexType name="B">{base}</xs:complexType>
        <xs:complexType name="R"><xs:complexContent><xs:restriction base="x:B">
            {derived}
        </xs:restriction></xs:complexContent></xs:complexType>
        </xs:schema>"#
    );
    Schema::from_xsd(xsd.as_bytes())
}

#[test]
fn rcase_rules() {
    let any = |attrs: &str| format!("<xs:sequence><xs:any {attrs}/></xs:sequence>");
    let seq = |body: &str| format!("<xs:sequence>{body}</xs:sequence>");
    let choice = |body: &str| format!("<xs:choice>{body}</xs:choice>");
    let all = |body: &str| format!("<xs:all>{body}</xs:all>");
    let e = |attrs: &str| format!(r#"<xs:element name="e" {attrs}/>"#);
    let cases: &[(&str, String, String, bool)] = &[
        // NameAndTypeOK: fixed values
        (
            "same fixed",
            seq(&e(r#"fixed="a""#)),
            seq(&e(r#"fixed="a""#)),
            true,
        ),
        (
            "other fixed",
            seq(&e(r#"fixed="a""#)),
            seq(&e(r#"fixed="b""#)),
            false,
        ),
        ("dropped fixed", seq(&e(r#"fixed="a""#)), seq(&e("")), false),
        (
            "same fixed int value",
            seq(&e(r#"type="xs:int" fixed="1""#)),
            seq(&e(r#"type="xs:int" fixed="01""#)),
            true,
        ),
        (
            "other fixed of a user string type",
            seq(&e(r#"type="x:S" fixed="a""#)),
            seq(&e(r#"type="x:S" fixed="b""#)),
            false,
        ),
        // NameAndTypeOK: block
        (
            "same block",
            seq(&e(r##"block="#all""##)),
            seq(&e(r##"block="#all""##)),
            true,
        ),
        (
            "wider block",
            seq(&e(r#"block="extension""#)),
            seq(&e(r#"block="extension restriction""#)),
            true,
        ),
        (
            "narrower block",
            seq(&e(r#"block="extension restriction""#)),
            seq(&e(r#"block="extension""#)),
            false,
        ),
        // NameAndTypeOK: namespaces and types
        (
            "ref to another namespace",
            seq(r#"<xs:element name="g" form="unqualified"/>"#),
            seq(r#"<xs:element ref="x:g"/>"#),
            false,
        ),
        (
            "complex type not derived",
            seq(&e(r#"type="x:C""#)),
            seq(&e(r#"type="x:B""#)),
            false,
        ),
        (
            "complex type itself",
            seq(&e(r#"type="x:C""#)),
            seq(&e(r#"type="x:C""#)),
            true,
        ),
        // NSCompat / NSRecurseCheckCardinality
        (
            "element in ##other",
            any(r###"namespace="##other""###),
            seq(&e("")),
            false,
        ),
        (
            "element in ##targetNamespace",
            any(r###"namespace="##targetNamespace""###),
            seq(&e("")),
            true,
        ),
        (
            "group within the wildcard's count",
            any(r#"maxOccurs="2""#),
            seq(&format!("{}{}", e(""), r#"<xs:element name="f"/>"#)),
            true,
        ),
        (
            "group above the wildcard's count",
            any(""),
            seq(&format!("{}{}", e(""), r#"<xs:element name="f"/>"#)),
            false,
        ),
        // NSSubset
        (
            "weaker processContents",
            any(r#"processContents="strict""#),
            any(r#"processContents="lax""#),
            false,
        ),
        (
            "##any under a list",
            any(r#"namespace="http://t""#),
            any(""),
            false,
        ),
        (
            "list within ##other",
            any(r###"namespace="##other""###),
            any(r#"namespace="urn:o""#),
            true,
        ),
        // Forbidden pairs
        (
            "wildcard for a group",
            seq(&format!("{}{}", e(""), r#"<xs:element name="f"/>"#)),
            any(""),
            false,
        ),
        (
            "choice for an all-group",
            all(r#"<xs:element name="e" minOccurs="0"/><xs:element name="f" minOccurs="0"/>"#),
            choice(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            false,
        ),
        // MapAndSum
        (
            "sequence within a choice",
            r#"<xs:choice maxOccurs="2"><xs:element name="e"/><xs:element name="f"/></xs:choice>"#
                .to_string(),
            seq(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            true,
        ),
        (
            "sequence too long for a choice",
            choice(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            seq(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            false,
        ),
        // RecurseUnordered
        (
            "sequence within an all-group",
            all(
                r#"<xs:element name="e"/><xs:element name="f"/><xs:element name="g" minOccurs="0"/>"#,
            ),
            seq(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            true,
        ),
        (
            "sequence missing a required member",
            all(r#"<xs:element name="e"/><xs:element name="f"/><xs:element name="g"/>"#),
            seq(r#"<xs:element name="e"/><xs:element name="f"/>"#),
            false,
        ),
    ];
    for (what, base, derived, ok) in cases {
        let result = restrict_result(base, derived);
        assert_eq!(result.is_ok(), *ok, "{what}: {:?}", result.err());
    }
}

#[test]
fn narrowing_element_type_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:simpleType name="rString">
            <xs:restriction base="xs:string">
                <xs:maxLength value="4"/>
            </xs:restriction>
        </xs:simpleType>
        <xs:complexType name="B">
            <xs:choice>
                <xs:element name="c1" type="xs:string"/>
            </xs:choice>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:choice>
                        <xs:element name="c1" type="x:rString"/>
                    </xs:choice>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("narrowing restriction must compile");
}

#[test]
fn occurrence_widening_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:element name="e" type="xs:string" maxOccurs="2"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:element name="e" type="xs:string" maxOccurs="5"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("occurrence widening");
    assert!(err.to_string().contains("restriction"), "got: {err}");
}

#[test]
fn dropping_required_base_element_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:element name="a" type="xs:string"/>
                <xs:element name="b" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:element name="a" type="xs:string"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("dropping required element");
    assert!(err.to_string().contains("restriction"), "got: {err}");
}

#[test]
fn dropping_optional_base_element_accepted() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:element name="a" type="xs:string"/>
                <xs:element name="b" type="xs:string" minOccurs="0"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:element name="a" type="xs:string"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    Schema::from_xsd(xsd.as_bytes()).expect("dropping optional content must compile");
}

#[test]
fn extra_derived_content_rejected() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:element name="a" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:element name="a" type="xs:string"/>
                        <xs:element name="zz" type="xs:string"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"#;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("extra content");
    assert!(err.to_string().contains("restriction"), "got: {err}");
}

#[test]
fn wildcard_restricting_element_rejected() {
    let xsd = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:element name="a" type="xs:string"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:any namespace="##any"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"###;
    let err = Schema::from_xsd(xsd.as_bytes()).expect_err("wildcard vs element");
    assert!(err.to_string().contains("restriction"), "got: {err}");
}

#[test]
fn element_within_wildcard_accepted() {
    let xsd = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
            targetNamespace="http://t" xmlns:x="http://t">
        <xs:complexType name="B">
            <xs:sequence>
                <xs:any namespace="##any"/>
            </xs:sequence>
        </xs:complexType>
        <xs:complexType name="R">
            <xs:complexContent>
                <xs:restriction base="x:B">
                    <xs:sequence>
                        <xs:element name="a" type="xs:string"/>
                    </xs:sequence>
                </xs:restriction>
            </xs:complexContent>
        </xs:complexType>
    </xs:schema>"###;
    Schema::from_xsd(xsd.as_bytes()).expect("element within wildcard must compile");
}
