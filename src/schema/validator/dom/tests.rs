//! Tests for DOM-based schema validation.

use std::sync::Arc;

use crate::document::XmlDocument;
use crate::parse;
use crate::schema::types::{CompiledSchema, ComplexType, ContentModel, ElementDef, TypeDef};

use super::super::ValidationMode;
use super::DomSchemaValidator;

fn create_test_doc(xml: &str) -> XmlDocument {
    parse(xml.as_bytes()).unwrap()
}

#[test]
fn test_dom_validator_empty_schema() {
    let doc = create_test_doc("<root><child/></root>");
    let schema = CompiledSchema::new();
    let validator = DomSchemaValidator::new(Arc::new(schema));

    let errors = validator.validate(&doc).unwrap();
    // Empty schema should not produce errors
    assert!(errors.is_empty());
}

#[test]
fn test_dom_validator_unknown_element_strict() {
    let doc = create_test_doc("<unknown/>");

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("known".to_string(), ElementDef::new("known"));

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("unknown"));
}

#[test]
fn test_dom_validator_unknown_element_lenient() {
    let doc = create_test_doc("<unknown/>");

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("known".to_string(), ElementDef::new("known"));

    let validator = DomSchemaValidator::new(Arc::new(schema)).with_mode(ValidationMode::Lenient);
    let errors = validator.validate(&doc).unwrap();

    assert!(errors.is_empty());
}

#[test]
fn test_dom_validator_min_occurs() {
    let doc = create_test_doc("<parent></parent>");

    let mut schema = CompiledSchema::new();

    let complex_type = ComplexType {
        name: "ParentType".to_string(),
        base_type: None,
        derivation: None,
        block: Default::default(),
        content: ContentModel::Sequence(vec![
            ElementDef::new("required_child").with_occurs(1, Some(1)),
        ]),
        attributes: Vec::new(),
        is_abstract: false,
        mixed: false,
    };

    schema.elements.insert(
        "parent".to_string(),
        ElementDef::new("parent").with_type("ParentType"),
    );

    schema
        .types
        .insert("ParentType".to_string(), TypeDef::Complex(complex_type));

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("required_child"));
}

#[test]
fn test_dom_validator_max_occurs() {
    let doc = create_test_doc("<parent><child/><child/><child/></parent>");

    let mut schema = CompiledSchema::new();

    let complex_type = ComplexType {
        name: "ParentType".to_string(),
        base_type: None,
        derivation: None,
        block: Default::default(),
        content: ContentModel::Sequence(vec![ElementDef::new("child").with_occurs(0, Some(2))]),
        attributes: Vec::new(),
        is_abstract: false,
        mixed: false,
    };

    schema.elements.insert(
        "parent".to_string(),
        ElementDef::new("parent").with_type("ParentType"),
    );

    schema
        .types
        .insert("ParentType".to_string(), TypeDef::Complex(complex_type));

    // Also define child as global element
    schema
        .elements
        .insert("child".to_string(), ElementDef::new("child"));

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("maximum"));
}

#[test]
fn test_dom_validator_choice_content_model() {
    let doc = create_test_doc("<boundedBy><Envelope/></boundedBy>");

    let mut schema = CompiledSchema::new();

    let mut choice_type = ComplexType::new("BoundingShapeType");
    choice_type.content = ContentModel::Choice(vec![
        ElementDef::new("Envelope").with_type("xs:string"),
        ElementDef::new("Null").with_type("xs:string"),
    ]);

    schema.types.insert(
        "BoundingShapeType".to_string(),
        TypeDef::Complex(choice_type),
    );

    schema.elements.insert(
        "boundedBy".to_string(),
        ElementDef::new("boundedBy").with_type("BoundingShapeType"),
    );
    schema
        .elements
        .insert("Envelope".to_string(), ElementDef::new("Envelope"));
    schema
        .elements
        .insert("Null".to_string(), ElementDef::new("Null"));

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    // Choice should accept one of the options
    assert!(errors.is_empty());
}

#[test]
fn test_dom_validator_substitution_group() {
    let doc = create_test_doc("<parent><ReliefFeature/></parent>");

    let mut schema = CompiledSchema::new();

    // Parent type expects _CityObject
    let mut parent_type = ComplexType::new("ParentType");
    parent_type.content = ContentModel::Sequence(vec![
        ElementDef::new("_CityObject").with_type("AbstractCityObjectType"),
    ]);
    schema
        .types
        .insert("ParentType".to_string(), TypeDef::Complex(parent_type));

    let abstract_type = ComplexType::new("AbstractCityObjectType");
    schema.types.insert(
        "AbstractCityObjectType".to_string(),
        TypeDef::Complex(abstract_type),
    );

    // Define elements
    let mut head_elem = ElementDef::new("_CityObject");
    head_elem.is_abstract = true;
    head_elem.type_ref = Some("AbstractCityObjectType".to_string());
    schema.elements.insert("_CityObject".to_string(), head_elem);

    let mut substitute_elem = ElementDef::new("ReliefFeature");
    substitute_elem.substitution_group = Some("_CityObject".to_string());
    schema
        .elements
        .insert("ReliefFeature".to_string(), substitute_elem);

    schema.elements.insert(
        "parent".to_string(),
        ElementDef::new("parent").with_type("ParentType"),
    );

    // Build substitution group caches
    schema
        .substitution_groups
        .insert("_CityObject".to_string(), vec!["ReliefFeature".to_string()]);
    schema
        .substitution_group_heads
        .insert("ReliefFeature".to_string(), "_CityObject".to_string());
    schema.transitive_substitution_groups.insert(
        "_CityObject".to_string(),
        Arc::new(vec!["ReliefFeature".to_string()]),
    );

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    // ReliefFeature should count toward _CityObject requirement
    assert!(
        errors.is_empty(),
        "Substitution group member should satisfy min_occurs, errors: {:?}",
        errors
    );
}

#[test]
fn test_dom_validator_with_max_errors() {
    let doc = create_test_doc("<root><a/><b/><c/><d/><e/></root>");

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("root".to_string(), ElementDef::new("root"));
    // Only root is known, all children are unknown

    let validator = DomSchemaValidator::new(Arc::new(schema)).with_max_errors(2);
    let errors = validator.validate(&doc).unwrap();

    // Should stop at 2 errors
    assert_eq!(errors.len(), 2);
}

#[test]
fn test_dom_validator_type_inheritance() {
    let doc = create_test_doc("<root><baseElement>content</baseElement></root>");

    let mut schema = CompiledSchema::new();

    // BaseType with "baseElement"
    let mut base_type = ComplexType::new("BaseType");
    base_type.content = ContentModel::Sequence(vec![
        ElementDef::new("baseElement")
            .with_type("xs:string")
            .optional(),
    ]);
    schema
        .types
        .insert("BaseType".to_string(), TypeDef::Complex(base_type));

    // ExtendedType extends BaseType
    let mut extended_type = ComplexType::new("ExtendedType");
    extended_type.content = ContentModel::ComplexExtension {
        base_type: "BaseType".to_string(),
        elements: vec![],
    };
    schema
        .types
        .insert("ExtendedType".to_string(), TypeDef::Complex(extended_type));

    schema.elements.insert(
        "root".to_string(),
        ElementDef::new("root").with_type("ExtendedType"),
    );
    schema.elements.insert(
        "baseElement".to_string(),
        ElementDef::new("baseElement").with_type("xs:string"),
    );

    let validator = DomSchemaValidator::new(Arc::new(schema));
    let errors = validator.validate(&doc).unwrap();

    // Should recognize inherited element
    assert!(
        errors.is_empty(),
        "Should accept inherited elements, errors: {:?}",
        errors
    );
}

#[test]
fn test_dom_validator_count_child_elements() {
    let doc = create_test_doc("<parent><a/><b/><a/><c/><a/></parent>");

    let schema = CompiledSchema::new();
    let validator = DomSchemaValidator::new(Arc::new(schema));

    let root = doc.get_root_element().unwrap();
    let counts = validator.count_child_elements(&root);

    assert_eq!(counts.get("a"), Some(&3));
    assert_eq!(counts.get("b"), Some(&1));
    assert_eq!(counts.get("c"), Some(&1));
    assert_eq!(counts.get("d"), None);
}

#[test]
fn test_dom_validator_collect_text_content() {
    let doc = create_test_doc("<root>Hello <child/>World</root>");

    let schema = CompiledSchema::new();
    let validator = DomSchemaValidator::new(Arc::new(schema));

    let root = doc.get_root_element().unwrap();
    let text = validator.collect_text_content(&root);

    assert_eq!(text, "Hello World");
}
