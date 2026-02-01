//! XPath value types and evaluation context.
//!
//! This module defines the core types used in XPath evaluation:
//! - `XPathValue`: The four XPath data types (NodeSet, String, Number, Boolean)
//! - `EvaluationContext`: Context information for evaluation

use std::sync::Arc;

use crate::document::XmlDocument;
use crate::namespace::NamespaceResolver;
use crate::node::XmlNode;

/// XPath value types according to XPath 1.0 specification.
///
/// XPath expressions evaluate to one of four basic types:
/// - **NodeSet**: An unordered collection of nodes without duplicates
/// - **String**: A sequence of UCS characters
/// - **Number**: A floating-point number (IEEE 754 double)
/// - **Boolean**: A true/false value
#[derive(Debug, Clone)]
pub enum XPathValue {
    /// A set of nodes (unordered, no duplicates)
    NodeSet(Vec<XmlNode>),
    /// A string value
    String(String),
    /// A numeric value (IEEE 754 double)
    Number(f64),
    /// A boolean value
    Boolean(bool),
}

impl XPathValue {
    /// Creates an empty node set.
    pub fn empty_nodeset() -> Self {
        XPathValue::NodeSet(Vec::new())
    }

    /// Creates a node set from a single node.
    pub fn from_node(node: XmlNode) -> Self {
        XPathValue::NodeSet(vec![node])
    }

    /// Creates a node set from multiple nodes.
    pub fn from_nodes(nodes: Vec<XmlNode>) -> Self {
        XPathValue::NodeSet(nodes)
    }

    /// Returns true if this is a node set.
    pub fn is_nodeset(&self) -> bool {
        matches!(self, XPathValue::NodeSet(_))
    }

    /// Returns true if this is a string.
    pub fn is_string(&self) -> bool {
        matches!(self, XPathValue::String(_))
    }

    /// Returns true if this is a number.
    pub fn is_number(&self) -> bool {
        matches!(self, XPathValue::Number(_))
    }

    /// Returns true if this is a boolean.
    pub fn is_boolean(&self) -> bool {
        matches!(self, XPathValue::Boolean(_))
    }

    /// Converts the value to a node set, returning empty vec for non-nodeset values.
    pub fn into_nodes(self) -> Vec<XmlNode> {
        match self {
            XPathValue::NodeSet(nodes) => nodes,
            _ => Vec::new(),
        }
    }

    /// Returns a reference to nodes if this is a node set.
    pub fn as_nodes(&self) -> Option<&[XmlNode]> {
        match self {
            XPathValue::NodeSet(nodes) => Some(nodes),
            _ => None,
        }
    }

    /// Converts to string according to XPath 1.0 section 4.2.
    ///
    /// - NodeSet: string value of first node in document order (or empty if empty set)
    /// - Number: converted per XPath spec (NaN -> "NaN", Inf -> "Infinity", etc.)
    /// - Boolean: "true" or "false"
    /// - String: unchanged
    pub fn to_string_value(&self) -> String {
        match self {
            XPathValue::NodeSet(nodes) => {
                nodes.first()
                    .and_then(|n| n.get_content())
                    .unwrap_or_default()
            }
            XPathValue::String(s) => s.clone(),
            XPathValue::Boolean(b) => b.to_string(),
            XPathValue::Number(n) => {
                if n.is_nan() {
                    "NaN".to_string()
                } else if n.is_infinite() {
                    if *n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
                } else if *n == 0.0 {
                    "0".to_string()
                } else {
                    // Remove trailing zeros for cleaner output
                    let s = n.to_string();
                    if s.contains('.') && !s.contains('e') && !s.contains('E') {
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    } else {
                        s
                    }
                }
            }
        }
    }

    /// Converts to boolean according to XPath 1.0 section 4.3.
    ///
    /// - NodeSet: true if non-empty
    /// - Number: false if NaN or zero, true otherwise
    /// - String: true if non-empty
    /// - Boolean: unchanged
    pub fn to_boolean(&self) -> bool {
        match self {
            XPathValue::NodeSet(nodes) => !nodes.is_empty(),
            XPathValue::String(s) => !s.is_empty(),
            XPathValue::Boolean(b) => *b,
            XPathValue::Number(n) => *n != 0.0 && !n.is_nan(),
        }
    }

    /// Converts to number according to XPath 1.0 section 4.4.
    ///
    /// - NodeSet: convert string value to number
    /// - String: parse as number, NaN if invalid
    /// - Boolean: 1 for true, 0 for false
    /// - Number: unchanged
    pub fn to_number(&self) -> f64 {
        match self {
            XPathValue::NodeSet(nodes) => {
                nodes.first()
                    .and_then(|n| n.get_content())
                    .and_then(|s| parse_xpath_number(&s))
                    .unwrap_or(f64::NAN)
            }
            XPathValue::String(s) => parse_xpath_number(s).unwrap_or(f64::NAN),
            XPathValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
            XPathValue::Number(n) => *n,
        }
    }

    /// Collects text values from nodes.
    pub fn collect_text_values(&self) -> Vec<String> {
        match self {
            XPathValue::NodeSet(nodes) => {
                nodes.iter()
                    .filter_map(|n| n.get_content())
                    .collect()
            }
            XPathValue::String(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

/// Parses a string as an XPath number.
///
/// According to XPath 1.0, leading and trailing whitespace is stripped,
/// and the remaining string is parsed as a floating-point number.
fn parse_xpath_number(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(f64::NAN);
    }
    trimmed.parse::<f64>().ok()
}

/// Context information for XPath evaluation.
///
/// This struct holds all the context needed during XPath expression evaluation,
/// including the current context node, position information, and document reference.
#[derive(Clone)]
pub struct EvaluationContext<'a> {
    /// The context node (current node being evaluated)
    pub node: XmlNode,
    /// Position of context node in the current node set (1-based)
    pub position: usize,
    /// Size of the current node set
    pub size: usize,
    /// Reference to the document being queried
    pub doc: &'a XmlDocument,
    /// Namespace resolver for prefix-to-URI mappings
    pub resolver: NamespaceResolver,
    /// Variable bindings (name -> value)
    variables: Option<Arc<std::collections::HashMap<String, XPathValue>>>,
}

impl<'a> EvaluationContext<'a> {
    /// Creates a new evaluation context.
    pub fn new(
        node: XmlNode,
        doc: &'a XmlDocument,
        resolver: NamespaceResolver,
    ) -> Self {
        Self {
            node,
            position: 1,
            size: 1,
            doc,
            resolver,
            variables: None,
        }
    }

    /// Creates a context for a specific position in a node set.
    pub fn with_position(mut self, position: usize, size: usize) -> Self {
        self.position = position;
        self.size = size;
        self
    }

    /// Creates a new context with a different context node.
    pub fn with_node(&self, node: XmlNode) -> Self {
        Self {
            node,
            position: self.position,
            size: self.size,
            doc: self.doc,
            resolver: self.resolver.clone(),
            variables: self.variables.clone(),
        }
    }

    /// Creates a new context for evaluating predicates on a node set.
    ///
    /// This sets up the position and size for the given node in the set.
    pub fn for_predicate(&self, node: XmlNode, position: usize, size: usize) -> Self {
        Self {
            node,
            position,
            size,
            doc: self.doc,
            resolver: self.resolver.clone(),
            variables: self.variables.clone(),
        }
    }

    /// Sets variable bindings.
    pub fn with_variables(
        mut self,
        variables: std::collections::HashMap<String, XPathValue>,
    ) -> Self {
        self.variables = Some(Arc::new(variables));
        self
    }

    /// Gets a variable value by name.
    pub fn get_variable(&self, name: &str) -> Option<&XPathValue> {
        self.variables.as_ref()?.get(name)
    }

    /// Returns the current position (1-based).
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns the context size.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Resolves a namespace prefix to its URI.
    pub fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        self.resolver.resolve_prefix(prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xpath_value_to_string() {
        assert_eq!(XPathValue::String("hello".into()).to_string_value(), "hello");
        assert_eq!(XPathValue::Number(42.0).to_string_value(), "42");
        assert_eq!(XPathValue::Number(3.14).to_string_value(), "3.14");
        assert_eq!(XPathValue::Number(f64::NAN).to_string_value(), "NaN");
        assert_eq!(XPathValue::Number(f64::INFINITY).to_string_value(), "Infinity");
        assert_eq!(XPathValue::Boolean(true).to_string_value(), "true");
        assert_eq!(XPathValue::Boolean(false).to_string_value(), "false");
    }

    #[test]
    fn test_xpath_value_to_boolean() {
        assert!(XPathValue::Boolean(true).to_boolean());
        assert!(!XPathValue::Boolean(false).to_boolean());
        assert!(XPathValue::String("hello".into()).to_boolean());
        assert!(!XPathValue::String("".into()).to_boolean());
        assert!(XPathValue::Number(1.0).to_boolean());
        assert!(!XPathValue::Number(0.0).to_boolean());
        assert!(!XPathValue::Number(f64::NAN).to_boolean());
    }

    #[test]
    fn test_xpath_value_to_number() {
        assert_eq!(XPathValue::Number(42.0).to_number(), 42.0);
        assert_eq!(XPathValue::String("3.14".into()).to_number(), 3.14);
        assert!(XPathValue::String("not a number".into()).to_number().is_nan());
        assert_eq!(XPathValue::Boolean(true).to_number(), 1.0);
        assert_eq!(XPathValue::Boolean(false).to_number(), 0.0);
    }

    #[test]
    fn test_parse_xpath_number() {
        assert_eq!(parse_xpath_number("42"), Some(42.0));
        assert_eq!(parse_xpath_number("  3.14  "), Some(3.14));
        assert_eq!(parse_xpath_number("-1.5"), Some(-1.5));
        assert!(parse_xpath_number("").unwrap().is_nan());
        assert!(parse_xpath_number("abc").is_none());
    }
}
