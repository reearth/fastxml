//! XML document representation.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::error::Result;
use crate::node_error::NodeError;
use crate::namespace::{Namespace, NamespaceResolver};
use crate::node::{NodeData, NodeId, XmlNode, XmlRoNode};

/// An XML document.
///
/// The document owns all nodes and provides methods for traversal
/// and manipulation. It uses a flat vector storage with indices
/// for efficient memory usage and cache locality.
#[derive(Clone)]
pub struct XmlDocument {
    /// All nodes in the document (index 0 is always the document node)
    pub(crate) nodes: Arc<RwLock<Vec<NodeData>>>,
    /// Root element node ID (not the document node)
    pub(crate) root_element_id: Option<NodeId>,
    /// Cached namespace resolver for XPath
    pub(crate) namespace_resolver: Arc<RwLock<NamespaceResolver>>,
}

impl XmlDocument {
    /// Creates a new empty document.
    pub fn new() -> Self {
        let mut nodes = Vec::with_capacity(64);
        nodes.push(NodeData::document());

        Self {
            nodes: Arc::new(RwLock::new(nodes)),
            root_element_id: None,
            namespace_resolver: Arc::new(RwLock::new(NamespaceResolver::new())),
        }
    }

    /// Creates a document node reference.
    pub fn document_node(&self) -> XmlNode {
        XmlNode {
            id: 0,
            nodes: Arc::clone(&self.nodes),
        }
    }

    /// Returns the root element node.
    pub fn get_root_element(&self) -> Result<XmlNode> {
        self.root_element_id
            .map(|id| XmlNode {
                id,
                nodes: Arc::clone(&self.nodes),
            })
            .ok_or_else(|| NodeError::NoRootElement.into())
    }

    /// Returns the root element as a read-only node.
    pub fn get_root_element_ro(&self) -> Result<XmlRoNode> {
        self.get_root_element().map(XmlRoNode::from_node)
    }

    /// Returns the namespace resolver for this document.
    pub fn namespace_resolver(&self) -> Arc<RwLock<NamespaceResolver>> {
        Arc::clone(&self.namespace_resolver)
    }

    /// Registers a namespace binding for XPath evaluation.
    pub fn register_namespace(&self, prefix: &str, uri: &str) {
        let mut resolver = self.namespace_resolver.write();
        resolver.register(prefix, uri);
    }

    /// Gets a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        if id < nodes.len() {
            Some(XmlNode {
                id,
                nodes: Arc::clone(&self.nodes),
            })
        } else {
            None
        }
    }

    /// Gets a read-only node by ID.
    pub fn get_node_ro(&self, id: NodeId) -> Option<XmlRoNode> {
        self.get_node(id).map(XmlRoNode::from_node)
    }

    /// Returns the total number of nodes in the document.
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    /// Allocates a new node and returns its ID.
    #[allow(dead_code)]
    pub(crate) fn allocate_node(&self, data: NodeData) -> NodeId {
        let mut nodes = self.nodes.write();
        let id = nodes.len();
        nodes.push(data);
        id
    }

    /// Sets the root element ID.
    #[allow(dead_code)]
    pub(crate) fn set_root_element(&self, _id: NodeId) {
        // Use interior mutability pattern - we need &mut self here
        // This is safe because we're only called during document construction
    }

    /// Adds a child node to a parent.
    #[allow(dead_code)]
    pub(crate) fn add_child(&self, parent_id: NodeId, child_id: NodeId) {
        let mut nodes = self.nodes.write();
        if let Some(child) = nodes.get_mut(child_id) {
            child.parent = Some(parent_id);
        }
        if let Some(parent) = nodes.get_mut(parent_id) {
            parent.children.push(child_id);
        }
    }

    /// Extracts namespace declarations from the root element and registers them.
    pub(crate) fn extract_and_register_namespaces(&self) {
        if let Some(root_id) = self.root_element_id {
            let nodes = self.nodes.read();
            if let Some(root) = nodes.get(root_id) {
                let mut resolver = self.namespace_resolver.write();
                for ns in &root.namespace_decls {
                    resolver.register(ns.prefix(), ns.uri());
                }
            }
        }
    }
}

impl Default for XmlDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for XmlDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nodes = self.nodes.read();
        f.debug_struct("XmlDocument")
            .field("node_count", &nodes.len())
            .field("root_element_id", &self.root_element_id)
            .finish()
    }
}

/// Builder for constructing XML documents.
///
/// This is used internally by the parser but can also be used
/// to programmatically build documents.
pub struct DocumentBuilder {
    document: XmlDocument,
    node_stack: Vec<NodeId>,
    next_id: NodeId,
}

impl DocumentBuilder {
    /// Creates a new document builder.
    pub fn new() -> Self {
        let document = XmlDocument::new();
        Self {
            document,
            node_stack: vec![0], // Start with document node
            next_id: 1,
        }
    }

    /// Returns a reference to the document being built.
    pub fn document(&self) -> &XmlDocument {
        &self.document
    }

    /// Starts a new element.
    #[allow(clippy::too_many_arguments)]
    pub fn start_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        namespace_uri: Option<&str>,
        attributes: Vec<(&str, &str)>,
        namespace_decls: Vec<Namespace>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;

        let mut node = NodeData::element(
            id,
            name.to_string(),
            prefix.map(|s| s.to_string()),
            namespace_uri.map(|s| s.to_string()),
        );

        for (key, value) in attributes {
            node.attributes.insert(key.to_string(), value.to_string());
        }

        node.namespace_decls = namespace_decls;
        node.line = line;
        node.column = column;

        let parent_id = *self.node_stack.last().unwrap_or(&0);

        // Add to document
        {
            let mut nodes = self.document.nodes.write();
            node.parent = Some(parent_id);
            nodes.push(node);

            // Add as child of parent
            if let Some(parent) = nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        // Set as root element if this is the first element
        if self.document.root_element_id.is_none() && parent_id == 0 {
            self.document.root_element_id = Some(id);
        }

        self.node_stack.push(id);
        id
    }

    /// Ends the current element.
    pub fn end_element(&mut self) {
        self.node_stack.pop();
    }

    /// Adds a text node.
    pub fn text(&mut self, content: &str) -> NodeId {
        if content.is_empty() {
            return 0; // Don't add empty text nodes
        }

        let id = self.next_id;
        self.next_id += 1;

        let node = NodeData::text(id, content.to_string());
        let parent_id = *self.node_stack.last().unwrap_or(&0);

        {
            let mut nodes = self.document.nodes.write();
            let mut node = node;
            node.parent = Some(parent_id);
            nodes.push(node);

            if let Some(parent) = nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        id
    }

    /// Adds a CDATA node.
    pub fn cdata(&mut self, content: &str) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;

        let node = NodeData::cdata(id, content.to_string());
        let parent_id = *self.node_stack.last().unwrap_or(&0);

        {
            let mut nodes = self.document.nodes.write();
            let mut node = node;
            node.parent = Some(parent_id);
            nodes.push(node);

            if let Some(parent) = nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        id
    }

    /// Adds a comment node.
    pub fn comment(&mut self, content: &str) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;

        let node = NodeData::comment(id, content.to_string());
        let parent_id = *self.node_stack.last().unwrap_or(&0);

        {
            let mut nodes = self.document.nodes.write();
            let mut node = node;
            node.parent = Some(parent_id);
            nodes.push(node);

            if let Some(parent) = nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        id
    }

    /// Adds a processing instruction.
    pub fn processing_instruction(&mut self, target: &str, content: Option<&str>) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;

        let node = NodeData::processing_instruction(
            id,
            target.to_string(),
            content.map(|s| s.to_string()),
        );
        let parent_id = *self.node_stack.last().unwrap_or(&0);

        {
            let mut nodes = self.document.nodes.write();
            let mut node = node;
            node.parent = Some(parent_id);
            nodes.push(node);

            if let Some(parent) = nodes.get_mut(parent_id) {
                parent.children.push(id);
            }
        }

        id
    }

    /// Finishes building and returns the document.
    pub fn build(self) -> XmlDocument {
        // Extract namespaces from root element
        self.document.extract_and_register_namespaces();
        self.document
    }
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_builder() {
        let mut builder = DocumentBuilder::new();

        builder.start_element(
            "root",
            None,
            None,
            vec![("attr", "value")],
            vec![Namespace::new("ns", "http://example.com")],
            Some(1),
            Some(1),
        );
        builder.text("Hello, ");
        builder.start_element("child", None, None, vec![], vec![], None, None);
        builder.text("World");
        builder.end_element();
        builder.end_element();

        let doc = builder.build();

        assert_eq!(doc.node_count(), 5); // document + root + 2 text + child

        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name(), "root");
        assert_eq!(root.get_attribute("attr"), Some("value".into()));

        let children = root.get_child_elements();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get_name(), "child");
    }
}
