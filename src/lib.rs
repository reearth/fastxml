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
//! - **Async Support**: Async schema fetching and resolution with tokio
//!
//! # Feature Flags
//!
//! - `ureq`: Enables synchronous HTTP client for schema fetching (recommended)
//! - `tokio`: Enables async HTTP client and schema resolution with reqwest
//! - `async-trait`: Enables async trait support for custom implementations
//! - `profile`: Enables memory profiling utilities
//!
//! # Quick Start
//!
//! ```rust
//! use fastxml::{Parser, evaluate, collect_text_values};
//!
//! let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
//!     <gml:name>Hello World</gml:name>
//! </root>"#;
//!
//! let doc = Parser::from(xml).parse().unwrap();
//!
//! let result = evaluate(&doc, "//gml:name").unwrap();
//! let texts = collect_text_values(&result);
//! println!("Found: {:?}", texts);
//! ```
//!
//! # Schema Validation
//!
//! ```rust
//! use fastxml::schema::{Schema, Validator};
//!
//! let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
//!     <xs:element name="age" type="xs:int"/>
//! </xs:schema>"#;
//!
//! let schema = Schema::from_xsd(xsd).unwrap();
//! let report = Validator::from("<age>42</age>").schema(schema).run().unwrap();
//! assert!(report.is_valid());
//! ```
//!
//! # Streaming Processing
//!
//! For large files, stream events with constant memory:
//!
//! ```rust
//! use fastxml::Parser;
//! use fastxml::event::XmlEvent;
//!
//! let xml = "<root><child/></root>";
//! let mut elements = 0;
//! Parser::from(xml).for_each_event(|event| {
//!     if let XmlEvent::StartElement { .. } = event {
//!         elements += 1;
//!     }
//!     Ok(())
//! }).unwrap();
//! assert_eq!(elements, 2);
//! ```
//!
//! # Async Schema Resolution
//!
//! Build a schema with async import/include resolution (requires `tokio` feature):
//!
//! ```ignore
//! use fastxml::schema::{AsyncDefaultFetcher, Schema};
//!
//! #[tokio::main]
//! async fn main() -> fastxml::error::Result<()> {
//!     let fetcher = AsyncDefaultFetcher::new()?;
//!     let schema = Schema::builder()
//!         .add("http://example.com/schema.xsd", std::fs::read("schema.xsd")?)
//!         .resolve_with_async(&fetcher)
//!         .await?;
//!     Ok(())
//! }
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
pub mod parser;
pub mod position;
pub mod profile;
pub mod schema;
pub mod serialize;
pub mod transform;
pub mod xpath;

// Re-export error types
pub use error::{Error, ErrorLevel, ErrorLocation, Result, StructuredError, ValidationErrorType};

// Re-export document types
pub use document::{DocumentBuilder, XmlDocument};

// Re-export node types
pub use node::{NodeId, NodeType, XmlNode, XmlRoNode};

// Re-export namespace types
pub use namespace::{Namespace, NamespaceResolver};

// Re-export compact_str for use in XmlEvent attributes
pub use compact_str::CompactString;

// Re-export parser entry points
pub use parser::{Parser, ParserOptions, parse_schema_locations};
// Internal-only: the free `parse` is used by in-crate tests; not part of the
// public API (use `Parser::from(..).parse()`).
pub(crate) use parser::parse;

// Re-export position tracking
pub use position::PositionTrackingReader;

// Re-export XPath context (for libxml compatibility)
pub use xpath::context::{XmlContext, XmlSafeContext};

// Re-export the serialization and XPath front doors
pub use serialize::Printer;
pub use xpath::{AsQuery, Query, QueryExt};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_parsing() {
        let xml = r#"<root attr="value"><child>text</child></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let root = get_root_node(&doc).unwrap();
        assert_eq!(get_node_tag(&root), "root");
    }

    #[test]
    fn test_xpath_evaluation() {
        let xml = r#"<root><Building/><Room/><Window/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "//*[name()='Building']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "Building");
    }

    #[test]
    fn test_context_xpath() {
        let xml = r#"<root><a>1</a><a>2</a></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let ctx = create_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let nodes = find_readonly_nodes_by_xpath(&ctx, "//a", &root).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_text_collection() {
        let xml = r#"<root><a>one</a><a>two</a></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root/a").unwrap();
        let texts = collect_text_values(&result);
        assert_eq!(texts, vec!["one", "two"]);
    }

    #[test]
    fn test_namespaced_xml() {
        let xml = r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_serialization() {
        let xml = r#"<root><child>text</child></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let mut root = get_root_node(&doc).unwrap();

        let serialized = node_to_xml_string(&doc, &mut root).unwrap();
        assert!(serialized.contains("<root>"));
        assert!(serialized.contains("<child>text</child>"));
    }

    #[test]
    fn test_create_safe_context() {
        let xml = r#"<root><a>1</a></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let ctx = create_safe_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let nodes = find_safe_readonly_nodes_by_xpath(&ctx, "//a", &root).unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_find_nodes_by_xpath() {
        let xml = r#"<root><a>1</a><b>2</b></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let ctx = create_context(&doc).unwrap();
        let root = get_root_node(&doc).unwrap();

        let nodes = find_nodes_by_xpath(&ctx, "//a", &root).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "a");
    }

    #[test]
    fn test_find_readonly_nodes_in_elements() {
        let xml = r#"<root><a>1</a><b>2</b><c>3</c></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let ctx = create_context(&doc).unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let nodes = find_readonly_nodes_in_elements(&ctx, &root, &["a", "c"]).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_collect_text_value_single() {
        let xml = r#"<root><a>hello</a></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root/a").unwrap();
        let text = collect_text_value(&result);
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_collect_text_value_empty() {
        let xml = r#"<root><a/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root/nonexistent").unwrap();
        let text = collect_text_value(&result);
        assert!(text.is_empty());
    }

    #[test]
    fn test_get_readonly_node_tag() {
        let xml = r#"<ns:root xmlns:ns="http://example.com"/>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        assert_eq!(get_readonly_node_tag(&root), "ns:root");
    }

    #[test]
    fn test_get_node_prefix() {
        let xml = r#"<ns:root xmlns:ns="http://example.com"/>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_node(&doc).unwrap();

        assert_eq!(get_node_prefix(&root), "ns");
    }

    #[test]
    fn test_get_node_prefix_empty() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_node(&doc).unwrap();

        assert_eq!(get_node_prefix(&root), "");
    }

    #[test]
    fn test_get_readonly_node_prefix() {
        let xml = r#"<ns:root xmlns:ns="http://example.com"/>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        assert_eq!(get_readonly_node_prefix(&root), "ns");
    }

    #[test]
    fn test_get_readonly_node_prefix_empty() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        assert_eq!(get_readonly_node_prefix(&root), "");
    }

    #[test]
    fn test_readonly_node_to_xml_string() {
        let xml = r#"<root><child>text</child></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let root = get_root_readonly_node(&doc).unwrap();

        let serialized = readonly_node_to_xml_string(&doc, &root).unwrap();
        assert!(serialized.contains("<root>"));
        assert!(serialized.contains("<child>text</child>"));
    }

    #[test]
    fn test_evaluate_with_string_ref() {
        let xml = r#"<root><a>1</a></root>"#;
        let doc = Parser::from(xml).parse().unwrap();
        let xpath_str = String::from("//a");

        let result = evaluate(&doc, &xpath_str).unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_parse_xsd_basic() {
        let xsd = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
    <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

        let schema = crate::schema::xsd::parse_xsd(xsd).unwrap();
        assert!(!schema.elements.is_empty() || !schema.types.is_empty());
    }
}
