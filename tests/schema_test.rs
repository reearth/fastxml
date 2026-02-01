//! Integration tests for schema validation.

use fastxml::schema::{
    CompiledSchema, ComplexType, ContentModel, ElementDef, InMemoryStore, SchemaStore, TempDirStore,
};

#[test]
fn test_tempdir_store() {
    let store = TempDirStore::new().unwrap();

    let uri = "http://example.com/schema.xsd";
    let content = b"<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"/>";

    // Store schema
    store.put(uri, content).unwrap();
    assert!(store.contains(uri));

    // Retrieve schema
    let retrieved = store.get(uri).unwrap().unwrap();
    assert_eq!(retrieved, content);

    // Get path
    let path = store.resolve_path(uri).unwrap();
    assert!(path.exists());

    // List schemas
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);

    // Remove schema
    assert!(store.remove(uri).unwrap());
    assert!(!store.contains(uri));
}

#[test]
fn test_memory_store() {
    let store = InMemoryStore::new();

    let uri = "http://example.com/schema.xsd";
    let content = b"<schema/>";

    store.put(uri, content).unwrap();
    assert!(store.contains(uri));
    assert_eq!(store.len(), 1);

    let retrieved = store.get(uri).unwrap().unwrap();
    assert_eq!(retrieved, content);

    store.clear().unwrap();
    assert!(store.is_empty());
}

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

#[test]
fn test_store_multiple_schemas() {
    let store = TempDirStore::new().unwrap();

    let schemas = vec![
        ("http://a.com/1.xsd", "schema1"),
        ("http://b.com/2.xsd", "schema2"),
        ("http://c.com/3.xsd", "schema3"),
    ];

    for (uri, content) in &schemas {
        store.put(uri, content.as_bytes()).unwrap();
    }

    assert_eq!(store.list().unwrap().len(), 3);

    for (uri, content) in &schemas {
        let retrieved = store.get(uri).unwrap().unwrap();
        assert_eq!(String::from_utf8(retrieved).unwrap(), *content);
    }
}
