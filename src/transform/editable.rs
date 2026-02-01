//! Editable node for DOM manipulation during transformation.

use std::collections::HashMap;

use crate::document::{DocumentBuilder, XmlDocument};
use crate::namespace::Namespace;
use crate::node::{NodeId, NodeType, XmlNode};

use super::error::{TransformError, TransformResult};

/// A DOM subtree that can be modified during transformation.
///
/// This wraps an `XmlDocument` containing only the matched subtree,
/// allowing modifications before serialization.
pub struct EditableNode {
    /// The document containing the subtree
    doc: XmlDocument,
    /// The root node ID of the matched subtree
    root_id: NodeId,
    /// Pending modifications
    modifications: Vec<Modification>,
    /// Whether the node should be removed from output
    removed: bool,
}

/// A modification to apply to a node.
#[derive(Debug, Clone)]
pub enum Modification {
    /// Set an attribute value
    SetAttribute {
        /// Attribute name
        name: String,
        /// Attribute value
        value: String,
    },
    /// Remove an attribute
    RemoveAttribute {
        /// Attribute name
        name: String,
    },
    /// Set the text content (replaces all children with a text node)
    SetTextContent(String),
    /// Append a new child element
    AppendChild(NewNode),
    /// Prepend a new child element
    PrependChild(NewNode),
    /// Replace text content while preserving structure
    ReplaceText {
        /// Old text to find
        old: String,
        /// New text to replace with
        new: String,
    },
}

/// A new node to be inserted.
#[derive(Debug, Clone)]
pub enum NewNode {
    /// A new element
    Element {
        /// Element name
        name: String,
        /// Namespace prefix
        prefix: Option<String>,
        /// Attributes
        attributes: HashMap<String, String>,
        /// Child nodes
        children: Vec<NewNode>,
    },
    /// A text node
    Text(String),
    /// A CDATA section
    CData(String),
    /// A comment
    Comment(String),
}

impl EditableNode {
    /// Creates a new editable node from a document and root ID.
    pub(crate) fn new(doc: XmlDocument, root_id: NodeId) -> Self {
        Self {
            doc,
            root_id,
            modifications: Vec::new(),
            removed: false,
        }
    }

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
    pub fn get_attributes(&self) -> HashMap<String, String> {
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
    fn root_node(&self) -> XmlNode {
        self.doc.get_node(self.root_id).expect("root node exists")
    }
}

/// A read-only reference to a child node within an EditableNode.
pub struct EditableNodeRef<'a> {
    node: XmlNode,
    #[allow(dead_code)]
    doc: &'a XmlDocument,
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

    /// Returns the text content.
    pub fn get_content(&self) -> Option<String> {
        self.node.get_content()
    }

    /// Gets an attribute value.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.node.get_attribute(name)
    }

    /// Returns all attributes.
    pub fn get_attributes(&self) -> HashMap<String, String> {
        self.node.get_attributes()
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

/// Builder for creating EditableNode from parsed XML events.
pub struct EditableNodeBuilder {
    builder: DocumentBuilder,
    depth: usize,
}

impl EditableNodeBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            builder: DocumentBuilder::new(),
            depth: 0,
        }
    }

    /// Adds a start element event.
    pub fn start_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        namespace_uri: Option<&str>,
        attributes: Vec<(&str, &str)>,
        namespace_decls: Vec<Namespace>,
    ) {
        self.builder.start_element(
            name,
            prefix,
            namespace_uri,
            attributes,
            namespace_decls,
            None,
            None,
        );
        self.depth += 1;
    }

    /// Adds an end element event.
    pub fn end_element(&mut self) {
        self.builder.end_element();
        self.depth = self.depth.saturating_sub(1);
    }

    /// Adds text content.
    pub fn text(&mut self, content: &str) {
        self.builder.text(content);
    }

    /// Adds CDATA content.
    pub fn cdata(&mut self, content: &str) {
        self.builder.cdata(content);
    }

    /// Adds a comment.
    pub fn comment(&mut self, content: &str) {
        self.builder.comment(content);
    }

    /// Returns true if the subtree is complete (depth returned to 0).
    pub fn is_complete(&self) -> bool {
        self.depth == 0
    }

    /// Returns the current depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Builds the EditableNode.
    pub fn build(self) -> TransformResult<EditableNode> {
        let doc = self.builder.build();
        let root_id = doc
            .root_element_id
            .ok_or_else(|| TransformError::XmlParse("no root element in subtree".to_string()))?;
        Ok(EditableNode::new(doc, root_id))
    }
}

impl Default for EditableNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node() -> EditableNode {
        let mut builder = EditableNodeBuilder::new();
        builder.start_element("item", None, None, vec![("id", "1")], vec![]);
        builder.text("Hello");
        builder.end_element();
        builder.build().unwrap()
    }

    #[test]
    fn test_read_api() {
        let node = create_test_node();
        assert_eq!(node.name(), "item");
        assert_eq!(node.get_attribute("id"), Some("1".to_string()));
        assert_eq!(node.get_content(), Some("Hello".to_string()));
    }

    #[test]
    fn test_set_attribute() {
        let mut node = create_test_node();
        node.set_attribute("modified", "true");
        assert_eq!(node.get_attribute("modified"), Some("true".to_string()));
        assert!(node.is_modified());
    }

    #[test]
    fn test_remove() {
        let mut node = create_test_node();
        assert!(!node.is_removed());
        node.remove();
        assert!(node.is_removed());
    }
}
