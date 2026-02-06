//! Basic schema construction tests.

use fastxml::schema::{CompiledSchema, ComplexType, ContentModel, ElementDef};

#[test]
fn test_compiled_schema() {
    let mut schema = CompiledSchema::new();

    // Add an element
    let elem = ElementDef::new("Building")
        .with_type("BuildingType")
        .optional();
    schema.elements.insert("Building".to_string(), elem);

    // Verify lookup
    let found = schema.get_element("Building");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Building");
    assert_eq!(found.unwrap().min_occurs, 0);
}

#[test]
fn test_element_def() {
    let elem = ElementDef::new("TestElement")
        .with_type("xs:string")
        .with_occurs(0, None)
        .optional()
        .unbounded();

    assert_eq!(elem.name, "TestElement");
    assert_eq!(elem.type_ref, Some("xs:string".to_string()));
    assert_eq!(elem.min_occurs, 0);
    assert_eq!(elem.max_occurs, None); // unbounded
}

#[test]
fn test_complex_type() {
    let child1 = ElementDef::new("child1").with_type("xs:string");
    let child2 = ElementDef::new("child2").with_type("xs:integer");

    let complex = ComplexType::sequence("MyType", vec![child1, child2]);

    assert_eq!(complex.name, "MyType");
    if let ContentModel::Sequence(elements) = &complex.content {
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].name, "child1");
        assert_eq!(elements[1].name, "child2");
    } else {
        panic!("expected sequence content");
    }
}

#[test]
fn test_schema_with_namespace() {
    let schema = CompiledSchema::with_namespace("http://example.com/ns");
    assert_eq!(
        schema.target_namespace,
        Some("http://example.com/ns".to_string())
    );
}
