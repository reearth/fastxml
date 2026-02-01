//! XSD Import/Include resolver.
//!
//! This module handles resolving xs:import and xs:include dependencies,
//! fetching remote schemas, and caching them.

use std::collections::{HashMap, HashSet, VecDeque};

use url::Url;

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::fetcher::{FetchResult, SchemaFetcher};
use crate::schema::store::SchemaStore;

use super::parser::parse_xsd_ast;
use super::types::XsdSchema;

/// Schema resolver that handles import/include chains.
pub struct SchemaResolver<'a, F: SchemaFetcher, S: SchemaStore> {
    fetcher: &'a F,
    store: &'a S,
    /// Resolved schemas by URI
    schemas: HashMap<String, XsdSchema>,
    /// URIs currently being resolved (for cycle detection)
    resolving: HashSet<String>,
}

impl<'a, F: SchemaFetcher, S: SchemaStore> SchemaResolver<'a, F, S> {
    /// Creates a new schema resolver.
    pub fn new(fetcher: &'a F, store: &'a S) -> Self {
        Self {
            fetcher,
            store,
            schemas: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    /// Resolves all dependencies starting from an entry schema.
    ///
    /// Returns all resolved schemas in dependency order (dependencies first).
    pub fn resolve_all(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<Vec<XsdSchema>> {
        // Parse the entry schema
        let entry_schema = parse_xsd_ast(entry_content)?;

        // Store and track the entry
        self.schemas.insert(entry_uri.to_string(), entry_schema);

        // Use BFS to resolve all dependencies
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(entry_uri.to_string());

        while let Some(current_uri) = queue.pop_front() {
            if self.resolving.contains(&current_uri) {
                return Err(SchemaError::CircularDependency { uri: current_uri }.into());
            }
            self.resolving.insert(current_uri.clone());

            // Get imports and includes from the current schema
            let (imports, includes) = {
                let schema = self.schemas.get(&current_uri).ok_or_else(|| {
                    crate::schema::error::SchemaError::SchemaNotFound {
                        uri: current_uri.clone(),
                    }
                })?;
                (schema.imports.clone(), schema.includes.clone())
            };

            // Process imports
            for import in imports {
                if let Some(location) = &import.schema_location {
                    let resolved_uri = resolve_uri(&current_uri, location)?;

                    if !self.schemas.contains_key(&resolved_uri) {
                        let content = self.fetch_schema(&resolved_uri)?;
                        let schema = parse_xsd_ast(&content)?;
                        self.schemas.insert(resolved_uri.clone(), schema);
                        queue.push_back(resolved_uri);
                    }
                }
            }

            // Process includes
            for include in includes {
                let resolved_uri = resolve_uri(&current_uri, &include.schema_location)?;

                if !self.schemas.contains_key(&resolved_uri) {
                    let content = self.fetch_schema(&resolved_uri)?;
                    let schema = parse_xsd_ast(&content)?;
                    self.schemas.insert(resolved_uri.clone(), schema);
                    queue.push_back(resolved_uri);
                }
            }

            self.resolving.remove(&current_uri);
        }

        // Return schemas in order (entry last for easier compilation)
        let mut result: Vec<XsdSchema> = Vec::new();

        // First add all non-entry schemas
        for (uri, schema) in &self.schemas {
            if uri != entry_uri {
                result.push(schema.clone());
            }
        }

        // Add entry schema last
        if let Some(entry) = self.schemas.remove(entry_uri) {
            result.push(entry);
        }

        Ok(result)
    }

    /// Fetches a schema, first checking the store cache.
    fn fetch_schema(&self, uri: &str) -> Result<Vec<u8>> {
        // Check store first
        if let Some(content) = self.store.get(uri)? {
            return Ok(content);
        }

        // Fetch from network
        let FetchResult {
            content, final_url, ..
        } = self.fetcher.fetch(uri)?;

        // Store in cache
        self.store.put(&final_url, &content)?;
        if final_url != uri {
            // Also cache under original URI
            self.store.put(uri, &content)?;
        }

        Ok(content)
    }

    /// Consumes the resolver and returns the resolved schemas.
    pub fn into_schemas(self) -> HashMap<String, XsdSchema> {
        self.schemas
    }
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
    let base_url =
        Url::parse(base).map_err(|e| crate::schema::error::SchemaError::InvalidBaseUri {
            uri: base.to_string(),
            message: e.to_string(),
        })?;

    // Resolve relative URL
    let resolved = base_url.join(relative).map_err(|e| {
        crate::schema::error::SchemaError::UrlResolutionFailed {
            relative: relative.to_string(),
            base: base.to_string(),
            message: e.to_string(),
        }
    })?;

    Ok(resolved.to_string())
}

/// Resolves schemas from content without network access.
///
/// This is useful for testing or when all schemas are provided inline.
pub fn resolve_schemas_from_content(schemas: &[(&str, &[u8])]) -> Result<Vec<XsdSchema>> {
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
            imports: vec![super::super::types::XsdImport {
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
}
