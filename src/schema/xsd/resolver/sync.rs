//! Synchronous schema resolver.
//!
//! This module provides the synchronous implementation of schema resolution
//! for import/include chains.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::Result;
use crate::schema::fetcher::SchemaFetcher;

use super::super::parser::parse_xsd_ast;
use super::super::types::XsdSchema;
use super::common::resolve_uri;

/// Schema resolver that handles import/include chains.
pub struct SchemaResolver<'a, F: SchemaFetcher> {
    fetcher: &'a F,
    /// Resolved schemas by URI
    schemas: HashMap<String, XsdSchema>,
    /// URIs currently being resolved (for cycle detection)
    resolving: HashSet<String>,
}

impl<'a, F: SchemaFetcher> SchemaResolver<'a, F> {
    /// Creates a new schema resolver.
    pub fn new(fetcher: &'a F) -> Self {
        Self {
            fetcher,
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
                return Err(crate::schema::error::SchemaError::CircularDependency {
                    uri: current_uri,
                }
                .into());
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

    /// Fetches a schema via the fetcher (caching is handled by the fetcher).
    fn fetch_schema(&self, uri: &str) -> Result<Vec<u8>> {
        let result = self.fetcher.fetch(uri)?;
        Ok(result.content)
    }

    /// Resolves an entry schema and accumulates it along with its dependencies.
    ///
    /// Unlike [`Self::resolve_all`], this method does not return schemas immediately.
    /// Instead, it accumulates them internally so that multiple entry schemas
    /// can share resolved dependencies (avoiding duplicate fetches).
    ///
    /// Call [`Self::take_all_schemas`] after all entries have been resolved.
    ///
    /// # Arguments
    ///
    /// * `entry_content` - The entry XSD file content as bytes
    /// * `entry_uri` - URI for the entry schema (used for resolving relative imports)
    pub fn resolve_entry(&mut self, entry_content: &[u8], entry_uri: &str) -> Result<()> {
        // Skip if already resolved
        if self.schemas.contains_key(entry_uri) {
            return Ok(());
        }

        // Parse the entry schema
        let entry_schema = parse_xsd_ast(entry_content)?;

        // Store and track the entry
        self.schemas.insert(entry_uri.to_string(), entry_schema);

        // Use BFS to resolve all dependencies
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(entry_uri.to_string());

        while let Some(current_uri) = queue.pop_front() {
            if self.resolving.contains(&current_uri) {
                return Err(crate::schema::error::SchemaError::CircularDependency {
                    uri: current_uri,
                }
                .into());
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

        Ok(())
    }

    /// Consumes the resolver and returns all accumulated schemas as a Vec.
    ///
    /// Use this after calling [`Self::resolve_entry`] one or more times to get
    /// all resolved schemas for compilation.
    pub fn take_all_schemas(self) -> Vec<XsdSchema> {
        self.schemas.into_values().collect()
    }

    /// Consumes the resolver and returns the resolved schemas.
    pub fn into_schemas(self) -> HashMap<String, XsdSchema> {
        self.schemas
    }
}
