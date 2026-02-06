//! Tests for XSD AST types.

use super::*;

// =============================================================================
// QName Tests
// =============================================================================

#[test]
fn test_qname_new() {
    let qn = QName::new("localName");
    assert_eq!(qn.prefix, None);
    assert_eq!(qn.local, "localName");
}

#[test]
fn test_qname_with_prefix() {
    let qn = QName::with_prefix("xs", "string");
    assert_eq!(qn.prefix, Some("xs".to_string()));
    assert_eq!(qn.local, "string");
}

#[test]
fn test_qname_parse() {
    let qn = QName::parse("xs:string");
    assert_eq!(qn.prefix, Some("xs".to_string()));
    assert_eq!(qn.local, "string");

    let qn2 = QName::parse("localName");
    assert_eq!(qn2.prefix, None);
    assert_eq!(qn2.local, "localName");
}

#[test]
fn test_qname_to_string_full() {
    let qn = QName::with_prefix("xs", "string");
    assert_eq!(qn.to_string_full(), "xs:string");

    let qn2 = QName::new("localName");
    assert_eq!(qn2.to_string_full(), "localName");
}

#[test]
fn test_qname_display() {
    let qn = QName::with_prefix("xs", "string");
    assert_eq!(format!("{}", qn), "xs:string");

    let qn2 = QName::new("localName");
    assert_eq!(format!("{}", qn2), "localName");
}

// =============================================================================
// Occurs Tests
// =============================================================================

#[test]
fn test_occurs_parse() {
    assert_eq!(Occurs::parse("0"), Ok(Occurs::Count(0)));
    assert_eq!(Occurs::parse("1"), Ok(Occurs::Count(1)));
    assert_eq!(Occurs::parse("unbounded"), Ok(Occurs::Unbounded));

    // Test error cases
    assert!(Occurs::parse("-1").is_err());
    assert!(Occurs::parse("invalid").is_err());
}

#[test]
fn test_occurs_default() {
    let occurs = Occurs::default();
    assert_eq!(occurs, Occurs::Count(1));
}

#[test]
fn test_occurs_is_unbounded() {
    assert!(Occurs::Unbounded.is_unbounded());
    assert!(!Occurs::Count(1).is_unbounded());
    assert!(!Occurs::Count(0).is_unbounded());
}

#[test]
fn test_occurs_to_option() {
    assert_eq!(Occurs::Count(5).to_option(), Some(5));
    assert_eq!(Occurs::Count(0).to_option(), Some(0));
    assert_eq!(Occurs::Unbounded.to_option(), None);
}

// =============================================================================
// XsdSchema Tests
// =============================================================================

#[test]
fn test_xsd_schema_default() {
    let schema = XsdSchema::new();
    assert!(schema.target_namespace.is_none());
    assert!(schema.elements.is_empty());
    assert!(schema.types.is_empty());
    assert!(schema.imports.is_empty());
    assert!(schema.includes.is_empty());
    assert!(schema.attributes.is_empty());
    assert_eq!(schema.element_form_default, FormDefault::Unqualified);
}

#[test]
fn test_xsd_schema_with_target_namespace() {
    let schema = XsdSchema::new().with_target_namespace("http://example.com");
    assert_eq!(
        schema.target_namespace,
        Some("http://example.com".to_string())
    );
}

// =============================================================================
// FormDefault Tests
// =============================================================================

#[test]
fn test_form_default() {
    assert_eq!(FormDefault::default(), FormDefault::Unqualified);
    assert_ne!(FormDefault::Qualified, FormDefault::Unqualified);
}

// =============================================================================
// XsdRedefine Tests
// =============================================================================

#[test]
fn test_xsd_redefine_new() {
    let redefine = XsdRedefine::new("http://example.com/schema.xsd");
    assert_eq!(redefine.schema_location, "http://example.com/schema.xsd");
    assert!(redefine.simple_types.is_empty());
    assert!(redefine.complex_types.is_empty());
    assert!(redefine.groups.is_empty());
    assert!(redefine.attribute_groups.is_empty());
}

// =============================================================================
// XsdIdentityConstraint Tests
// =============================================================================

#[test]
fn test_xsd_identity_constraint_unique() {
    let constraint = XsdIdentityConstraint::unique("myUnique", "./item");
    assert_eq!(constraint.name, "myUnique");
    assert_eq!(constraint.constraint_type, XsdConstraintType::Unique);
    assert_eq!(constraint.selector, "./item");
    assert!(constraint.fields.is_empty());
    assert!(constraint.refer.is_none());
}

#[test]
fn test_xsd_identity_constraint_key() {
    let constraint = XsdIdentityConstraint::key("myKey", "./item");
    assert_eq!(constraint.name, "myKey");
    assert_eq!(constraint.constraint_type, XsdConstraintType::Key);
    assert_eq!(constraint.selector, "./item");
}

#[test]
fn test_xsd_identity_constraint_keyref() {
    let refer = QName::new("myKey");
    let constraint = XsdIdentityConstraint::keyref("myKeyRef", "./item", refer);
    assert_eq!(constraint.name, "myKeyRef");
    assert_eq!(constraint.constraint_type, XsdConstraintType::KeyRef);
    assert!(constraint.refer.is_some());
    assert_eq!(constraint.refer.unwrap().local, "myKey");
}

#[test]
fn test_xsd_identity_constraint_with_field() {
    let constraint = XsdIdentityConstraint::unique("myUnique", "./item").with_field("@id");
    assert_eq!(constraint.fields, vec!["@id"]);
}

#[test]
fn test_xsd_identity_constraint_with_fields() {
    let constraint = XsdIdentityConstraint::key("myKey", "./item").with_fields(["@id", "@name"]);
    assert_eq!(constraint.fields, vec!["@id", "@name"]);
}

// =============================================================================
// XsdElement Tests
// =============================================================================

#[test]
fn test_xsd_element_new() {
    let elem = XsdElement::new("myElement");
    assert_eq!(elem.name, "myElement");
    assert!(elem.type_ref.is_none());
    assert!(elem.inline_type.is_none());
    assert!(elem.ref_.is_none());
    assert_eq!(elem.min_occurs, Occurs::Count(1));
    assert_eq!(elem.max_occurs, Occurs::Count(1));
    assert!(!elem.is_abstract);
    assert!(!elem.nillable);
}

#[test]
fn test_xsd_element_ref() {
    let elem = XsdElement::ref_(QName::new("otherElement"));
    assert_eq!(elem.name, "");
    assert!(elem.ref_.is_some());
    assert_eq!(elem.ref_.unwrap().local, "otherElement");
}

// =============================================================================
// XsdTypeDef Tests
// =============================================================================

#[test]
fn test_xsd_type_def_name_simple() {
    let simple = XsdSimpleType::new("myType");
    let type_def = XsdTypeDef::Simple(simple);
    assert_eq!(type_def.name(), Some("myType"));
}

#[test]
fn test_xsd_type_def_name_complex() {
    let complex = XsdComplexType::new("myComplex");
    let type_def = XsdTypeDef::Complex(complex);
    assert_eq!(type_def.name(), Some("myComplex"));
}

#[test]
fn test_xsd_type_def_name_anonymous() {
    let simple = XsdSimpleType::anonymous();
    let type_def = XsdTypeDef::Simple(simple);
    assert_eq!(type_def.name(), None);
}

#[test]
fn test_xsd_type_def_is_simple() {
    let simple = XsdTypeDef::Simple(XsdSimpleType::new("t"));
    assert!(simple.is_simple());
    assert!(!simple.is_complex());
}

#[test]
fn test_xsd_type_def_is_complex() {
    let complex = XsdTypeDef::Complex(XsdComplexType::new("t"));
    assert!(complex.is_complex());
    assert!(!complex.is_simple());
}

// =============================================================================
// XsdSimpleType Tests
// =============================================================================

#[test]
fn test_xsd_simple_type_new() {
    let simple = XsdSimpleType::new("mySimple");
    assert_eq!(simple.name, Some("mySimple".to_string()));
}

#[test]
fn test_xsd_simple_type_anonymous() {
    let simple = XsdSimpleType::anonymous();
    assert_eq!(simple.name, None);
}

// =============================================================================
// XsdComplexType Tests
// =============================================================================

#[test]
fn test_xsd_complex_type_new() {
    let complex = XsdComplexType::new("myComplex");
    assert_eq!(complex.name, Some("myComplex".to_string()));
    assert!(!complex.is_abstract);
    assert!(!complex.mixed);
    assert!(complex.attributes.is_empty());
}

#[test]
fn test_xsd_complex_type_anonymous() {
    let complex = XsdComplexType::anonymous();
    assert_eq!(complex.name, None);
}

// =============================================================================
// XsdAny Tests
// =============================================================================

#[test]
fn test_xsd_any_default() {
    let any = XsdAny::default();
    assert_eq!(any.min_occurs, Occurs::Count(1));
    assert_eq!(any.max_occurs, Occurs::Count(1));
    assert!(matches!(any.namespace, NamespaceConstraint::Any));
    assert_eq!(any.process_contents, ProcessContentsMode::Strict);
}

// =============================================================================
// ProcessContentsMode Tests
// =============================================================================

#[test]
fn test_process_contents_mode_default() {
    assert_eq!(ProcessContentsMode::default(), ProcessContentsMode::Strict);
}

// =============================================================================
// XsdAttribute Tests
// =============================================================================

#[test]
fn test_xsd_attribute_new() {
    let attr = XsdAttribute::new("myAttr");
    assert_eq!(attr.name, Some("myAttr".to_string()));
    assert!(attr.type_ref.is_none());
    assert!(attr.ref_.is_none());
    assert_eq!(attr.use_, AttributeUse::Optional);
}

#[test]
fn test_xsd_attribute_ref() {
    let attr = XsdAttribute::ref_(QName::new("otherAttr"));
    assert_eq!(attr.name, None);
    assert!(attr.ref_.is_some());
    assert_eq!(attr.ref_.unwrap().local, "otherAttr");
}

// =============================================================================
// AttributeUse Tests
// =============================================================================

#[test]
fn test_attribute_use_default() {
    assert_eq!(AttributeUse::default(), AttributeUse::Optional);
}

// =============================================================================
// XsdAttributeGroup Tests
// =============================================================================

#[test]
fn test_xsd_attribute_group_new() {
    let group = XsdAttributeGroup::new("myGroup");
    assert_eq!(group.name, Some("myGroup".to_string()));
    assert!(group.ref_.is_none());
    assert!(group.attributes.is_empty());
}

#[test]
fn test_xsd_attribute_group_ref() {
    let group = XsdAttributeGroup::ref_(QName::new("otherGroup"));
    assert_eq!(group.name, None);
    assert!(group.ref_.is_some());
    assert_eq!(group.ref_.unwrap().local, "otherGroup");
}

// =============================================================================
// XsdGroup Tests
// =============================================================================

#[test]
fn test_xsd_group_new() {
    let group = XsdGroup::new("myGroup");
    assert_eq!(group.name, Some("myGroup".to_string()));
    assert!(group.ref_.is_none());
    assert!(group.particle.is_none());
    assert_eq!(group.min_occurs, Occurs::Count(1));
    assert_eq!(group.max_occurs, Occurs::Count(1));
}

#[test]
fn test_xsd_group_ref() {
    let group = XsdGroup::ref_(QName::new("otherGroup"));
    assert_eq!(group.name, None);
    assert!(group.ref_.is_some());
    assert_eq!(group.ref_.unwrap().local, "otherGroup");
}

// =============================================================================
// XsdSequence/XsdChoice/XsdAll Tests
// =============================================================================

#[test]
fn test_xsd_sequence_default() {
    let seq = XsdSequence::default();
    assert!(seq.particles.is_empty());
}

#[test]
fn test_xsd_choice_default() {
    let choice = XsdChoice::default();
    assert!(choice.particles.is_empty());
}

#[test]
fn test_xsd_all_default() {
    let all = XsdAll::default();
    assert!(all.elements.is_empty());
}

// =============================================================================
// XsdSimpleRestriction Tests
// =============================================================================

#[test]
fn test_xsd_simple_restriction_default() {
    let restriction = XsdSimpleRestriction::default();
    assert!(restriction.base.is_none());
    assert!(restriction.inline_base.is_none());
    assert!(restriction.facets.is_empty());
}

// =============================================================================
// NamespaceConstraint Tests
// =============================================================================

#[test]
fn test_namespace_constraint_variants() {
    let any = NamespaceConstraint::Any;
    let other = NamespaceConstraint::Other;
    let list = NamespaceConstraint::List(vec!["http://example.com".to_string()]);
    let target = NamespaceConstraint::TargetNamespace;
    let local = NamespaceConstraint::Local;

    // Just test that all variants exist and can be created
    assert!(matches!(any, NamespaceConstraint::Any));
    assert!(matches!(other, NamespaceConstraint::Other));
    assert!(matches!(list, NamespaceConstraint::List(_)));
    assert!(matches!(target, NamespaceConstraint::TargetNamespace));
    assert!(matches!(local, NamespaceConstraint::Local));
}

// =============================================================================
// XsdConstraintType Tests
// =============================================================================

#[test]
fn test_xsd_constraint_type_variants() {
    assert_eq!(XsdConstraintType::Unique, XsdConstraintType::Unique);
    assert_eq!(XsdConstraintType::Key, XsdConstraintType::Key);
    assert_eq!(XsdConstraintType::KeyRef, XsdConstraintType::KeyRef);
    assert_ne!(XsdConstraintType::Unique, XsdConstraintType::Key);
}

// =============================================================================
// DerivationControl/DerivationType Tests
// =============================================================================

#[test]
fn test_derivation_control_all() {
    let control = DerivationControl::All;
    assert!(matches!(control, DerivationControl::All));
}

#[test]
fn test_derivation_control_list() {
    let control =
        DerivationControl::List(vec![DerivationType::Extension, DerivationType::Restriction]);
    if let DerivationControl::List(types) = control {
        assert_eq!(types.len(), 2);
        assert!(types.contains(&DerivationType::Extension));
        assert!(types.contains(&DerivationType::Restriction));
    } else {
        panic!("Expected List variant");
    }
}

#[test]
fn test_derivation_type_variants() {
    assert_eq!(DerivationType::Extension, DerivationType::Extension);
    assert_eq!(DerivationType::Restriction, DerivationType::Restriction);
    assert_eq!(DerivationType::Substitution, DerivationType::Substitution);
    assert_ne!(DerivationType::Extension, DerivationType::Restriction);
}

// =============================================================================
// WhiteSpaceValue Tests
// =============================================================================

#[test]
fn test_whitespace_value_variants() {
    assert_eq!(WhiteSpaceValue::Preserve, WhiteSpaceValue::Preserve);
    assert_eq!(WhiteSpaceValue::Replace, WhiteSpaceValue::Replace);
    assert_eq!(WhiteSpaceValue::Collapse, WhiteSpaceValue::Collapse);
    assert_ne!(WhiteSpaceValue::Preserve, WhiteSpaceValue::Collapse);
}

// =============================================================================
// XsdFacet Tests
// =============================================================================

#[test]
fn test_xsd_facet_variants() {
    let facets = vec![
        XsdFacet::Enumeration("value".to_string()),
        XsdFacet::Pattern("[0-9]+".to_string()),
        XsdFacet::MinLength(1),
        XsdFacet::MaxLength(100),
        XsdFacet::Length(10),
        XsdFacet::MinInclusive("0".to_string()),
        XsdFacet::MaxInclusive("100".to_string()),
        XsdFacet::MinExclusive("0".to_string()),
        XsdFacet::MaxExclusive("100".to_string()),
        XsdFacet::TotalDigits(10),
        XsdFacet::FractionDigits(2),
        XsdFacet::WhiteSpace(WhiteSpaceValue::Collapse),
    ];
    assert_eq!(facets.len(), 12);
}

// =============================================================================
// XsdSimpleTypeContent Tests
// =============================================================================

#[test]
fn test_xsd_simple_type_content_restriction() {
    let content = XsdSimpleTypeContent::Restriction(XsdSimpleRestriction::default());
    assert!(matches!(content, XsdSimpleTypeContent::Restriction(_)));
}

#[test]
fn test_xsd_simple_type_content_list() {
    let content = XsdSimpleTypeContent::List(XsdSimpleList {
        item_type: Some(QName::new("string")),
        inline_type: None,
    });
    assert!(matches!(content, XsdSimpleTypeContent::List(_)));
}

#[test]
fn test_xsd_simple_type_content_union() {
    let content = XsdSimpleTypeContent::Union(XsdSimpleUnion {
        member_types: vec![QName::new("string"), QName::new("int")],
        inline_types: Vec::new(),
    });
    assert!(matches!(content, XsdSimpleTypeContent::Union(_)));
}

// =============================================================================
// XsdComplexContent Tests
// =============================================================================

#[test]
fn test_xsd_complex_content_empty() {
    let content = XsdComplexContent::Empty;
    assert!(matches!(content, XsdComplexContent::Empty));
}

#[test]
fn test_xsd_complex_content_particle() {
    let content = XsdComplexContent::Particle(XsdParticle::Sequence(XsdSequence::default()));
    assert!(matches!(content, XsdComplexContent::Particle(_)));
}

// =============================================================================
// XsdParticle Tests
// =============================================================================

#[test]
fn test_xsd_particle_variants() {
    let seq = XsdParticle::Sequence(XsdSequence::default());
    let choice = XsdParticle::Choice(XsdChoice::default());
    let all = XsdParticle::All(XsdAll::default());
    let group_ref = XsdParticle::GroupRef(QName::new("myGroup"));
    let any = XsdParticle::Any(XsdAny::default());

    assert!(matches!(seq, XsdParticle::Sequence(_)));
    assert!(matches!(choice, XsdParticle::Choice(_)));
    assert!(matches!(all, XsdParticle::All(_)));
    assert!(matches!(group_ref, XsdParticle::GroupRef(_)));
    assert!(matches!(any, XsdParticle::Any(_)));
}

// =============================================================================
// XsdParticleItem Tests
// =============================================================================

#[test]
fn test_xsd_particle_item_variants() {
    let elem = XsdParticleItem::Element(XsdElement::new("e"));
    let seq = XsdParticleItem::Sequence(XsdSequence::default());
    let choice = XsdParticleItem::Choice(XsdChoice::default());
    let group_ref = XsdParticleItem::GroupRef(QName::new("g"));
    let any = XsdParticleItem::Any(XsdAny::default());

    assert!(matches!(elem, XsdParticleItem::Element(_)));
    assert!(matches!(seq, XsdParticleItem::Sequence(_)));
    assert!(matches!(choice, XsdParticleItem::Choice(_)));
    assert!(matches!(group_ref, XsdParticleItem::GroupRef(_)));
    assert!(matches!(any, XsdParticleItem::Any(_)));
}
