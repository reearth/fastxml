//! Tests for compiled schema lookups.

use super::*;

#[test]
fn test_get_element_with_local_name() {
    let mut schema = CompiledSchema::new();
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeature"),
        ElementDef::new("ReliefFeature"),
    );

    // Local name lookup should work
    assert!(schema.get_element("ReliefFeature").is_some());
}

#[test]
fn test_get_element_with_qualified_name() {
    let mut schema = CompiledSchema::new();
    schema.elements_ns.insert(
        crate::schema::types::NsName::new("", "ReliefFeature"),
        ElementDef::new("ReliefFeature"),
    );

    // Qualified name lookup should fall back to local name
    assert!(
        schema.get_element("dem:ReliefFeature").is_some(),
        "Should find 'ReliefFeature' when looking up 'dem:ReliefFeature'"
    );
}

#[test]
fn test_get_type_with_local_name() {
    let mut schema = CompiledSchema::new();
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractCityObjectType"),
        TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
    );

    // Local name lookup should work
    assert!(schema.get_type("AbstractCityObjectType").is_some());
}

#[test]
fn test_get_type_with_qualified_name() {
    let mut schema = CompiledSchema::new();
    schema.types_ns.insert(
        crate::schema::types::NsName::new("", "AbstractCityObjectType"),
        TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
    );

    // Qualified name lookup should fall back to local name
    assert!(
        schema.get_type("core:AbstractCityObjectType").is_some(),
        "Should find 'AbstractCityObjectType' when looking up 'core:AbstractCityObjectType'"
    );
}

#[test]
fn test_get_type_not_found() {
    let schema = CompiledSchema::new();
    assert!(schema.get_type("NonExistentType").is_none());
    assert!(schema.get_type("prefix:NonExistentType").is_none());
}

#[test]
fn test_get_element_not_found() {
    let schema = CompiledSchema::new();
    assert!(schema.get_element("NonExistentElement").is_none());
    assert!(schema.get_element("prefix:NonExistentElement").is_none());
}

// === Namespace-qualified accessor semantics (C3) ===

/// Two globals sharing a local name in different namespaces must not
/// collide on the namespace-keyed map (the wildG031 class).
#[test]
fn ns_maps_resolve_same_local_in_different_namespaces() {
    const NS_A: &str = "http://example.com/a";
    const NS_B: &str = "http://example.com/b";
    let mut schema = CompiledSchema::new();
    let mut a = ElementDef::new("value");
    a.type_ref = Some("a:AType".into());
    let mut b = ElementDef::new("value");
    b.type_ref = Some("b:BType".into());
    schema.elements_ns.insert(NsName::new(NS_A, "value"), a);
    schema.elements_ns.insert(NsName::new(NS_B, "value"), b);

    // Each namespace resolves to its OWN declaration, not the other's.
    assert_eq!(
        schema
            .element_ns(NS_A, "value")
            .unwrap()
            .type_ref
            .as_deref(),
        Some("a:AType")
    );
    assert_eq!(
        schema
            .element_ns(NS_B, "value")
            .unwrap()
            .type_ref
            .as_deref(),
        Some("b:BType")
    );
}

/// A cleanly-resolved namespace that misses (with no chameleon `""`
/// entry) must stay a miss — `element_ns` must NOT scan other namespaces
/// (amendment #1: the any-ns scan can only hide "not declared" errors).
#[test]
fn ns_element_lookup_does_not_leak_across_namespaces() {
    const NS_A: &str = "http://example.com/a";
    const NS_C: &str = "http://example.com/c";
    let mut schema = CompiledSchema::new();
    schema
        .elements_ns
        .insert(NsName::new(NS_A, "value"), ElementDef::new("value"));

    // Wrong namespace -> strict miss (no cross-namespace leak).
    assert!(schema.element_ns(NS_C, "value").is_none());
    // The leniency scan (only for unresolvable instance namespaces) DOES
    // find it by local name.
    assert!(schema.element_ns_any("value").is_some());
}

/// Chameleon-include fallback: a component registered under the
/// no-namespace `""` is reachable from a qualified lookup.
#[test]
fn ns_type_lookup_chameleon_fallback() {
    const NS_A: &str = "http://example.com/a";
    let mut schema = CompiledSchema::new();
    schema.types_ns.insert(
        NsName::new("", "ChameleonType"),
        TypeDef::Complex(ComplexType::new("ChameleonType")),
    );
    assert!(schema.type_ns(NS_A, "ChameleonType").is_some());
    assert!(schema.type_ns("", "ChameleonType").is_some());
    assert!(schema.type_ns(NS_A, "Missing").is_none());
}
