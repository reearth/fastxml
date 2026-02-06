//! Read-only XML node reference.

use indexmap::IndexMap;

use crate::namespace::Namespace;

use super::mutable::XmlNode;
use super::types::{NodeId, NodeType};

/// A read-only reference to a node.
///
/// This is a lightweight, read-only view of a node that provides
/// the same getters as `XmlNode` but cannot modify the document.
#[derive(Clone)]
pub struct XmlRoNode {
    inner: XmlNode,
}

impl XmlRoNode {
    /// Creates a read-only view from an XmlNode.
    pub fn from_node(node: XmlNode) -> Self {
        Self { inner: node }
    }

    /// Returns the node ID.
    pub fn id(&self) -> NodeId {
        self.inner.id()
    }

    /// Returns the node type.
    pub fn get_type(&self) -> NodeType {
        self.inner.get_type()
    }

    /// Returns the local name.
    pub fn get_name(&self) -> String {
        self.inner.get_name()
    }

    /// Returns the namespace prefix.
    pub fn get_prefix(&self) -> Option<String> {
        self.inner.get_prefix()
    }

    /// Returns the namespace URI.
    pub fn get_namespace_uri(&self) -> Option<String> {
        self.inner.get_namespace_uri()
    }

    /// Returns the namespace.
    pub fn get_namespace(&self) -> Option<Namespace> {
        self.inner.get_namespace()
    }

    /// Returns the qualified name.
    pub fn qname(&self) -> String {
        self.inner.qname()
    }

    /// Returns the text content.
    pub fn get_content(&self) -> Option<String> {
        self.inner.get_content()
    }

    /// Returns an attribute value.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.inner.get_attribute(name)
    }

    /// Returns an attribute value with namespace.
    pub fn get_attribute_ns(&self, name: &str, ns_uri: &str) -> Option<String> {
        self.inner.get_attribute_ns(name, ns_uri)
    }

    /// Returns all attributes.
    pub fn get_attributes(&self) -> IndexMap<String, String> {
        self.inner.get_attributes()
    }

    /// Returns namespace declarations.
    pub fn get_namespace_declarations(&self) -> Vec<Namespace> {
        self.inner.get_namespace_declarations()
    }

    /// Returns the parent node.
    pub fn get_parent(&self) -> Option<XmlRoNode> {
        self.inner.get_parent().map(XmlRoNode::from_node)
    }

    /// Returns child nodes.
    pub fn get_child_nodes(&self) -> Vec<XmlRoNode> {
        self.inner
            .get_child_nodes()
            .into_iter()
            .map(XmlRoNode::from_node)
            .collect()
    }

    /// Returns child elements.
    pub fn get_child_elements(&self) -> Vec<XmlRoNode> {
        self.inner
            .get_child_elements()
            .into_iter()
            .map(XmlRoNode::from_node)
            .collect()
    }

    /// Returns the first child.
    pub fn first_child(&self) -> Option<XmlRoNode> {
        self.inner.first_child().map(XmlRoNode::from_node)
    }

    /// Returns the last child.
    pub fn last_child(&self) -> Option<XmlRoNode> {
        self.inner.last_child().map(XmlRoNode::from_node)
    }

    /// Returns the line number.
    pub fn line(&self) -> Option<usize> {
        self.inner.line()
    }

    /// Returns the column number.
    pub fn column(&self) -> Option<usize> {
        self.inner.column()
    }

    /// Returns true if this is an element.
    pub fn is_element(&self) -> bool {
        self.inner.is_element()
    }

    /// Returns true if this is text.
    pub fn is_text(&self) -> bool {
        self.inner.is_text()
    }

    /// Converts to a mutable XmlNode (use with caution).
    pub fn into_node(self) -> XmlNode {
        self.inner
    }
}

impl std::fmt::Debug for XmlRoNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XmlRoNode")
            .field("id", &self.id())
            .field("type", &self.get_type())
            .field("name", &self.get_name())
            .finish()
    }
}

impl PartialEq for XmlRoNode {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for XmlRoNode {}

impl std::hash::Hash for XmlRoNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}
