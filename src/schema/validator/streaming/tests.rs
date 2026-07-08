//! Tests for the one-pass streaming schema validator.

use std::sync::Arc;

use super::*;
use crate::error::{ErrorLevel, StructuredError, ValidationErrorType};
use crate::event::{RawEvent, XmlEventHandler};
use crate::namespace::Namespace;
use crate::schema::types::{
    CompiledSchema, ComplexType, ContentModel, ContentModelType, FlattenedChildren,
};

use super::super::state::{ElementContext, ValidationState};

// =============================================
// ValidationMode Tests
// =============================================

#[test]
fn test_validation_mode_default() {
    let mode = ValidationMode::default();
    assert_eq!(mode, ValidationMode::Strict);
}

// =============================================
// ValidationState Tests
// =============================================

#[test]
fn test_validation_state_new() {
    let state = ValidationState::new();
    assert!(state.element_stack.is_empty());
    assert_eq!(state.depth, 0);
    assert_eq!(state.namespace_stack.len(), 1);
}

#[test]
fn test_validation_state_push_pop_element() {
    let mut state = ValidationState::new();

    state.push_element_str("root", None);
    assert_eq!(state.depth, 1);
    assert_eq!(state.element_stack.len(), 1);
    assert_eq!(state.element_stack[0].name.as_ref(), "root");

    state.push_element_str("child", Some("http://example.com"));
    assert_eq!(state.depth, 2);
    assert_eq!(state.element_stack.len(), 2);

    let popped = state.pop_element().unwrap();
    assert_eq!(popped.name.as_ref(), "child");
    assert_eq!(state.depth, 1);
}

#[test]
fn test_validation_state_element_path() {
    let mut state = ValidationState::new();
    assert_eq!(state.element_path(), "/");

    state.push_element_str("root", None);
    assert_eq!(state.element_path(), "/root");

    state.push_element_str("child", None);
    assert_eq!(state.element_path(), "/root/child");
}

// =============================================
// ElementContext Tests
// =============================================

#[test]
fn test_element_context_new() {
    let ctx = ElementContext::from_str("test", Some("http://example.com"));
    assert_eq!(ctx.name.as_ref(), "test");
    assert_eq!(ctx.namespace.as_deref(), Some("http://example.com"));
    assert!(ctx.child_counts.is_empty());
    assert!(ctx.text_content.is_empty());
    assert!(!ctx.schema_validated);
}

#[test]
fn test_element_context_child_counts() {
    let mut ctx = ElementContext::from_str("parent", None);

    assert_eq!(ctx.get_child_count("child1"), 0);

    assert_eq!(ctx.increment_child("child1"), 1);
    assert_eq!(ctx.get_child_count("child1"), 1);

    assert_eq!(ctx.increment_child("child1"), 2);
    assert_eq!(ctx.get_child_count("child1"), 2);

    assert_eq!(ctx.increment_child("child2"), 1);
    assert_eq!(ctx.get_child_count("child2"), 1);
}

// =============================================
// OnePassSchemaValidator Tests
// =============================================

#[test]
fn test_streaming_validator_new() {
    let schema = CompiledSchema::new();
    let validator = OnePassSchemaValidator::new(Arc::new(schema));
    assert!(validator.is_valid());
    assert!(validator.is_clean());
    assert_eq!(validator.error_count(), 0);
}

#[test]
fn test_streaming_validator_with_mode() {
    let schema = CompiledSchema::new();
    let validator = OnePassSchemaValidator::new(Arc::new(schema)).set_mode(ValidationMode::Lenient);
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_max_errors() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));
    validator.set_max_errors(2);

    // Add 3 errors
    validator.add_error(StructuredError::new("error1", ValidationErrorType::Other));
    validator.add_error(StructuredError::new("error2", ValidationErrorType::Other));
    validator.add_error(StructuredError::new("error3", ValidationErrorType::Other));

    // Should only have 2 errors
    assert_eq!(validator.errors().len(), 2);
}

#[test]
fn test_streaming_validator_make_error() {
    let schema = CompiledSchema::new();
    let validator = OnePassSchemaValidator::new(Arc::new(schema));

    let error = validator.make_error(ValidationErrorType::UnknownElement, "test error");
    assert_eq!(error.message.as_ref(), "test error");
    assert_eq!(error.error_type, ValidationErrorType::UnknownElement);
}

#[test]
fn test_streaming_validator_errors_and_warnings() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator.add_error(
        StructuredError::new("error1", ValidationErrorType::Other).with_level(ErrorLevel::Error),
    );
    validator.add_error(
        StructuredError::new("warning1", ValidationErrorType::Other)
            .with_level(ErrorLevel::Warning),
    );
    validator.add_error(
        StructuredError::new("error2", ValidationErrorType::Other).with_level(ErrorLevel::Error),
    );

    assert_eq!(validator.error_count(), 2);
    assert_eq!(validator.warning_count(), 1);
    assert_eq!(validator.errors_only().len(), 2);
    assert_eq!(validator.warnings().len(), 1);
    assert!(!validator.is_valid());
    assert!(!validator.is_clean());
}

#[test]
fn test_streaming_validator_with_schema_elements() {
    use crate::schema::types::{ElementDef, SimpleType, TypeDef};

    let mut schema = CompiledSchema::new();

    // Add a simple element definition
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "root"),
        ElementDef::new("root").with_type("xs:string"),
    );

    // Add type definition
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "string"),
        TypeDef::Simple(SimpleType::new("xs:string")),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Valid element
    let _ = validator.handle(&RawEvent::StartElement {
        name: "root",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: Some(1),
        column: Some(1),
    });

    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_unknown_element_strict() {
    use crate::schema::types::ElementDef;

    let mut schema = CompiledSchema::new();

    // Add at least one element so schema has elements
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "known"),
        ElementDef::new("known"),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    let _ = validator.handle(&RawEvent::StartElement {
        name: "unknown",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: Some(1),
        column: Some(1),
    });

    // Should have an error for unknown element in strict mode
    assert!(!validator.is_valid());
    assert!(
        validator
            .errors()
            .iter()
            .any(|e| e.message.contains("unknown"))
    );
}

#[test]
fn test_streaming_validator_unknown_element_lenient() {
    use crate::schema::types::ElementDef;

    let mut schema = CompiledSchema::new();

    // Add at least one element so schema has elements
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "known"),
        ElementDef::new("known"),
    );

    let mut validator =
        OnePassSchemaValidator::new(Arc::new(schema)).set_mode(ValidationMode::Lenient);

    let _ = validator.handle(&RawEvent::StartElement {
        name: "unknown",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: Some(1),
        column: Some(1),
    });

    // Should NOT have an error in lenient mode
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_text_content() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    let _ = validator.handle(&RawEvent::StartElement {
        name: "test",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: None,
        column: Some(1),
    });

    let _ = validator.handle(&RawEvent::Text("content"));

    // Check that text was collected
    let ctx = validator.state.current_element().unwrap();
    assert_eq!(ctx.text_content, "content");
}

#[test]
fn test_streaming_validator_cdata_content() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    let _ = validator.handle(&RawEvent::StartElement {
        name: "test",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: None,
        column: Some(1),
    });

    let _ = validator.handle(&RawEvent::CData("cdata content"));

    // Check that CDATA was collected as text
    let ctx = validator.state.current_element().unwrap();
    assert_eq!(ctx.text_content, "cdata content");
}

#[test]
fn test_streaming_validator_finish_unclosed_element() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start element but don't close it
    let _ = validator.handle(&RawEvent::StartElement {
        name: "unclosed",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: None,
        column: Some(1),
    });

    let _ = validator.finish();

    // Should report unclosed element
    assert!(!validator.is_valid());
    assert!(
        validator
            .errors()
            .iter()
            .any(|e| e.message.contains("not closed"))
    );
}

#[test]
fn test_streaming_validator_into_errors() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator.add_error(StructuredError::new(
        "test error",
        ValidationErrorType::Other,
    ));

    let errors = validator.into_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message.as_ref(), "test error");
}

#[test]
fn test_streaming_validator_min_occurs() {
    use crate::schema::types::{ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // Create a complex type with required child
    let complex_type = ComplexType {
        name: "ParentType".to_string(),
        base_type: None,
        base_ns: None,
        derivation: None,
        block: Default::default(),
        wildcard: None,
        attr_wildcard: None,
        content: ContentModel::Sequence(vec![
            ElementDef::new("required_child").with_occurs(1, Some(1)),
        ]),
        attributes: Vec::new(),
        is_abstract: false,
        mixed: false,
        particle: None,
    };

    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "parent"),
        ElementDef::new("parent").with_type("ParentType"),
    );

    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ParentType"),
        TypeDef::Complex(complex_type),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start parent element
    let _ = validator.handle(&RawEvent::StartElement {
        name: "parent",
        prefix: None,
        attributes: &[],
        namespace_decls: &[],
        line: Some(1),
        column: Some(1),
    });

    // End parent without adding required child
    let _ = validator.handle(&RawEvent::EndElement {
        name: "parent",
        prefix: None,
    });

    // Should have error about missing required child
    assert!(!validator.is_valid());
    assert!(
        validator
            .errors()
            .iter()
            .any(|e| e.message.contains("required_child"))
    );
}

#[test]
fn test_streaming_validator() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.handle(&RawEvent::Eof).unwrap();
    validator.finish().unwrap();

    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_set_max_errors() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));
    validator.set_max_errors(5);
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_errors_methods() {
    let schema = CompiledSchema::new();
    let validator = OnePassSchemaValidator::new(Arc::new(schema));
    assert!(validator.errors().is_empty());
    assert!(validator.errors_only().is_empty());
    assert!(validator.warnings().is_empty());
    assert_eq!(validator.error_count(), 0);
    assert_eq!(validator.warning_count(), 0);
}

#[test]
fn test_streaming_validator_is_clean() {
    let schema = CompiledSchema::new();
    let validator = OnePassSchemaValidator::new(Arc::new(schema));
    assert!(validator.is_clean());
}

#[test]
fn test_streaming_validator_with_prefix() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: Some("ns"),
            attributes: &[],
            namespace_decls: &[Namespace::new(
                "ns".to_string(),
                "http://example.com".to_string(),
            )],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: Some("ns"),
        })
        .unwrap();

    validator.finish().unwrap();
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_with_attributes() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[
                ("id", "1".into()),
                ("xmlns:ns", "http://example.com".into()),
                ("xsi:schemaLocation", "http://example.com schema.xsd".into()),
            ],
            namespace_decls: &[],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_nested_elements() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::StartElement {
            name: "child",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::StartElement {
            name: "grandchild",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator.handle(&RawEvent::Text("content")).unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "grandchild",
            prefix: None,
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "child",
            prefix: None,
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();
    assert!(validator.is_valid());
}

#[test]
fn test_streaming_validator_other_events() {
    let schema = CompiledSchema::new();
    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // ProcessingInstruction
    validator
        .handle(&RawEvent::ProcessingInstruction {
            target: "xml",
            content: Some("version=\"1.0\""),
        })
        .unwrap();

    // Comment
    validator
        .handle(&RawEvent::Comment("This is a comment"))
        .unwrap();

    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: None,
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();
    assert!(validator.is_valid());
}

// =============================================
// Type Inheritance Tests
// =============================================

/// Test that inherited elements from base types are recognized.
///
/// This test reproduces the issue where elements like `creationDate` defined
/// in a base type (e.g., AbstractCityObjectType) are not recognized when
/// validating an element whose type extends that base type.
#[test]
fn test_inherited_elements_from_base_type() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    // Build a schema with type inheritance:
    // - BaseType has element "baseElement" (like creationDate in _CityObject)
    // - ExtendedType extends BaseType and adds "extElement" (like lod in ReliefFeature)
    // - "root" element uses ExtendedType

    let mut schema = CompiledSchema::new();

    // BaseType with "baseElement"
    let mut base_type = ComplexType::new("BaseType");
    base_type.content = ContentModel::Sequence(vec![
        ElementDef::new("baseElement")
            .with_type("xs:string")
            .optional(),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "BaseType"),
        TypeDef::Complex(base_type),
    );

    // ExtendedType extends BaseType, adds "extElement"
    let mut extended_type = ComplexType::new("ExtendedType");
    extended_type.content = ContentModel::ComplexExtension {
        base_type: "BaseType".to_string(),
        elements: vec![
            ElementDef::new("extElement")
                .with_type("xs:integer")
                .optional(),
        ],
    };
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ExtendedType"),
        TypeDef::Complex(extended_type),
    );

    // Root element uses ExtendedType
    let root_elem = ElementDef::new("root").with_type("ExtendedType");
    schema
        .elements_ns
        .insert(crate::schema::types::NsName::new("", "root"), root_elem);

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start root element
    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Add inherited element (baseElement) - this should be valid!
    validator
        .handle(&RawEvent::StartElement {
            name: "baseElement",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::Text("inherited content"))
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "baseElement",
            prefix: None,
        })
        .unwrap();

    // Add direct extension element (extElement)
    validator
        .handle(&RawEvent::StartElement {
            name: "extElement",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();

    validator.handle(&RawEvent::Text("42")).unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "extElement",
            prefix: None,
        })
        .unwrap();

    // End root
    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // Check for errors - inherited element should NOT cause an error
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("baseElement"))
        .collect();

    assert!(
        errors.is_empty(),
        "Inherited element 'baseElement' should be recognized, but got errors: {:?}",
        errors
    );

    assert!(
        validator.is_valid(),
        "Validation should pass for inherited elements, but got errors: {:?}",
        validator.errors()
    );
}

/// Test multi-level type inheritance (grandparent -> parent -> child).
#[test]
fn test_multi_level_inheritance() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // GrandparentType has "grandparentElem"
    let mut grandparent_type = ComplexType::new("GrandparentType");
    grandparent_type.content = ContentModel::Sequence(vec![
        ElementDef::new("grandparentElem")
            .with_type("xs:string")
            .optional(),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "GrandparentType"),
        TypeDef::Complex(grandparent_type),
    );

    // ParentType extends GrandparentType, adds "parentElem"
    let mut parent_type = ComplexType::new("ParentType");
    parent_type.content = ContentModel::ComplexExtension {
        base_type: "GrandparentType".to_string(),
        elements: vec![
            ElementDef::new("parentElem")
                .with_type("xs:string")
                .optional(),
        ],
    };
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ParentType"),
        TypeDef::Complex(parent_type),
    );

    // ChildType extends ParentType, adds "childElem"
    let mut child_type = ComplexType::new("ChildType");
    child_type.content = ContentModel::ComplexExtension {
        base_type: "ParentType".to_string(),
        elements: vec![
            ElementDef::new("childElem")
                .with_type("xs:string")
                .optional(),
        ],
    };
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ChildType"),
        TypeDef::Complex(child_type),
    );

    // Root element uses ChildType
    let root_elem = ElementDef::new("root").with_type("ChildType");
    schema
        .elements_ns
        .insert(crate::schema::types::NsName::new("", "root"), root_elem);

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start root
    validator
        .handle(&RawEvent::StartElement {
            name: "root",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Add grandparent-level element
    validator
        .handle(&RawEvent::StartElement {
            name: "grandparentElem",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&RawEvent::Text("gp")).unwrap();
    validator
        .handle(&RawEvent::EndElement {
            name: "grandparentElem",
            prefix: None,
        })
        .unwrap();

    // Add parent-level element
    validator
        .handle(&RawEvent::StartElement {
            name: "parentElem",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(3),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&RawEvent::Text("p")).unwrap();
    validator
        .handle(&RawEvent::EndElement {
            name: "parentElem",
            prefix: None,
        })
        .unwrap();

    // Add child-level element
    validator
        .handle(&RawEvent::StartElement {
            name: "childElem",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(4),
            column: Some(1),
        })
        .unwrap();
    validator.handle(&RawEvent::Text("c")).unwrap();
    validator
        .handle(&RawEvent::EndElement {
            name: "childElem",
            prefix: None,
        })
        .unwrap();

    // End root
    validator
        .handle(&RawEvent::EndElement {
            name: "root",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // All three elements should be valid (inherited from different levels)
    assert!(
        validator.is_valid(),
        "Multi-level inheritance should work, but got errors: {:?}",
        validator.errors()
    );
}

// =============================================
// Substitution Group Tests
// =============================================

/// Test that substitution group members can be used in place of the head element.
///
/// This test reproduces the issue where elements like `dem:ReliefFeature` are not
/// recognized as valid substitutes for abstract elements like `_CityObject`.
#[test]
fn test_substitution_group_basic() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // Define a parent type that expects "_CityObject" (abstract head element) as REQUIRED
    let mut parent_type = ComplexType::new("ParentType");
    parent_type.content = ContentModel::Sequence(vec![
        // Parent expects "_CityObject" as required child (min_occurs=1)
        ElementDef::new("_CityObject").with_type("AbstractCityObjectType"),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ParentType"),
        TypeDef::Complex(parent_type),
    );

    // Define the abstract type
    let abstract_type = ComplexType::new("AbstractCityObjectType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractCityObjectType"),
        TypeDef::Complex(abstract_type),
    );

    // Define the concrete type
    let concrete_type = ComplexType::new("ReliefFeatureType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeatureType"),
        TypeDef::Complex(concrete_type),
    );

    // Define the head element (abstract)
    let mut head_elem = ElementDef::new("_CityObject");
    head_elem.is_abstract = true;
    head_elem.type_ref = Some("AbstractCityObjectType".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "_CityObject"),
        head_elem,
    );

    // Define the substitute element (concrete)
    let mut substitute_elem = ElementDef::new("ReliefFeature");
    substitute_elem.type_ref = Some("ReliefFeatureType".to_string());
    substitute_elem.substitution_group = Some("_CityObject".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeature"),
        substitute_elem,
    );

    // Define parent element
    let parent_elem = ElementDef::new("parent").with_type("ParentType");
    schema
        .elements_ns
        .insert(crate::schema::types::NsName::new("", "parent"), parent_elem);

    // Build substitution groups (head -> members)
    schema
        .substitution_groups
        .insert("_CityObject".to_string(), vec!["ReliefFeature".to_string()]);

    // Build reverse lookup cache (member -> head)
    schema
        .substitution_group_heads
        .insert("ReliefFeature".to_string(), "_CityObject".to_string());

    // Build transitive members cache (head -> all members)
    schema.transitive_substitution_groups.insert(
        "_CityObject".to_string(),
        Arc::new(vec!["ReliefFeature".to_string()]),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start parent element
    validator
        .handle(&RawEvent::StartElement {
            name: "parent",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Use substitute element (ReliefFeature instead of _CityObject)
    validator
        .handle(&RawEvent::StartElement {
            name: "ReliefFeature",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "ReliefFeature",
            prefix: None,
        })
        .unwrap();

    // End parent
    validator
        .handle(&RawEvent::EndElement {
            name: "parent",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // Check: ReliefFeature should be accepted as a substitute for _CityObject
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("ReliefFeature") && e.message.contains("not declared"))
        .collect();

    assert!(
        errors.is_empty(),
        "Substitution group member 'ReliefFeature' should be accepted in place of '_CityObject', but got errors: {:?}",
        errors
    );

    assert!(
        validator.is_valid(),
        "Validation should pass for substitution group members, but got errors: {:?}",
        validator.errors()
    );
}

/// Test that max_occurs is correctly validated for substitution groups.
///
/// When multiple substitution group members are used, their counts should be
/// summed when checking against the max_occurs constraint.
#[test]
fn test_substitution_group_max_occurs() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // Define a parent type that expects "_CityObject" with max_occurs=2
    let mut parent_type = ComplexType::new("ParentType");
    parent_type.content = ContentModel::Sequence(vec![
        // Parent expects "_CityObject" at most 2 times
        ElementDef::new("_CityObject")
            .with_type("AbstractCityObjectType")
            .with_occurs(0, Some(2)),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ParentType"),
        TypeDef::Complex(parent_type),
    );

    // Define types
    let abstract_type = ComplexType::new("AbstractCityObjectType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractCityObjectType"),
        TypeDef::Complex(abstract_type),
    );

    let relief_type = ComplexType::new("ReliefFeatureType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeatureType"),
        TypeDef::Complex(relief_type),
    );

    let building_type = ComplexType::new("BuildingType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "BuildingType"),
        TypeDef::Complex(building_type),
    );

    // Define elements
    let mut head_elem = ElementDef::new("_CityObject");
    head_elem.is_abstract = true;
    head_elem.type_ref = Some("AbstractCityObjectType".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "_CityObject"),
        head_elem,
    );

    let mut relief_elem = ElementDef::new("ReliefFeature");
    relief_elem.type_ref = Some("ReliefFeatureType".to_string());
    relief_elem.substitution_group = Some("_CityObject".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeature"),
        relief_elem,
    );

    let mut building_elem = ElementDef::new("Building");
    building_elem.type_ref = Some("BuildingType".to_string());
    building_elem.substitution_group = Some("_CityObject".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "Building"),
        building_elem,
    );

    let parent_elem = ElementDef::new("parent").with_type("ParentType");
    schema
        .elements_ns
        .insert(crate::schema::types::NsName::new("", "parent"), parent_elem);

    // Build substitution groups
    schema.substitution_groups.insert(
        "_CityObject".to_string(),
        vec!["ReliefFeature".to_string(), "Building".to_string()],
    );

    // Build reverse lookup cache (member -> head)
    schema
        .substitution_group_heads
        .insert("ReliefFeature".to_string(), "_CityObject".to_string());
    schema
        .substitution_group_heads
        .insert("Building".to_string(), "_CityObject".to_string());

    // Build transitive members cache (head -> all members)
    schema.transitive_substitution_groups.insert(
        "_CityObject".to_string(),
        Arc::new(vec!["ReliefFeature".to_string(), "Building".to_string()]),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start parent
    validator
        .handle(&RawEvent::StartElement {
            name: "parent",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Add 3 substitutes (exceeds max_occurs=2)
    for (i, name) in ["ReliefFeature", "Building", "ReliefFeature"]
        .iter()
        .enumerate()
    {
        validator
            .handle(&RawEvent::StartElement {
                name,
                prefix: None,
                attributes: &[],
                namespace_decls: &[],
                line: Some(i + 2),
                column: Some(1),
            })
            .unwrap();
        validator
            .handle(&RawEvent::EndElement { name, prefix: None })
            .unwrap();
    }

    // End parent
    validator
        .handle(&RawEvent::EndElement {
            name: "parent",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // Check: Should have a max_occurs error since we have 3 substitutes but max is 2
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("occurs") && e.message.contains("maximum"))
        .collect();

    assert!(
        !errors.is_empty(),
        "Should have a max_occurs error when 3 substitutes are used but max is 2, errors: {:?}",
        validator.errors()
    );
}

// =============================================
// Choice Content Model Tests
// =============================================

/// Test that Choice content model accepts any one of the choices.
///
/// This test reproduces the issue where `boundedBy` requires `Envelope` OR `Null`,
/// but the validator incorrectly requires both when using Choice content model.
#[test]
fn test_choice_content_model_basic() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // Define a type with Choice content model (like BoundingShapeType)
    // Choice means: ONE of the elements should be present, not ALL
    let mut choice_type = ComplexType::new("BoundingShapeType");
    choice_type.content = ContentModel::Choice(vec![
        ElementDef::new("Envelope").with_type("xs:string"),
        ElementDef::new("Null").with_type("xs:string"),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "BoundingShapeType"),
        TypeDef::Complex(choice_type),
    );

    // Define parent element that uses the choice type
    let parent_elem = ElementDef::new("boundedBy").with_type("BoundingShapeType");
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "boundedBy"),
        parent_elem,
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start boundedBy
    validator
        .handle(&RawEvent::StartElement {
            name: "boundedBy",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Add Envelope (one of the choices)
    validator
        .handle(&RawEvent::StartElement {
            name: "Envelope",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "Envelope",
            prefix: None,
        })
        .unwrap();

    // End boundedBy
    validator
        .handle(&RawEvent::EndElement {
            name: "boundedBy",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // Check: Should NOT have an error about missing 'Null' element
    // because Choice means ONE of the options, not ALL
    let errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("Null") && e.message.contains("requires"))
        .collect();

    assert!(
        errors.is_empty(),
        "Choice content model should accept any ONE of the choices, not require ALL. Got errors: {:?}",
        errors
    );

    assert!(
        validator.is_valid(),
        "Validation should pass when one choice element is present, but got errors: {:?}",
        validator.errors()
    );
}

// =============================================
// Simple API Tests (validate method)
// =============================================

/// Test the simple validate() API.
#[test]
fn test_validate_simple_api() {
    let schema = CompiledSchema::new();
    let xml = r#"<root><child>text</child></root>"#;
    let reader = std::io::BufReader::new(xml.as_bytes());

    let errors = OnePassSchemaValidator::new(Arc::new(schema))
        .validate(reader)
        .unwrap();

    // Empty schema should validate any document
    assert!(errors.is_empty());
}

/// Test the simple validate() API with max_errors.
#[test]
fn test_validate_simple_api_with_max_errors() {
    use crate::schema::types::ElementDef;

    let mut schema = CompiledSchema::new();
    // Add an element so unknown elements trigger errors
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "known"),
        ElementDef::new("known"),
    );

    let xml = r#"<unknown1><unknown2><unknown3/></unknown2></unknown1>"#;
    let reader = std::io::BufReader::new(xml.as_bytes());

    let errors = OnePassSchemaValidator::new(Arc::new(schema))
        .with_max_errors(2)
        .validate(reader)
        .unwrap();

    // Should have at most 2 errors due to max_errors limit
    assert_eq!(errors.len(), 2);
}

/// Test the builder pattern methods.
#[test]
fn test_builder_pattern() {
    let schema = Arc::new(CompiledSchema::new());
    let xml = r#"<root/>"#;
    let reader = std::io::BufReader::new(xml.as_bytes());

    let errors = OnePassSchemaValidator::new(Arc::clone(&schema))
        .set_mode(ValidationMode::Lenient)
        .with_max_errors(10)
        .validate(reader)
        .unwrap();

    assert!(errors.is_empty());
}

/// Test substitution groups with prefixed element names.
///
/// This reproduces the issue where:
/// - Schema expects `_Ring` as child (abstract element)
/// - XML has `gml:LinearRing` (prefixed substitution group member)
/// - Substitution members are stored as `["Ring", "LinearRing"]` (no prefix)
/// - Child counts are stored as `gml:LinearRing` (with prefix)
/// - Validation should recognize `gml:LinearRing` as a valid substitute for `_Ring`
#[test]
fn test_substitution_group_with_prefixed_elements() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    let mut schema = CompiledSchema::new();

    // Define parent type (like AbstractRingPropertyType) that expects "_Ring"
    let mut parent_type = ComplexType::new("AbstractRingPropertyType");
    parent_type.content = ContentModel::Sequence(vec![
        // Parent expects "_Ring" as required child
        ElementDef::new("_Ring").with_type("AbstractRingType"),
    ]);
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractRingPropertyType"),
        TypeDef::Complex(parent_type),
    );

    // Define the abstract type
    let abstract_type = ComplexType::new("AbstractRingType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractRingType"),
        TypeDef::Complex(abstract_type),
    );

    // Define the concrete type
    let concrete_type = ComplexType::new("LinearRingType");
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "LinearRingType"),
        TypeDef::Complex(concrete_type),
    );

    // Define the head element (abstract)
    let mut head_elem = ElementDef::new("_Ring");
    head_elem.is_abstract = true;
    head_elem.type_ref = Some("AbstractRingType".to_string());
    schema
        .elements_ns
        .insert(crate::schema::types::NsName::new("", "_Ring"), head_elem);

    // Define the substitute element
    let mut substitute_elem = ElementDef::new("LinearRing");
    substitute_elem.type_ref = Some("LinearRingType".to_string());
    substitute_elem.substitution_group = Some("_Ring".to_string());
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "LinearRing"),
        substitute_elem,
    );

    // Define parent element (like "exterior")
    let parent_elem = ElementDef::new("exterior").with_type("AbstractRingPropertyType");
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "exterior"),
        parent_elem,
    );

    // Build substitution groups (head -> members)
    schema
        .substitution_groups
        .insert("_Ring".to_string(), vec!["LinearRing".to_string()]);

    // Build reverse lookup cache (member -> head)
    schema
        .substitution_group_heads
        .insert("LinearRing".to_string(), "_Ring".to_string());

    // Build transitive members cache (head -> all members)
    schema.transitive_substitution_groups.insert(
        "_Ring".to_string(),
        Arc::new(vec!["LinearRing".to_string()]),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    // Start exterior element
    validator
        .handle(&RawEvent::StartElement {
            name: "exterior",
            prefix: None,
            attributes: &[],
            namespace_decls: &[],
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Use prefixed substitute element: gml:LinearRing instead of _Ring
    // Note: In actual XML parsing, 'name' is the local name only,
    // and 'prefix' is passed separately
    validator
        .handle(&RawEvent::StartElement {
            name: "LinearRing",  // Local name only
            prefix: Some("gml"), // Prefix passed separately
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "LinearRing",
            prefix: Some("gml"),
        })
        .unwrap();

    // End exterior
    validator
        .handle(&RawEvent::EndElement {
            name: "exterior",
            prefix: None,
        })
        .unwrap();

    validator.finish().unwrap();

    // Should have no errors - gml:LinearRing should be recognized as substitute for _Ring
    let ring_errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("_Ring"))
        .collect();

    assert!(
        ring_errors.is_empty(),
        "Prefixed substitution group member 'gml:LinearRing' should satisfy '_Ring' requirement, but got errors: {:?}",
        ring_errors
    );
}

/// Test that elements with same local name but different namespaces are distinguished.
///
/// This reproduces the issue where `gml:boundedBy` (expects Envelope/Null) and
/// `brid:boundedBy` (expects WallSurface/RoofSurface) are conflated.
#[test]
fn test_same_local_name_different_namespaces() {
    use crate::schema::types::{ComplexType, ContentModel, ElementDef, TypeDef};

    use crate::namespace::Namespace;
    use crate::schema::types::NsName;

    const GML_NS: &str = "http://www.opengis.net/gml";
    const BRID_NS: &str = "http://www.opengis.net/citygml/bridge/2.0";

    let mut schema = CompiledSchema::new();
    schema
        .prefix_namespaces
        .insert("gml".to_string(), GML_NS.to_string());
    schema
        .prefix_namespaces
        .insert("brid".to_string(), BRID_NS.to_string());
    schema
        .namespace_prefixes
        .insert(GML_NS.to_string(), "gml".to_string());
    schema
        .namespace_prefixes
        .insert(BRID_NS.to_string(), "brid".to_string());

    // Define gml:BoundingShapeType with Choice(Envelope, Null)
    let mut gml_bounding_type = ComplexType::new("BoundingShapeType");
    gml_bounding_type.content = ContentModel::Choice(vec![
        ElementDef::new("Envelope").with_type("xs:string"),
        ElementDef::new("Null").with_type("xs:string"),
    ]);
    schema.types_ns.insert(
        NsName::new(GML_NS, "BoundingShapeType"),
        TypeDef::Complex(gml_bounding_type),
    );

    // Define brid:BridgeBoundedByType with Choice(WallSurface, RoofSurface)
    let mut brid_bounded_type = ComplexType::new("BridgeBoundedByType");
    brid_bounded_type.content = ContentModel::Choice(vec![
        ElementDef::new("WallSurface").with_type("xs:string"),
        ElementDef::new("RoofSurface").with_type("xs:string"),
    ]);
    schema.types_ns.insert(
        NsName::new(BRID_NS, "BridgeBoundedByType"),
        TypeDef::Complex(brid_bounded_type),
    );

    // Define gml:boundedBy element
    let mut gml_bounded_elem = ElementDef::new("boundedBy").with_type("gml:BoundingShapeType");
    gml_bounded_elem.type_ns = Some(NsName::new(GML_NS, "BoundingShapeType"));
    schema
        .elements_ns
        .insert(NsName::new(GML_NS, "boundedBy"), gml_bounded_elem);

    // Define brid:boundedBy element
    let mut brid_bounded_elem = ElementDef::new("boundedBy").with_type("brid:BridgeBoundedByType");
    brid_bounded_elem.type_ns = Some(NsName::new(BRID_NS, "BridgeBoundedByType"));
    schema
        .elements_ns
        .insert(NsName::new(BRID_NS, "boundedBy"), brid_bounded_elem);

    // Pre-populate the ns-keyed type children cache
    let gml_cache = FlattenedChildren::with_content_model(ContentModelType::Choice);
    schema.ns_type_children_cache.insert(
        NsName::new(GML_NS, "BoundingShapeType"),
        Arc::new({
            let mut f = gml_cache;
            f.constraints.insert("Envelope".to_string(), (0, Some(1)));
            f.constraints.insert("Null".to_string(), (0, Some(1)));
            f
        }),
    );

    let brid_cache = FlattenedChildren::with_content_model(ContentModelType::Choice);
    schema.ns_type_children_cache.insert(
        NsName::new(BRID_NS, "BridgeBoundedByType"),
        Arc::new({
            let mut f = brid_cache;
            f.constraints
                .insert("WallSurface".to_string(), (0, Some(1)));
            f.constraints
                .insert("RoofSurface".to_string(), (0, Some(1)));
            f
        }),
    );

    let mut validator = OnePassSchemaValidator::new(Arc::new(schema));

    let ns_decls = [
        Namespace::new("brid", BRID_NS),
        Namespace::new("gml", GML_NS),
    ];

    // Start brid:boundedBy (expects WallSurface or RoofSurface)
    validator
        .handle(&RawEvent::StartElement {
            name: "boundedBy",
            prefix: Some("brid"),
            attributes: &[],
            namespace_decls: &ns_decls,
            line: Some(1),
            column: Some(1),
        })
        .unwrap();

    // Add WallSurface (valid for brid:boundedBy)
    validator
        .handle(&RawEvent::StartElement {
            name: "WallSurface",
            prefix: Some("brid"),
            attributes: &[],
            namespace_decls: &[],
            line: Some(2),
            column: Some(1),
        })
        .unwrap();

    validator
        .handle(&RawEvent::EndElement {
            name: "WallSurface",
            prefix: Some("brid"),
        })
        .unwrap();

    // End brid:boundedBy
    validator
        .handle(&RawEvent::EndElement {
            name: "boundedBy",
            prefix: Some("brid"),
        })
        .unwrap();

    validator.finish().unwrap();

    // Should NOT have an error about missing 'Envelope' or 'Null'
    // because brid:boundedBy expects WallSurface/RoofSurface, not Envelope/Null
    let envelope_errors: Vec<_> = validator
        .errors()
        .iter()
        .filter(|e| e.message.contains("Envelope") || e.message.contains("Null"))
        .collect();

    assert!(
        envelope_errors.is_empty(),
        "brid:boundedBy should NOT require Envelope/Null (those are for gml:boundedBy). Got errors: {:?}",
        envelope_errors
    );

    assert!(
        validator.is_valid(),
        "Validation should pass for brid:boundedBy with WallSurface, but got errors: {:?}",
        validator.errors()
    );
}
