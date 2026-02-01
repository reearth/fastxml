//! fastxml - Fast, memory-efficient XML library with XPath and XSD validation support.
//!
//! This library provides XML parsing, XPath evaluation, and XSD schema validation
//! with a focus on memory efficiency through streaming processing.
//!
//! # Features
//!
//! - **Fast XML Parsing**: Built on quick-xml for high-performance parsing
//! - **XPath Support**: Evaluate XPath 1.0 expressions
//! - **XSD Validation**: Stream-based schema validation
//! - **Memory Efficient**: Streaming APIs to minimize memory usage
//! - **Thread Safe**: Safe concurrent access through careful design
//!
//! # Feature Flags
//!
//! - `sync` (default): Enables synchronous HTTP client for schema fetching
//! - `async`: Enables async support with tokio and reqwest
//! - `profile`: Enables memory profiling utilities
//!
//! # Quick Start
//!
//! ```rust
//! use fastxml::{parse, xpath, get_root_node, get_node_tag};
//!
//! // Parse XML
//! let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
//!     <gml:name>Hello World</gml:name>
//! </root>"#;
//!
//! let doc = parse(xml).unwrap();
//!
//! // Get root element
//! let root = get_root_node(&doc).unwrap();
//! println!("Root element: {}", get_node_tag(&root));
//!
//! // Evaluate XPath
//! let result = xpath::evaluate(&doc, "//gml:name").unwrap();
//! let texts = xpath::collect_text_values(&result);
//! println!("Found: {:?}", texts);
//! ```
//!
//! # libxml API Compatibility
//!
//! This library provides API compatibility with the libxml crate for easier migration:
//!
//! ```rust
//! use fastxml::{
//!     // Types
//!     XmlDocument, XmlNode, XmlRoNode, XmlContext,
//!     // Parsing
//!     parse,
//!     // XPath
//!     evaluate, create_context, find_nodes_by_xpath,
//!     collect_text_values, collect_text_value,
//!     // Node operations
//!     get_root_node, get_root_readonly_node, get_node_tag,
//!     node_to_xml_string, readonly_node_to_xml_string,
//!     // Schema validation
//!     create_xml_schema_validation_context,
//!     validate_document_by_schema,
//!     parse_schema_locations,
//! };
//! ```
//!
//! # Streaming Processing
//!
//! For large files, use streaming APIs to minimize memory usage:
//!
//! ```rust
//! use fastxml::event::{StreamingParser, XmlEventHandler, XmlEvent};
//! use fastxml::error::Result;
//!
//! struct MyHandler;
//!
//! impl XmlEventHandler for MyHandler {
//!     fn handle(&mut self, event: &XmlEvent) -> Result<()> {
//!         match event {
//!             XmlEvent::StartElement { name, .. } => {
//!                 println!("Start: {}", name);
//!             }
//!             _ => {}
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let xml = "<root><child/></root>";
//! let mut parser = StreamingParser::new(xml.as_bytes());
//! parser.add_handler(Box::new(MyHandler));
//! parser.parse().unwrap();
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
// Allow some clippy lints for code clarity
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::large_enum_variant)]

pub mod document;
pub mod error;
pub mod event;
pub mod generator;
pub mod namespace;
pub mod node;
pub mod node_error;
pub mod parse_error;
pub mod parser;
pub mod profile;
pub mod schema;
pub mod serialize;
pub mod xpath;

// Re-export error types
pub use error::{Error, ErrorLevel, Result, StructuredError, ValidationErrorType};

// Re-export document types
pub use document::{DocumentBuilder, XmlDocument};

// Re-export node types
pub use node::{NodeId, NodeType, XmlNode, XmlRoNode};

// Re-export namespace types
pub use namespace::{Namespace, NamespaceResolver};

// Re-export parser functions
pub use parser::{
    ParserOptions, parse, parse_from_bufread, parse_schema_locations, parse_with_options,
};

// Re-export XPath context (for libxml compatibility)
pub use xpath::context::{XmlContext, XmlSafeContext};

// ============================================================================
// libxml-compatible API
// ============================================================================

/// Evaluates an XPath expression on a document.
///
/// This is the main XPath evaluation entry point, compatible with libxml's API.
pub fn evaluate<T: AsRef<str>>(
    document: &XmlDocument,
    xpath_expr: T,
) -> Result<xpath::XPathResult> {
    xpath::evaluate(document, xpath_expr.as_ref())
}

/// Creates an XPath context for a document.
///
/// The context automatically registers namespace bindings from the root element.
pub fn create_context(document: &XmlDocument) -> Result<XmlContext> {
    xpath::create_context(document)
}

/// Creates a thread-safe XPath context for a document.
pub fn create_safe_context(document: &XmlDocument) -> Result<XmlSafeContext> {
    xpath::create_safe_context(document)
}

/// Finds nodes by XPath expression relative to a node.
pub fn find_nodes_by_xpath(
    ctx: &XmlContext,
    xpath_expr: &str,
    node: &XmlNode,
) -> Result<Vec<XmlNode>> {
    xpath::find_nodes_by_xpath(ctx, xpath_expr, node)
}

/// Finds read-only nodes by XPath expression.
pub fn find_readonly_nodes_by_xpath(
    ctx: &XmlContext,
    xpath_expr: &str,
    node: &XmlRoNode,
) -> Result<Vec<XmlRoNode>> {
    xpath::find_readonly_nodes_by_xpath(ctx, xpath_expr, node)
}

/// Finds read-only nodes using a thread-safe context.
pub fn find_safe_readonly_nodes_by_xpath(
    ctx: &XmlSafeContext,
    xpath_expr: &str,
    node: &XmlRoNode,
) -> Result<Vec<XmlRoNode>> {
    xpath::find_safe_readonly_nodes_by_xpath(ctx, xpath_expr, node)
}

/// Finds read-only nodes matching element names.
pub fn find_readonly_nodes_in_elements(
    ctx: &XmlContext,
    node: &XmlRoNode,
    elements_to_match: &[&str],
) -> Result<Vec<XmlRoNode>> {
    xpath::find_readonly_nodes_in_elements(ctx, node, elements_to_match)
}

/// Collects text values from an XPath result.
pub fn collect_text_values(xpath_value: &xpath::XPathResult) -> Vec<String> {
    xpath::collect_text_values(xpath_value)
}

/// Collects a single text value from an XPath result.
pub fn collect_text_value(xpath_value: &xpath::XPathResult) -> String {
    xpath::collect_text_value(xpath_value)
}

/// Gets the root element node from a document.
pub fn get_root_node(document: &XmlDocument) -> Result<XmlNode> {
    document.get_root_element()
}

/// Gets the root element as a read-only node.
pub fn get_root_readonly_node(document: &XmlDocument) -> Result<XmlRoNode> {
    document.get_root_element_ro()
}

/// Gets the qualified tag name of a node (prefix:name or just name).
pub fn get_node_tag(node: &XmlNode) -> String {
    node.qname()
}

/// Gets the qualified tag name of a read-only node.
pub fn get_readonly_node_tag(node: &XmlRoNode) -> String {
    node.qname()
}

/// Gets the namespace prefix of a node.
pub fn get_node_prefix(node: &XmlNode) -> String {
    node.get_prefix().unwrap_or_default()
}

/// Gets the namespace prefix of a read-only node.
pub fn get_readonly_node_prefix(node: &XmlRoNode) -> String {
    node.get_prefix().unwrap_or_default()
}

/// Serializes a node to an XML string.
pub fn node_to_xml_string(document: &XmlDocument, node: &mut XmlNode) -> Result<String> {
    serialize::node_to_xml_string(document, node)
}

/// Serializes a read-only node to an XML string.
pub fn readonly_node_to_xml_string(document: &XmlDocument, node: &XmlRoNode) -> Result<String> {
    serialize::readonly_node_to_xml_string(document, node)
}

/// Creates an XSD schema validation context.
pub fn create_xml_schema_validation_context(
    schema_location: String,
) -> Result<schema::XmlSchemaValidationContext> {
    schema::create_xml_schema_validation_context(&schema_location)
}

/// Creates an XSD schema validation context from a buffer.
pub fn create_xml_schema_validation_context_from_buffer(
    schema: &[u8],
) -> Result<schema::XmlSchemaValidationContext> {
    schema::create_xml_schema_validation_context_from_buffer(schema)
}

/// Validates a document against an XSD schema.
pub fn validate_document_by_schema(
    document: &XmlDocument,
    schema_location: String,
) -> Result<Vec<StructuredError>> {
    schema::validate_document_by_schema(document, &schema_location)
}

/// Validates a document using an existing validation context.
pub fn validate_document_by_schema_context(
    document: &XmlDocument,
    ctx: &schema::XmlSchemaValidationContext,
) -> Result<Vec<StructuredError>> {
    schema::validate_document_by_schema_context(document, ctx)
}

/// Parses XSD content and returns a compiled schema.
pub fn parse_xsd(content: &[u8]) -> Result<schema::types::CompiledSchema> {
    schema::parse_xsd(content)
}

/// Parses XSD content with import resolution.
#[cfg(feature = "ureq")]
pub fn parse_xsd_with_imports(
    content: &[u8],
    base_uri: &str,
    fetcher: &schema::UreqFetcher,
    store: &impl schema::store::SchemaStore,
) -> Result<schema::types::CompiledSchema> {
    schema::parse_xsd_with_imports(content, base_uri, fetcher, store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_parsing() {
        let xml = r#"<root attr="value"><child>text</child></root>"#;
        let doc = parse(xml).unwrap();

        let root = get_root_node(&doc).unwrap();
        assert_eq!(get_node_tag(&root), "root");
    }

    #[test]
    fn test_xpath_evaluation() {
        let xml = r#"<root><Building/><Room/><Window/></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "//*[name()='Building']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "Building");
    }

    #[test]
    fn test_context_xpath() {
        let xml = r#"<root><a>1</a><a>2</a></root>"#;
        let doc = parse(xml).unwrap();
        let ctx = create_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let nodes = find_readonly_nodes_by_xpath(&ctx, "//a", &root).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_text_collection() {
        let xml = r#"<root><a>one</a><a>two</a></root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/root/a").unwrap();
        let texts = collect_text_values(&result);
        assert_eq!(texts, vec!["one", "two"]);
    }

    #[test]
    fn test_namespaced_xml() {
        let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#;
        let doc = parse(xml).unwrap();

        let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_serialization() {
        let xml = r#"<root><child>text</child></root>"#;
        let doc = parse(xml).unwrap();
        let mut root = get_root_node(&doc).unwrap();

        let serialized = node_to_xml_string(&doc, &mut root).unwrap();
        assert!(serialized.contains("<root>"));
        assert!(serialized.contains("<child>text</child>"));
    }
}
