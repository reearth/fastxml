//! libxml-compatible API.
//!
//! These free functions mirror the shape of libxml's C API to ease migration.
//! They are thin wrappers over the modern front doors — prefer those for new
//! code:
//!
//! | libxml-style (`fastxml::compat`)      | Modern equivalent                       |
//! |---------------------------------------|-----------------------------------------|
//! | `evaluate(&doc, xpath)`               | [`doc.query(xpath)`](crate::QueryExt)   |
//! | `create_context(&doc)`                | [`XmlContext::new(&doc)`](crate::XmlContext) |
//! | `node_to_xml_string(&doc, node)`      | [`Printer::from(node)`](crate::Printer) |
//! | `get_root_node(&doc)`                 | `doc.get_root_element()`                 |
//! | `get_node_tag(node)`                  | `node.qname()`                          |
//!
//! ```
//! use fastxml::Parser;
//! use fastxml::compat::{evaluate, get_root_node};
//!
//! let doc = Parser::from("<root><item/></root>").parse().unwrap();
//! let root = get_root_node(&doc).unwrap();
//! let items = evaluate(&doc, "//item").unwrap();
//! assert_eq!(items.into_nodes().len(), 1);
//! ```

use crate::document::XmlDocument;
use crate::error::Result;
use crate::node::{XmlNode, XmlRoNode};
use crate::xpath::{self, XmlContext, XmlSafeContext};

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
    crate::serialize::node_to_xml_string(document, node)
}

/// Serializes a read-only node to an XML string.
pub fn readonly_node_to_xml_string(document: &XmlDocument, node: &XmlRoNode) -> Result<String> {
    crate::serialize::readonly_node_to_xml_string(document, node)
}
