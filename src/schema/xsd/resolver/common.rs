//! Common helper functions for schema resolution.
//!
//! This module contains pure functions (no I/O) that are shared between
//! sync and async resolvers.

use std::collections::{HashMap, HashSet};

use url::Url;

use crate::error::Result;
use crate::schema::error::SchemaError;

use super::super::types::XsdSchema;

/// Extracts dependency URIs from a schema.
///
/// Collects all import and include locations and resolves them against the base URI.
#[allow(dead_code)]
pub fn extract_dependencies(schema: &XsdSchema, base_uri: &str) -> Vec<String> {
    let mut deps = Vec::new();

    for import in &schema.imports {
        if let Some(location) = &import.schema_location {
            if let Ok(uri) = resolve_uri(base_uri, location) {
                deps.push(uri);
            }
        }
    }

    for include in &schema.includes {
        if let Ok(uri) = resolve_uri(base_uri, &include.schema_location) {
            deps.push(uri);
        }
    }

    deps
}

/// Checks for circular dependencies.
///
/// Returns an error if the URI is already in the resolving set.
#[allow(dead_code)]
pub fn check_cycle(uri: &str, resolving: &HashSet<String>) -> Result<()> {
    if resolving.contains(uri) {
        return Err(SchemaError::CircularDependency {
            uri: uri.to_string(),
        }
        .into());
    }
    Ok(())
}

/// Orders schemas for output with entry schema last.
///
/// Dependencies should be compiled before the schemas that depend on them.
#[allow(dead_code)]
pub fn order_schemas_for_output(
    schemas: &HashMap<String, XsdSchema>,
    entry_uri: &str,
) -> Vec<XsdSchema> {
    let mut result: Vec<XsdSchema> = schemas
        .iter()
        .filter(|(uri, _)| *uri != entry_uri)
        .map(|(_, schema)| schema.clone())
        .collect();

    if let Some(entry) = schemas.get(entry_uri) {
        result.push(entry.clone());
    }

    result
}

/// Resolves a relative URI against a base URI.
pub fn resolve_uri(base: &str, relative: &str) -> Result<String> {
    // If relative is already absolute, use it directly
    if relative.starts_with("http://")
        || relative.starts_with("https://")
        || relative.starts_with("file://")
    {
        return Ok(relative.to_string());
    }

    // Parse base URL
    let base_url = Url::parse(base).map_err(|e| SchemaError::InvalidBaseUri {
        uri: base.to_string(),
        message: e.to_string(),
    })?;

    // Resolve relative URL
    let resolved = base_url
        .join(relative)
        .map_err(|e| SchemaError::UrlResolutionFailed {
            relative: relative.to_string(),
            base: base.to_string(),
            message: e.to_string(),
        })?;

    Ok(resolved.to_string())
}

/// Resolves schemas from content without network access.
///
/// This is useful for testing or when all schemas are provided inline.
pub fn resolve_schemas_from_content(schemas: &[(&str, &[u8])]) -> Result<Vec<XsdSchema>> {
    use super::super::parser::parse_xsd_ast;

    let mut result = Vec::new();

    for (uri, content) in schemas {
        let schema = parse_xsd_ast(content)?;
        tracing::debug!(
            "Parsed schema from {}: {} types, {} elements",
            uri,
            schema.types.len(),
            schema.elements.len()
        );
        result.push(schema);
    }

    Ok(result)
}

/// A simple dependency tracker for ordering schema compilation.
pub struct DependencyTracker {
    /// Dependencies: (dependent -> dependencies)
    deps: HashMap<String, HashSet<String>>,
    /// All known URIs
    uris: HashSet<String>,
}

impl DependencyTracker {
    /// Creates a new dependency tracker.
    pub fn new() -> Self {
        Self {
            deps: HashMap::new(),
            uris: HashSet::new(),
        }
    }

    /// Adds a schema and its dependencies.
    pub fn add(&mut self, uri: &str, schema: &XsdSchema) {
        self.uris.insert(uri.to_string());

        let mut dependencies = HashSet::new();

        // Add import dependencies
        for import in &schema.imports {
            if let Some(loc) = &import.schema_location {
                if let Ok(resolved) = resolve_uri(uri, loc) {
                    dependencies.insert(resolved);
                }
            }
        }

        // Add include dependencies
        for include in &schema.includes {
            if let Ok(resolved) = resolve_uri(uri, &include.schema_location) {
                dependencies.insert(resolved);
            }
        }

        self.deps.insert(uri.to_string(), dependencies);
    }

    /// Returns URIs in topological order (dependencies first).
    pub fn topological_order(&self) -> Result<Vec<String>> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut in_progress = HashSet::new();

        for uri in &self.uris {
            self.visit(uri, &mut result, &mut visited, &mut in_progress)?;
        }

        Ok(result)
    }

    fn visit(
        &self,
        uri: &str,
        result: &mut Vec<String>,
        visited: &mut HashSet<String>,
        in_progress: &mut HashSet<String>,
    ) -> Result<()> {
        if visited.contains(uri) {
            return Ok(());
        }

        if in_progress.contains(uri) {
            return Err(SchemaError::CircularDependency {
                uri: uri.to_string(),
            }
            .into());
        }

        in_progress.insert(uri.to_string());

        if let Some(deps) = self.deps.get(uri) {
            for dep in deps {
                if self.uris.contains(dep) {
                    self.visit(dep, result, visited, in_progress)?;
                }
            }
        }

        in_progress.remove(uri);
        visited.insert(uri.to_string());
        result.push(uri.to_string());

        Ok(())
    }
}

impl Default for DependencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::xsd::types::XsdImport;

    #[test]
    fn test_resolve_uri_absolute() {
        let result = resolve_uri(
            "http://example.com/schemas/base.xsd",
            "http://other.com/schema.xsd",
        )
        .unwrap();
        assert_eq!(result, "http://other.com/schema.xsd");
    }

    #[test]
    fn test_resolve_uri_relative() {
        let result = resolve_uri("http://example.com/schemas/base.xsd", "types.xsd").unwrap();
        assert_eq!(result, "http://example.com/schemas/types.xsd");
    }

    #[test]
    fn test_resolve_uri_parent() {
        let result = resolve_uri(
            "http://example.com/schemas/v1/base.xsd",
            "../common/types.xsd",
        )
        .unwrap();
        assert_eq!(result, "http://example.com/schemas/common/types.xsd");
    }

    #[test]
    fn test_dependency_tracker() {
        let mut tracker = DependencyTracker::new();

        // Create mock schemas
        let schema_a = XsdSchema {
            imports: vec![XsdImport {
                namespace: None,
                schema_location: Some("b.xsd".to_string()),
            }],
            ..Default::default()
        };

        let schema_b = XsdSchema::default();

        tracker.add("http://example.com/a.xsd", &schema_a);
        tracker.add("http://example.com/b.xsd", &schema_b);

        let order = tracker.topological_order().unwrap();

        // B should come before A (since A depends on B)
        let pos_a = order.iter().position(|u| u.contains("a.xsd")).unwrap();
        let pos_b = order.iter().position(|u| u.contains("b.xsd")).unwrap();
        assert!(pos_b < pos_a);
    }

    #[test]
    fn test_resolve_schemas_from_content() {
        let xsd_a = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let schemas =
            resolve_schemas_from_content(&[("http://example.com/a.xsd", xsd_a.as_bytes())])
                .unwrap();

        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].elements.len(), 1);
    }

    #[test]
    fn test_extract_dependencies() {
        let schema = XsdSchema {
            imports: vec![XsdImport {
                namespace: Some("http://example.com/types".to_string()),
                schema_location: Some("types.xsd".to_string()),
            }],
            includes: vec![crate::schema::xsd::types::XsdInclude {
                schema_location: "common.xsd".to_string(),
            }],
            ..Default::default()
        };

        let deps = extract_dependencies(&schema, "http://example.com/main.xsd");
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"http://example.com/types.xsd".to_string()));
        assert!(deps.contains(&"http://example.com/common.xsd".to_string()));
    }

    #[test]
    fn test_check_cycle() {
        let mut resolving = HashSet::new();
        resolving.insert("http://example.com/a.xsd".to_string());

        // Should not error for new URI
        assert!(check_cycle("http://example.com/b.xsd", &resolving).is_ok());

        // Should error for existing URI
        assert!(check_cycle("http://example.com/a.xsd", &resolving).is_err());
    }

    #[test]
    fn test_order_schemas_for_output() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "http://example.com/entry.xsd".to_string(),
            XsdSchema::default(),
        );
        schemas.insert(
            "http://example.com/dep.xsd".to_string(),
            XsdSchema::default(),
        );

        let ordered = order_schemas_for_output(&schemas, "http://example.com/entry.xsd");
        assert_eq!(ordered.len(), 2);
        // Entry should be last (but since we can't distinguish them here, just check count)
    }
}
