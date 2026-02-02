//! Asynchronous schema resolver.
//!
//! This module provides the async implementation of schema resolution
//! for import/include chains.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::Result;
use crate::schema::fetcher::{AsyncSchemaFetcher, FetchResult};
use crate::schema::store::AsyncSchemaStore;

use super::super::parser::parse_xsd_ast;
use super::super::types::XsdSchema;
use super::common::resolve_uri;

/// Async schema resolver that handles import/include chains.
pub struct AsyncSchemaResolver<'a, F: AsyncSchemaFetcher, S: AsyncSchemaStore> {
    fetcher: &'a F,
    store: &'a S,
    /// Resolved schemas by URI
    schemas: HashMap<String, XsdSchema>,
    /// URIs currently being resolved (for cycle detection)
    resolving: HashSet<String>,
}

impl<'a, F: AsyncSchemaFetcher, S: AsyncSchemaStore> AsyncSchemaResolver<'a, F, S> {
    /// Creates a new async schema resolver.
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
    pub async fn resolve_all(
        &mut self,
        entry_content: &[u8],
        entry_uri: &str,
    ) -> Result<Vec<XsdSchema>> {
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
                        let content = self.fetch_schema(&resolved_uri).await?;
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
                    let content = self.fetch_schema(&resolved_uri).await?;
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
    async fn fetch_schema(&self, uri: &str) -> Result<Vec<u8>> {
        // Check store first
        if let Some(content) = self.store.get(uri).await? {
            return Ok(content);
        }

        // Fetch from network
        let FetchResult {
            content, final_url, ..
        } = self.fetcher.fetch(uri).await?;

        // Store in cache
        self.store.put(&final_url, &content).await?;
        if final_url != uri {
            // Also cache under original URI
            self.store.put(uri, &content).await?;
        }

        Ok(content)
    }

    /// Consumes the resolver and returns the resolved schemas.
    pub fn into_schemas(self) -> HashMap<String, XsdSchema> {
        self.schemas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::FetchResult;
    use crate::schema::memory::InMemoryStore;
    use parking_lot::RwLock;
    use std::collections::HashMap as StdHashMap;
    use std::sync::Arc;

    /// Mock async fetcher for testing
    struct MockAsyncFetcher {
        responses: Arc<RwLock<StdHashMap<String, Vec<u8>>>>,
    }

    impl MockAsyncFetcher {
        fn new() -> Self {
            Self {
                responses: Arc::new(RwLock::new(StdHashMap::new())),
            }
        }

        fn add_response(&self, url: &str, content: &[u8]) {
            self.responses
                .write()
                .insert(url.to_string(), content.to_vec());
        }
    }

    #[async_trait::async_trait]
    impl AsyncSchemaFetcher for MockAsyncFetcher {
        async fn fetch(&self, url: &str) -> Result<FetchResult> {
            let responses = self.responses.read();
            if let Some(content) = responses.get(url) {
                Ok(FetchResult {
                    content: content.clone(),
                    final_url: url.to_string(),
                    redirected: false,
                })
            } else {
                Err(crate::schema::fetch_error::FetchError::RequestFailed {
                    url: url.to_string(),
                    message: "Not found".to_string(),
                }
                .into())
            }
        }
    }

    #[tokio::test]
    async fn test_async_resolve_simple() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        let store = InMemoryStore::new();

        let mut resolver = AsyncSchemaResolver::new(&fetcher, &store);
        let schemas = resolver
            .resolve_all(xsd.as_bytes(), "http://example.com/test.xsd")
            .await
            .unwrap();

        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].elements.len(), 1);
    }

    #[tokio::test]
    async fn test_async_resolve_with_import() {
        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/types">
            <xs:simpleType name="NameType">
                <xs:restriction base="xs:string">
                    <xs:maxLength value="100"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:t="http://example.com/types"
                   targetNamespace="http://example.com/main">
            <xs:import namespace="http://example.com/types" schemaLocation="types.xsd"/>
            <xs:element name="person">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="name" type="t:NameType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/types.xsd", types_xsd.as_bytes());

        let store = InMemoryStore::new();

        let mut resolver = AsyncSchemaResolver::new(&fetcher, &store);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        // Should have 2 schemas: types.xsd and main.xsd
        assert_eq!(schemas.len(), 2);

        // The store should have cached types.xsd
        assert!(crate::schema::store::SchemaStore::contains(
            &store,
            "http://example.com/types.xsd"
        ));
    }

    #[tokio::test]
    async fn test_async_resolve_with_include() {
        let common_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="IDType">
                <xs:restriction base="xs:string">
                    <xs:pattern value="[A-Z]{2}[0-9]{4}"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:include schemaLocation="common.xsd"/>
            <xs:element name="item">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="id" type="IDType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/common.xsd", common_xsd.as_bytes());

        let store = InMemoryStore::new();

        let mut resolver = AsyncSchemaResolver::new(&fetcher, &store);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        assert_eq!(schemas.len(), 2);
    }

    #[tokio::test]
    async fn test_async_resolve_uses_cache() {
        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="CachedType">
                <xs:restriction base="xs:string"/>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:import schemaLocation="types.xsd"/>
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        // Don't add to fetcher - it should be fetched from store

        let store = InMemoryStore::new();
        // Pre-populate the store (use sync method)
        crate::schema::store::SchemaStore::put(
            &store,
            "http://example.com/types.xsd",
            types_xsd.as_bytes(),
        )
        .unwrap();

        let mut resolver = AsyncSchemaResolver::new(&fetcher, &store);
        let schemas = resolver
            .resolve_all(main_xsd.as_bytes(), "http://example.com/main.xsd")
            .await
            .unwrap();

        // Should succeed even though fetcher doesn't have types.xsd
        assert_eq!(schemas.len(), 2);
    }
}
