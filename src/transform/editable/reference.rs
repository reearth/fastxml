//! Read-only reference to child nodes.

use indexmap::IndexMap;

use crate::document::XmlDocument;
use crate::node::{NodeType, XmlNode};

/// A read-only reference to a child node within an EditableNode.
pub struct EditableNodeRef<'a> {
    pub(crate) node: XmlNode,
    pub(crate) doc: &'a XmlDocument,
}

impl<'a> EditableNodeRef<'a> {
    /// Returns the node type.
    pub fn node_type(&self) -> NodeType {
        self.node.get_type()
    }

    /// Returns the local name.
    pub fn name(&self) -> String {
        self.node.get_name()
    }

    /// Returns the qualified name.
    pub fn qname(&self) -> String {
        self.node.qname()
    }

    /// Returns the namespace prefix.
    pub fn prefix(&self) -> Option<String> {
        self.node.get_prefix()
    }

    /// Returns the namespace URI.
    pub fn namespace_uri(&self) -> Option<String> {
        self.node.get_namespace_uri()
    }

    /// Returns the text content.
    pub fn get_content(&self) -> Option<String> {
        self.node.get_content()
    }

    /// Gets an attribute value.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.node.get_attribute(name)
    }

    /// Gets an attribute value by namespace URI and local name.
    pub fn get_attribute_ns(&self, namespace_uri: &str, local_name: &str) -> Option<String> {
        if let Some((_prefix, uri)) = self.node.get_attribute_ns_info(local_name) {
            if uri == namespace_uri {
                return self.node.get_attribute(local_name);
            }
        }
        None
    }

    /// Returns all attributes.
    pub fn get_attributes(&self) -> IndexMap<String, String> {
        self.node.get_attributes()
    }

    /// Returns child element nodes.
    pub fn children(&self) -> Vec<EditableNodeRef<'a>> {
        self.node
            .get_child_elements()
            .into_iter()
            .map(|node| EditableNodeRef {
                node,
                doc: self.doc,
            })
            .collect()
    }

    /// Returns all child nodes (including text, comments, etc.).
    pub fn child_nodes(&self) -> Vec<EditableNodeRef<'a>> {
        self.node
            .get_child_nodes()
            .into_iter()
            .map(|node| EditableNodeRef {
                node,
                doc: self.doc,
            })
            .collect()
    }

    /// Returns true if this is an element node.
    pub fn is_element(&self) -> bool {
        self.node.is_element()
    }

    /// Returns true if this is a text node.
    pub fn is_text(&self) -> bool {
        self.node.is_text()
    }
}
