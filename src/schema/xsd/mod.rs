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
//! use fastxml::schema::{UreqFetcher, TempDirStore};
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
//! let store = TempDirStore::new()?;
//! let schema = parse_xsd_with_imports(
//!     xsd_content.as_bytes(),
//!     "http://example.com/schema.xsd",
//!     &fetcher,
//!     &store,
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
pub mod facets;
pub mod parser;
pub mod resolver;
pub mod types;

use crate::error::Result;
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::store::SchemaStore;
use crate::schema::types::CompiledSchema;

pub use builtin::{gml, register_builtin_types, xs};
pub use compiler::{XsdCompiler, compile_schemas};
pub use parser::{XSD_NAMESPACE, XsdParser, parse_xsd_ast};
pub use resolver::{SchemaResolver, resolve_uri};
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
/// fetching remote schemas as needed and caching them in the store.
///
/// # Arguments
///
/// * `content` - The entry XSD file content as bytes
/// * `base_uri` - Base URI for resolving relative imports
/// * `fetcher` - Schema fetcher for downloading remote schemas
/// * `store` - Schema store for caching downloaded schemas
///
/// # Returns
///
/// A compiled schema with all dependencies resolved
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::{UreqFetcher, TempDirStore};
///
/// let fetcher = UreqFetcher::new();
/// let store = TempDirStore::new()?;
///
/// let schema = parse_xsd_with_imports(
///     xsd_content.as_bytes(),
///     "http://example.com/schemas/main.xsd",
///     &fetcher,
///     &store,
/// )?;
/// ```
pub fn parse_xsd_with_imports<F: SchemaFetcher, S: SchemaStore>(
    content: &[u8],
    base_uri: &str,
    fetcher: &F,
    store: &S,
) -> Result<CompiledSchema> {
    // Create resolver and resolve all dependencies
    let mut resolver = SchemaResolver::new(fetcher, store);
    let schemas = resolver.resolve_all(content, base_uri)?;

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
}
