//! XSD Schema Parser and Compiler.
//!
//! This module provides functionality to parse XSD (XML Schema Definition) files
//! and compile them into the runtime validation representation (CompiledSchema).
//!
//! # Overview
//!
//! The XSD processing pipeline consists of:
//!
//! 1. **Parsing**: XSD XML is parsed into an AST (Abstract Syntax Tree) representation
//! 2. **Resolution**: Import and include dependencies are resolved and fetched
//! 3. **Compilation**: The AST is compiled into a CompiledSchema for validation
//!
//! # Example
//!
//! ```ignore
//! use fastxml::schema::xsd::{parse_xsd, parse_xsd_with_imports};
//! use fastxml::schema::UreqFetcher;
//!
//! // Simple parsing (no import resolution)
//! let xsd_content = r#"
//!     <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
//!         <xs:element name="root" type="xs:string"/>
//!     </xs:schema>
//! "#;
//! let schema = parse_xsd(xsd_content.as_bytes())?;
//!
//! // With import resolution
//! let fetcher = UreqFetcher::new();
//! let schema = parse_xsd_with_imports(
//!     xsd_content.as_bytes(),
//!     "http://example.com/schema.xsd",
//!     &fetcher,
//! )?;
//! ```
//!
//! # PLATEAU/CityGML Support
//!
//! This module includes built-in support for:
//! - XSD primitive types (xs:string, xs:integer, etc.)
//! - GML types (gml:CodeType, gml:MeasureType, geometry types)
//! - CityGML/PLATEAU schema patterns

pub mod builtin;
pub mod compiler;
pub mod constraints;
pub mod content_model;
pub mod error;
pub mod facets;
pub mod parser;
pub mod resolver;
pub mod types;

use crate::error::Result;
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::types::CompiledSchema;

#[cfg(feature = "tokio")]
use crate::schema::fetcher::AsyncSchemaFetcher;

pub use builtin::{gml, register_builtin_types, xs};
pub use compiler::{XsdCompiler, compile_schemas};
pub use parser::{XSD_NAMESPACE, XsdParser, parse_xsd_ast};
pub use resolver::{SchemaResolver, resolve_uri};

#[cfg(feature = "tokio")]
pub use resolver::AsyncSchemaResolver;
pub use types::*;

/// Parses XSD content and compiles it into a CompiledSchema.
///
/// This is the simplest entry point for XSD parsing. It does not resolve
/// import/include dependencies - those will be missing from the result.
///
/// # Arguments
///
/// * `content` - The XSD file content as bytes
///
/// # Returns
///
/// A compiled schema ready for validation
///
/// # Example
///
/// ```ignore
/// let xsd = r#"
///     <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
///         <xs:element name="root" type="xs:string"/>
///     </xs:schema>
/// "#;
/// let schema = parse_xsd(xsd.as_bytes())?;
/// assert!(schema.elements.contains_key("root"));
/// ```
pub fn parse_xsd(content: &[u8]) -> Result<CompiledSchema> {
    // Parse AST
    let ast = parse_xsd_ast(content)?;

    // Compile
    let mut schema = compile_schemas(vec![ast])?;

    // Register built-in types
    register_builtin_types(&mut schema);

    Ok(schema)
}

/// Parses XSD content with import/include resolution.
///
/// This function resolves all xs:import and xs:include dependencies,
/// fetching remote schemas as needed. Caching is handled by the fetcher
/// (use `CachingFetcher` to add caching to any fetcher).
///
/// # Arguments
///
/// * `content` - The entry XSD file content as bytes
/// * `base_uri` - Base URI for resolving relative imports
/// * `fetcher` - Schema fetcher for downloading remote schemas
///
/// # Returns
///
/// A compiled schema with all dependencies resolved
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::UreqFetcher;
///
/// let fetcher = UreqFetcher::new();
///
/// let schema = parse_xsd_with_imports(
///     xsd_content.as_bytes(),
///     "http://example.com/schemas/main.xsd",
///     &fetcher,
/// )?;
/// ```
pub fn parse_xsd_with_imports<F: SchemaFetcher>(
    content: &[u8],
    base_uri: &str,
    fetcher: &F,
) -> Result<CompiledSchema> {
    // Create resolver and resolve all dependencies
    let mut resolver = SchemaResolver::new(fetcher);
    let schemas = resolver.resolve_all(content, base_uri)?;

    // Compile all schemas
    let mut schema = compile_schemas(schemas)?;

    // Register built-in types
    register_builtin_types(&mut schema);

    Ok(schema)
}

/// Parses XSD content with import/include resolution asynchronously.
///
/// This is the async version of [`parse_xsd_with_imports`]. It resolves all
/// xs:import and xs:include dependencies, fetching remote schemas as needed.
/// Caching is handled by the fetcher (use `AsyncCachingFetcher` to add caching).
///
/// # Arguments
///
/// * `content` - The entry XSD file content as bytes
/// * `base_uri` - Base URI for resolving relative imports
/// * `fetcher` - Async schema fetcher for downloading remote schemas
///
/// # Returns
///
/// A compiled schema with all dependencies resolved
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::{AsyncDefaultFetcher, parse_xsd_with_imports_async};
///
/// let fetcher = AsyncDefaultFetcher::new()?;
///
/// let schema = parse_xsd_with_imports_async(
///     xsd_content.as_bytes(),
///     "http://example.com/schemas/main.xsd",
///     &fetcher,
/// ).await?;
/// ```
#[cfg(feature = "tokio")]
pub async fn parse_xsd_with_imports_async<F: AsyncSchemaFetcher>(
    content: &[u8],
    base_uri: &str,
    fetcher: &F,
) -> Result<CompiledSchema> {
    // Create async resolver and resolve all dependencies
    let mut resolver = AsyncSchemaResolver::new(fetcher);
    let schemas = resolver.resolve_all(content, base_uri).await?;

    // Compile all schemas
    let mut schema = compile_schemas(schemas)?;

    // Register built-in types
    register_builtin_types(&mut schema);

    Ok(schema)
}

/// Parses multiple XSD contents and compiles them together.
///
/// This is useful when you have all schema files available locally
/// and don't need network resolution.
///
/// # Arguments
///
/// * `contents` - List of (URI, content) pairs
///
/// # Returns
///
/// A compiled schema containing all definitions
///
/// # Example
///
/// ```ignore
/// let schemas = vec![
///     ("http://example.com/types.xsd", types_xsd.as_bytes()),
///     ("http://example.com/main.xsd", main_xsd.as_bytes()),
/// ];
/// let compiled = parse_xsd_multiple(&schemas)?;
/// ```
pub fn parse_xsd_multiple(contents: &[(&str, &[u8])]) -> Result<CompiledSchema> {
    let schemas = resolver::resolve_schemas_from_content(contents)?;
    let mut schema = compile_schemas(schemas)?;
    register_builtin_types(&mut schema);
    Ok(schema)
}

/// Parses multiple XSD entry schemas with shared import/include resolution.
///
/// This function uses a single resolver to resolve all entries, so shared
/// dependencies (e.g., GML base schemas) are only fetched and parsed once.
/// This is the recommended approach when `xsi:schemaLocation` specifies
/// multiple schema entries.
///
/// # Arguments
///
/// * `entries` - List of (URI, content) pairs for entry schemas
/// * `fetcher` - Schema fetcher for downloading remote schemas
///
/// # Returns
///
/// A compiled schema with all entries and their dependencies resolved
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::{UreqFetcher, parse_xsd_with_imports_multiple};
///
/// let fetcher = UreqFetcher::new();
///
/// let schema = parse_xsd_with_imports_multiple(
///     &[
///         ("http://example.com/a.xsd", a_content),
///         ("http://example.com/b.xsd", b_content),
///     ],
///     &fetcher,
/// )?;
/// ```
pub fn parse_xsd_with_imports_multiple<F: SchemaFetcher>(
    entries: &[(&str, &[u8])],
    fetcher: &F,
) -> Result<CompiledSchema> {
    let mut resolver = SchemaResolver::new(fetcher);
    for (uri, content) in entries {
        resolver.resolve_entry(content, uri)?;
    }
    let schemas = resolver.take_all_schemas();
    let mut schema = compile_schemas(schemas)?;
    register_builtin_types(&mut schema);
    Ok(schema)
}

/// Parses multiple XSD entry schemas with shared import/include resolution (async).
///
/// This is the async version of [`parse_xsd_with_imports_multiple`].
///
/// # Arguments
///
/// * `entries` - List of (URI, content) pairs for entry schemas
/// * `fetcher` - Async schema fetcher for downloading remote schemas
///
/// # Returns
///
/// A compiled schema with all entries and their dependencies resolved
#[cfg(feature = "tokio")]
pub async fn parse_xsd_with_imports_multiple_async<F: AsyncSchemaFetcher>(
    entries: &[(&str, &[u8])],
    fetcher: &F,
) -> Result<CompiledSchema> {
    let mut resolver = AsyncSchemaResolver::new(fetcher);
    for (uri, content) in entries {
        resolver.resolve_entry(content, uri).await?;
    }
    let schemas = resolver.take_all_schemas();
    let mut schema = compile_schemas(schemas)?;
    register_builtin_types(&mut schema);
    Ok(schema)
}

/// Creates a CompiledSchema with only built-in types registered.
///
/// This is useful for validation that only needs primitive XSD and GML types
/// without parsing any custom schemas.
pub fn create_builtin_schema() -> CompiledSchema {
    let mut schema = CompiledSchema::new();
    register_builtin_types(&mut schema);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xsd_simple() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/test">
            <xs:element name="root" type="xs:string"/>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
        assert!(schema.elements.contains_key("root"));
        assert_eq!(
            schema.target_namespace,
            Some("http://example.com/test".to_string())
        );
    }

    #[test]
    fn test_parse_xsd_with_complex_type() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="person" type="PersonType"/>
            <xs:complexType name="PersonType">
                <xs:sequence>
                    <xs:element name="name" type="xs:string"/>
                    <xs:element name="age" type="xs:integer"/>
                </xs:sequence>
            </xs:complexType>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
        assert!(schema.elements.contains_key("person"));
        assert!(schema.types.contains_key("PersonType"));
    }

    #[test]
    fn test_parse_xsd_with_simple_type() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="StatusType">
                <xs:restriction base="xs:string">
                    <xs:enumeration value="active"/>
                    <xs:enumeration value="inactive"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();
        assert!(schema.types.contains_key("StatusType"));
    }

    #[test]
    fn test_builtin_types_registered() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="test" type="xs:string"/>
        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();

        // XSD built-in types should be registered
        assert!(schema.types.contains_key("xs:string"));
        assert!(schema.types.contains_key("xs:integer"));
        assert!(schema.types.contains_key("string"));
        assert!(schema.types.contains_key("integer"));

        // GML types should be registered
        assert!(schema.types.contains_key("gml:CodeType"));
        assert!(schema.types.contains_key("gml:MeasureType"));
    }

    #[test]
    fn test_create_builtin_schema() {
        let schema = create_builtin_schema();

        // Should have XSD types
        assert!(schema.types.contains_key("xs:string"));
        assert!(schema.types.contains_key("xs:integer"));
        assert!(schema.types.contains_key("xs:double"));
        assert!(schema.types.contains_key("xs:dateTime"));

        // Should have GML types
        assert!(schema.types.contains_key("gml:CodeType"));
        assert!(schema.types.contains_key("gml:MeasureType"));
        assert!(schema.types.contains_key("gml:PointType"));
    }

    #[test]
    fn test_parse_xsd_multiple() {
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
                   targetNamespace="http://example.com/main">
            <xs:element name="root" type="xs:string"/>
        </xs:schema>"#;

        let schema = parse_xsd_multiple(&[
            ("http://example.com/types.xsd", types_xsd.as_bytes()),
            ("http://example.com/main.xsd", main_xsd.as_bytes()),
        ])
        .unwrap();

        assert!(schema.types.contains_key("NameType"));
        assert!(schema.elements.contains_key("root"));
    }

    #[test]
    fn test_parse_citygml_like_schema() {
        // A schema that mimics CityGML patterns
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:gml="http://www.opengis.net/gml/3.2"
                   targetNamespace="http://example.com/building">

            <xs:element name="Building" type="BuildingType"/>

            <xs:complexType name="BuildingType">
                <xs:complexContent>
                    <xs:extension base="gml:AbstractFeatureType">
                        <xs:sequence>
                            <xs:element name="class" type="gml:CodeType" minOccurs="0"/>
                            <xs:element name="function" type="gml:CodeType" minOccurs="0" maxOccurs="unbounded"/>
                            <xs:element name="usage" type="gml:CodeType" minOccurs="0" maxOccurs="unbounded"/>
                            <xs:element name="measuredHeight" type="gml:LengthType" minOccurs="0"/>
                        </xs:sequence>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>

        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();

        assert!(schema.elements.contains_key("Building"));
        assert!(schema.types.contains_key("BuildingType"));

        // Built-in GML types should be available
        assert!(schema.types.contains_key("gml:CodeType"));
        assert!(schema.types.contains_key("gml:LengthType"));
        assert!(schema.types.contains_key("gml:AbstractFeatureType"));
    }

    #[test]
    fn test_simple_content_restriction_should_be_simple_content() {
        // This tests that simpleContent/restriction is correctly parsed as SimpleContent,
        // not ComplexExtension. This is the pattern used by gml:LengthType.
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:test="http://test.example.com"
                   targetNamespace="http://test.example.com">

            <!-- Base type with simpleContent/extension -->
            <xs:complexType name="MeasureType">
                <xs:simpleContent>
                    <xs:extension base="xs:double">
                        <xs:attribute name="uom" type="xs:anyURI" use="required"/>
                    </xs:extension>
                </xs:simpleContent>
            </xs:complexType>

            <!-- Derived type with simpleContent/restriction -->
            <xs:complexType name="LengthType">
                <xs:simpleContent>
                    <xs:restriction base="test:MeasureType"/>
                </xs:simpleContent>
            </xs:complexType>

        </xs:schema>"#;

        let schema = parse_xsd(xsd.as_bytes()).unwrap();

        // MeasureType should be SimpleContent
        let measure_type = schema
            .types
            .get("MeasureType")
            .expect("MeasureType not found");
        if let crate::schema::types::TypeDef::Complex(ct) = measure_type {
            assert!(
                matches!(
                    ct.content,
                    crate::schema::types::ContentModel::SimpleContent { .. }
                ),
                "MeasureType should be SimpleContent, got {:?}",
                ct.content
            );
        } else {
            panic!("MeasureType should be ComplexType");
        }

        // LengthType should also be SimpleContent (not ComplexExtension!)
        let length_type = schema
            .types
            .get("LengthType")
            .expect("LengthType not found");
        if let crate::schema::types::TypeDef::Complex(ct) = length_type {
            assert!(
                matches!(
                    ct.content,
                    crate::schema::types::ContentModel::SimpleContent { .. }
                ),
                "LengthType should be SimpleContent, got {:?}",
                ct.content
            );
        } else {
            panic!("LengthType should be ComplexType");
        }
    }
}

#[cfg(all(test, feature = "tokio"))]
mod async_tests {
    use super::*;
    use crate::error::Result;
    use crate::schema::fetcher::{AsyncSchemaFetcher, FetchResult};
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Mock async fetcher for testing
    struct MockAsyncFetcher {
        responses: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    }

    impl MockAsyncFetcher {
        fn new() -> Self {
            Self {
                responses: Arc::new(RwLock::new(HashMap::new())),
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
    async fn test_parse_xsd_with_imports_async_simple() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/test">
            <xs:element name="root" type="xs:string"/>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();

        let schema =
            parse_xsd_with_imports_async(xsd.as_bytes(), "http://example.com/test.xsd", &fetcher)
                .await
                .unwrap();

        assert!(schema.elements.contains_key("root"));
        assert_eq!(
            schema.target_namespace,
            Some("http://example.com/test".to_string())
        );

        // Built-in types should be registered
        assert!(schema.types.contains_key("xs:string"));
        assert!(schema.types.contains_key("gml:CodeType"));
    }

    #[tokio::test]
    async fn test_parse_xsd_with_imports_async_with_import() {
        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/types">
            <xs:simpleType name="EmailType">
                <xs:restriction base="xs:string">
                    <xs:pattern value="[^@]+@[^@]+\.[^@]+"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:t="http://example.com/types"
                   targetNamespace="http://example.com/main">
            <xs:import namespace="http://example.com/types" schemaLocation="types.xsd"/>
            <xs:element name="user">
                <xs:complexType>
                    <xs:sequence>
                        <xs:element name="email" type="t:EmailType"/>
                    </xs:sequence>
                </xs:complexType>
            </xs:element>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/types.xsd", types_xsd.as_bytes());

        let schema = parse_xsd_with_imports_async(
            main_xsd.as_bytes(),
            "http://example.com/main.xsd",
            &fetcher,
        )
        .await
        .unwrap();

        // Main schema elements
        assert!(schema.elements.contains_key("user"));

        // Imported type should be available (stored with namespace prefix)
        assert!(schema.types.contains_key("t:EmailType"));
    }

    #[tokio::test]
    async fn test_parse_xsd_with_imports_async_nested_imports() {
        let base_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/base">
            <xs:simpleType name="IDType">
                <xs:restriction base="xs:string">
                    <xs:pattern value="[A-Z0-9]+"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let types_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:b="http://example.com/base"
                   targetNamespace="http://example.com/types">
            <xs:import namespace="http://example.com/base" schemaLocation="base.xsd"/>
            <xs:complexType name="EntityType">
                <xs:sequence>
                    <xs:element name="id" type="b:IDType"/>
                </xs:sequence>
            </xs:complexType>
        </xs:schema>"#;

        let main_xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   xmlns:t="http://example.com/types"
                   targetNamespace="http://example.com/main">
            <xs:import namespace="http://example.com/types" schemaLocation="types.xsd"/>
            <xs:element name="entity" type="t:EntityType"/>
        </xs:schema>"#;

        let fetcher = MockAsyncFetcher::new();
        fetcher.add_response("http://example.com/base.xsd", base_xsd.as_bytes());
        fetcher.add_response("http://example.com/types.xsd", types_xsd.as_bytes());

        let schema = parse_xsd_with_imports_async(
            main_xsd.as_bytes(),
            "http://example.com/main.xsd",
            &fetcher,
        )
        .await
        .unwrap();

        // All types should be available (stored with namespace prefixes)
        assert!(schema.elements.contains_key("entity"));
        assert!(schema.types.contains_key("t:EntityType"));
        assert!(schema.types.contains_key("b:IDType"));
    }
}
