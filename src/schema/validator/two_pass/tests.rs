//! Tests for two-pass schema validation.

#![allow(deprecated)]

use std::io::Cursor;
use std::sync::Arc;

use crate::event::StreamingParser;
use crate::schema::types::CompiledSchema;

use super::super::ValidationMode;
use super::TwoPassSchemaValidator;
use super::builder::SkeletonBuilder;
use super::skeleton::{DocumentSkeleton, ElementSkeleton};

fn create_test_reader(xml: &str) -> Cursor<Vec<u8>> {
    Cursor::new(xml.as_bytes().to_vec())
}

#[test]
fn test_element_skeleton_new() {
    let skeleton = ElementSkeleton::new("test".into(), None, Some(1), Some(5));
    assert_eq!(skeleton.name.as_ref(), "test");
    assert!(skeleton.prefix.is_none());
    assert_eq!(skeleton.line, Some(1));
    assert_eq!(skeleton.column, Some(5));
    assert!(skeleton.child_counts.is_empty());
    assert!(skeleton.text_content.is_empty());
}

#[test]
fn test_element_skeleton_child_counts() {
    let mut skeleton = ElementSkeleton::new("parent".into(), None, None, None);

    assert_eq!(skeleton.get_child_count("child"), 0);

    assert_eq!(skeleton.increment_child("child"), 1);
    assert_eq!(skeleton.get_child_count("child"), 1);

    assert_eq!(skeleton.increment_child("child"), 2);
    assert_eq!(skeleton.get_child_count("child"), 2);

    assert_eq!(skeleton.increment_child("other"), 1);
    assert_eq!(skeleton.get_child_count("other"), 1);
}

#[test]
fn test_document_skeleton_new() {
    let skeleton = DocumentSkeleton::new();
    assert!(skeleton.nodes.is_empty());
    assert!(skeleton.root_index.is_none());
}

#[test]
fn test_skeleton_builder_simple() {
    let xml = r#"<root><child>text</child></root>"#;
    let reader = create_test_reader(xml);

    let mut parser = StreamingParser::new(reader);
    let builder = SkeletonBuilder::new();
    parser.add_handler(Box::new(builder));
    parser.parse().unwrap();

    let handlers = parser.into_handlers();
    for handler in handlers {
        if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
            let skeleton = builder.into_skeleton();
            assert_eq!(skeleton.nodes.len(), 2);
            assert_eq!(skeleton.root_index, Some(0));

            // Root node
            let root = skeleton.get_node(0).unwrap();
            assert_eq!(root.name.as_ref(), "root");
            assert_eq!(root.get_child_count("child"), 1);
            assert_eq!(root.children_indices.len(), 1);

            // Child node
            let child = skeleton.get_node(1).unwrap();
            assert_eq!(child.name.as_ref(), "child");
            assert_eq!(child.text_content, "text");

            return;
        }
    }
    panic!("SkeletonBuilder not found");
}

#[test]
fn test_skeleton_builder_nested() {
    let xml = r#"<root><a><b><c/></b></a></root>"#;
    let reader = create_test_reader(xml);

    let mut parser = StreamingParser::new(reader);
    let builder = SkeletonBuilder::new();
    parser.add_handler(Box::new(builder));
    parser.parse().unwrap();

    let handlers = parser.into_handlers();
    for handler in handlers {
        if let Ok(builder) = handler.as_any().downcast::<SkeletonBuilder>() {
            let skeleton = builder.into_skeleton();
            assert_eq!(skeleton.nodes.len(), 4);

            // Check parent-child relationships
            let root = skeleton.get_node(0).unwrap();
            assert_eq!(root.name.as_ref(), "root");
            assert_eq!(root.children_indices, vec![1]);

            let a = skeleton.get_node(1).unwrap();
            assert_eq!(a.name.as_ref(), "a");
            assert_eq!(a.children_indices, vec![2]);

            let b = skeleton.get_node(2).unwrap();
            assert_eq!(b.name.as_ref(), "b");
            assert_eq!(b.children_indices, vec![3]);

            let c = skeleton.get_node(3).unwrap();
            assert_eq!(c.name.as_ref(), "c");
            assert!(c.children_indices.is_empty());

            return;
        }
    }
    panic!("SkeletonBuilder not found");
}

#[test]
fn test_two_pass_validator_empty_schema() {
    let xml = r#"<root><child/></root>"#;
    let reader = create_test_reader(xml);

    let schema = CompiledSchema::new();
    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();
    // Empty schema should not produce errors
    assert!(errors.is_empty());
}

#[test]
fn test_two_pass_validator_unknown_element_strict() {
    use crate::schema::types::ElementDef;

    let xml = r#"<unknown/>"#;
    let reader = create_test_reader(xml);

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("known".to_string(), ElementDef::new("known"));

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();
    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("unknown"));
}

#[test]
fn test_two_pass_validator_unknown_element_lenient() {
    use crate::schema::types::ElementDef;

    let xml = r#"<unknown/>"#;
    let reader = create_test_reader(xml);

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("known".to_string(), ElementDef::new("known"));

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .with_mode(ValidationMode::Lenient)
        .validate(reader)
        .unwrap();
    assert!(errors.is_empty());
}

#[test]
fn test_two_pass_validator_min_occurs() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let xml = r#"<parent></parent>"#;
    let reader = create_test_reader(xml);

    let mut schema = CompiledSchema::new();

    let complex_type = ComplexType {
        name: "ParentType".to_string(),
        base_type: None,
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

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("required_child"));
}

#[test]
fn test_two_pass_validator_max_occurs() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let xml = r#"<parent><child/><child/><child/></parent>"#;
    let reader = create_test_reader(xml);

    let mut schema = CompiledSchema::new();

    let complex_type = ComplexType {
        name: "ParentType".to_string(),
        base_type: None,
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

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    assert!(!errors.is_empty());
    assert!(errors[0].message.contains("maximum"));
}

#[test]
fn test_two_pass_validator_choice_content_model() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let xml = r#"<boundedBy><Envelope/></boundedBy>"#;
    let reader = create_test_reader(xml);

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

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    // Choice should accept one of the options
    assert!(errors.is_empty());
}

#[test]
fn test_two_pass_validator_substitution_group() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let xml = r#"<parent><ReliefFeature/></parent>"#;
    let reader = create_test_reader(xml);

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

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    // ReliefFeature should count toward _CityObject requirement
    assert!(
        errors.is_empty(),
        "Substitution group member should satisfy min_occurs, errors: {:?}",
        errors
    );
}

#[test]
fn test_two_pass_validator_with_max_errors() {
    use crate::schema::types::ElementDef;

    let xml = r#"<root><a/><b/><c/><d/><e/></root>"#;
    let reader = create_test_reader(xml);

    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("root".to_string(), ElementDef::new("root"));
    // Only root is known, all children are unknown

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .with_max_errors(2)
        .validate(reader)
        .unwrap();
    // Should stop at 2 errors
    assert_eq!(errors.len(), 2);
}

#[test]
fn test_two_pass_validator_type_inheritance() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let xml = r#"<root><baseElement>content</baseElement></root>"#;
    let reader = create_test_reader(xml);

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

    let errors = TwoPassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    // Should recognize inherited element
    assert!(
        errors.is_empty(),
        "Should accept inherited elements, errors: {:?}",
        errors
    );
}
