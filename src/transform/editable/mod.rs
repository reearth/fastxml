//! Editable node for DOM manipulation during transformation.

mod builder;
mod reference;
mod serialize;
mod types;
mod xpath;

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::document::XmlDocument;
use crate::node::{NodeId, XmlNode};

pub use builder::EditableNodeBuilder;
pub use reference::EditableNodeRef;
pub use types::{Modification, NewNode};

/// A DOM subtree that can be modified during transformation.
///
/// This wraps an `XmlDocument` containing only the matched subtree,
/// allowing modifications before serialization.
pub struct EditableNode {
    /// The document containing the subtree
    pub(crate) doc: XmlDocument,
    /// The root node ID of the matched subtree
    root_id: NodeId,
    /// Pending modifications
    modifications: Vec<Modification>,
    /// Whether the node should be removed from output
    removed: bool,
    /// Registered namespaces (prefix -> URI) for use in serialization
    pub(crate) namespaces: HashMap<String, String>,
}

impl EditableNode {
    /// Creates a new editable node from a document and root ID.
    pub(crate) fn new(doc: XmlDocument, root_id: NodeId) -> Self {
        Self {
            doc,
            root_id,
            modifications: Vec::new(),
            removed: false,
            namespaces: HashMap::new(),
        }
    }

    /// Creates a new editable node with namespace mappings.
    pub(crate) fn with_namespaces(
        doc: XmlDocument,
        root_id: NodeId,
        namespaces: HashMap<String, String>,
    ) -> Self {
        Self {
            doc,
            root_id,
            modifications: Vec::new(),
            removed: false,
            namespaces,
        }
    }

    // =========================================================================
    // Read API
    // =========================================================================

    /// Returns the local name of the element.
    pub fn name(&self) -> String {
        self.root_node().get_name()
    }

    /// Returns the qualified name (prefix:name) of the element.
    pub fn qname(&self) -> String {
        self.root_node().qname()
    }

    /// Returns the namespace prefix if any.
    pub fn prefix(&self) -> Option<String> {
        self.root_node().get_prefix()
    }

    /// Returns the namespace URI if any.
    pub fn namespace_uri(&self) -> Option<String> {
        self.root_node().get_namespace_uri()
    }

    /// Gets an attribute value by name.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.root_node().get_attribute(name)
    }

    /// Returns all attributes as a map.
    pub fn get_attributes(&self) -> IndexMap<String, String> {
        self.root_node().get_attributes()
    }

    /// Returns the text content of the node.
    pub fn get_content(&self) -> Option<String> {
        self.root_node().get_content()
    }

    /// Returns child element nodes.
    pub fn children(&self) -> Vec<EditableNodeRef<'_>> {
        self.root_node()
            .get_child_elements()
            .into_iter()
            .map(|node| EditableNodeRef {
                node,
                doc: &self.doc,
            })
            .collect()
    }

    /// Returns all child nodes (including text, comments, etc.).
    pub fn child_nodes(&self) -> Vec<EditableNodeRef<'_>> {
        self.root_node()
            .get_child_nodes()
            .into_iter()
            .map(|node| EditableNodeRef {
                node,
                doc: &self.doc,
            })
            .collect()
    }

    // =========================================================================
    // Modification API
    // =========================================================================

    /// Sets an attribute value.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        // Apply immediately to the underlying node
        self.root_node().set_attribute(name, value);
        // Also record for tracking
        self.modifications.push(Modification::SetAttribute {
            name: name.to_string(),
            value: value.to_string(),
        });
    }

    /// Removes an attribute.
    pub fn remove_attribute(&mut self, name: &str) {
        // Apply immediately to the underlying node
        self.root_node().remove_attribute(name);
        // Also record for tracking
        self.modifications.push(Modification::RemoveAttribute {
            name: name.to_string(),
        });
    }

    /// Sets the text content, replacing all children.
    pub fn set_text_content(&mut self, text: &str) {
        // Apply immediately to the underlying node
        self.root_node().set_content(text);
        // Also record for tracking
        self.modifications
            .push(Modification::SetTextContent(text.to_string()));
    }

    /// Appends a new child node.
    pub fn append_child(&mut self, node: NewNode) {
        self.modifications.push(Modification::AppendChild(node));
    }

    /// Prepends a new child node.
    pub fn prepend_child(&mut self, node: NewNode) {
        self.modifications.push(Modification::PrependChild(node));
    }

    /// Replaces text content while preserving structure.
    pub fn replace_text(&mut self, old: &str, new: &str) {
        self.modifications.push(Modification::ReplaceText {
            old: old.to_string(),
            new: new.to_string(),
        });
    }

    /// Marks this node for removal from output.
    pub fn remove(&mut self) {
        self.removed = true;
    }

    /// Returns true if this node is marked for removal.
    pub fn is_removed(&self) -> bool {
        self.removed
    }

    /// Returns true if any modifications have been made.
    pub fn is_modified(&self) -> bool {
        !self.modifications.is_empty() || self.removed
    }

    /// Returns the pending modifications.
    pub fn modifications(&self) -> &[Modification] {
        &self.modifications
    }

    /// Returns a reference to the underlying document.
    pub fn document(&self) -> &XmlDocument {
        &self.doc
    }

    /// Returns a mutable reference to the underlying document.
    pub fn document_mut(&mut self) -> &mut XmlDocument {
        &mut self.doc
    }

    /// Returns the root node.
    pub(crate) fn root_node(&self) -> XmlNode {
        self.doc.get_node(self.root_id).expect("root node exists")
    }
}

#[cfg(test)]
mod tests;
