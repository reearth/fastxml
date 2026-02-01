//! XML node representation and operations.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use smallvec::SmallVec;

use crate::namespace::Namespace;

/// Node identifier for internal use.
pub type NodeId = usize;

/// Type of XML node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// Document root node
    Document,
    /// Element node (e.g., `<element>`)
    Element,
    /// Text node
    Text,
    /// CDATA section
    CData,
    /// Comment node
    Comment,
    /// Processing instruction
    ProcessingInstruction,
    /// Attribute node (virtual, for XPath)
    Attribute,
}

impl NodeType {
    /// Returns true if this node type can have children.
    pub fn can_have_children(&self) -> bool {
        matches!(self, NodeType::Document | NodeType::Element)
    }
}

/// Internal node data stored in the document.
#[derive(Debug)]
pub(crate) struct NodeData {
    /// Unique node ID
    pub id: NodeId,
    /// Node type
    pub node_type: NodeType,
    /// Local name (for elements and attributes)
    pub name: String,
    /// Namespace prefix (if any)
    pub prefix: Option<String>,
    /// Namespace URI (if any)
    pub namespace_uri: Option<String>,
    /// Text content (for text, CDATA, and comment nodes)
    pub content: Option<String>,
    /// Attributes (for element nodes)
    pub attributes: HashMap<String, String>,
    /// Namespace declarations on this element
    pub namespace_decls: Vec<Namespace>,
    /// Parent node ID
    pub parent: Option<NodeId>,
    /// Child node IDs
    pub children: SmallVec<[NodeId; 4]>,
    /// Line number in source (if available)
    pub line: Option<usize>,
    /// Column number in source (if available)
    pub column: Option<usize>,
}

impl NodeData {
    /// Creates a new document node.
    pub fn document() -> Self {
        Self {
            id: 0,
            node_type: NodeType::Document,
            name: String::new(),
            prefix: None,
            namespace_uri: None,
            content: None,
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new element node.
    pub fn element(
        id: NodeId,
        name: String,
        prefix: Option<String>,
        namespace_uri: Option<String>,
    ) -> Self {
        Self {
            id,
            node_type: NodeType::Element,
            name,
            prefix,
            namespace_uri,
            content: None,
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new text node.
    pub fn text(id: NodeId, content: String) -> Self {
        Self {
            id,
            node_type: NodeType::Text,
            name: String::new(),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new CDATA node.
    pub fn cdata(id: NodeId, content: String) -> Self {
        Self {
            id,
            node_type: NodeType::CData,
            name: String::new(),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new comment node.
    pub fn comment(id: NodeId, content: String) -> Self {
        Self {
            id,
            node_type: NodeType::Comment,
            name: String::new(),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new processing instruction node.
    pub fn processing_instruction(id: NodeId, target: String, content: Option<String>) -> Self {
        Self {
            id,
            node_type: NodeType::ProcessingInstruction,
            name: target,
            prefix: None,
            namespace_uri: None,
            content,
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new attribute node (for XPath evaluation).
    pub fn attribute(id: NodeId, name: String, value: String) -> Self {
        Self {
            id,
            node_type: NodeType::Attribute,
            name,
            prefix: None,
            namespace_uri: None,
            content: Some(value),
            attributes: HashMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Returns the qualified name (prefix:name or just name).
    pub fn qname(&self) -> String {
        match &self.prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, self.name),
            _ => self.name.clone(),
        }
    }
}

/// A reference to a node within a document.
///
/// This is a lightweight handle that can be used to access node data
/// through the document.
#[derive(Clone)]
pub struct XmlNode {
    /// The node ID
    pub(crate) id: NodeId,
    /// Reference to the document's node storage
    pub(crate) nodes: Arc<RwLock<Vec<NodeData>>>,
}

impl XmlNode {
    /// Returns the node ID.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the node type.
    pub fn get_type(&self) -> NodeType {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.node_type)
            .unwrap_or(NodeType::Document)
    }

    /// Returns the local name of the node.
    pub fn get_name(&self) -> String {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.name.clone())
            .unwrap_or_default()
    }

    /// Returns the namespace prefix (if any).
    pub fn get_prefix(&self) -> Option<String> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| n.prefix.clone())
    }

    /// Returns the namespace URI (if any).
    pub fn get_namespace_uri(&self) -> Option<String> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| n.namespace_uri.clone())
    }

    /// Returns the namespace (if any).
    pub fn get_namespace(&self) -> Option<Namespace> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| {
            n.namespace_uri
                .as_ref()
                .map(|uri| Namespace::new(n.prefix.clone().unwrap_or_default(), uri.clone()))
        })
    }

    /// Returns the qualified name (prefix:name or just name).
    pub fn qname(&self) -> String {
        let nodes = self.nodes.read();
        nodes.get(self.id).map(|n| n.qname()).unwrap_or_default()
    }

    /// Returns the text content of the node.
    pub fn get_content(&self) -> Option<String> {
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;

        match node.node_type {
            NodeType::Text | NodeType::CData | NodeType::Comment | NodeType::Attribute => {
                node.content.clone()
            }
            NodeType::Element => {
                // Collect text content from all descendant text nodes
                let mut content = String::new();
                self.collect_text_content_recursive(node.id, &nodes, &mut content);
                if content.is_empty() {
                    None
                } else {
                    Some(content)
                }
            }
            _ => None,
        }
    }

    fn collect_text_content_recursive(
        &self,
        node_id: NodeId,
        nodes: &[NodeData],
        content: &mut String,
    ) {
        if let Some(node) = nodes.get(node_id) {
            match node.node_type {
                NodeType::Text | NodeType::CData => {
                    if let Some(ref text) = node.content {
                        content.push_str(text);
                    }
                }
                NodeType::Element => {
                    for &child_id in &node.children {
                        self.collect_text_content_recursive(child_id, nodes, content);
                    }
                }
                _ => {}
            }
        }
    }

    /// Returns an attribute value by name.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.attributes.get(name).cloned())
    }

    /// Returns an attribute value by name and namespace.
    pub fn get_attribute_ns(&self, name: &str, ns_uri: &str) -> Option<String> {
        // For now, we store attributes with their full qualified names
        // This is a simplified implementation
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;

        // Try exact match first
        if let Some(value) = node.attributes.get(name) {
            return Some(value.clone());
        }

        // Try with namespace prefix lookup
        for ns in &node.namespace_decls {
            if ns.uri() == ns_uri {
                let prefixed_name = format!("{}:{}", ns.prefix(), name);
                if let Some(value) = node.attributes.get(&prefixed_name) {
                    return Some(value.clone());
                }
            }
        }

        None
    }

    /// Returns all attributes as a map.
    pub fn get_attributes(&self) -> HashMap<String, String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.attributes.clone())
            .unwrap_or_default()
    }

    /// Returns namespace declarations on this element.
    pub fn get_namespace_declarations(&self) -> Vec<Namespace> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.namespace_decls.clone())
            .unwrap_or_default()
    }

    /// Returns the parent node (if any).
    pub fn get_parent(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let parent_id = nodes.get(self.id)?.parent?;
        Some(XmlNode {
            id: parent_id,
            nodes: Arc::clone(&self.nodes),
        })
    }

    /// Returns all child nodes.
    pub fn get_child_nodes(&self) -> Vec<XmlNode> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| {
                n.children
                    .iter()
                    .map(|&id| XmlNode {
                        id,
                        nodes: Arc::clone(&self.nodes),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns child element nodes (excluding text, comments, etc.).
    pub fn get_child_elements(&self) -> Vec<XmlNode> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| {
                n.children
                    .iter()
                    .filter_map(|&id| {
                        nodes.get(id).and_then(|child| {
                            if child.node_type == NodeType::Element {
                                Some(XmlNode {
                                    id,
                                    nodes: Arc::clone(&self.nodes),
                                })
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the first child element (if any).
    pub fn first_child(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;
        node.children.first().map(|&id| XmlNode {
            id,
            nodes: Arc::clone(&self.nodes),
        })
    }

    /// Returns the last child element (if any).
    pub fn last_child(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;
        node.children.last().map(|&id| XmlNode {
            id,
            nodes: Arc::clone(&self.nodes),
        })
    }

    /// Returns the line number (if available).
    pub fn line(&self) -> Option<usize> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| n.line)
    }

    /// Returns the column number (if available).
    pub fn column(&self) -> Option<usize> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| n.column)
    }

    /// Sets an attribute value.
    pub fn set_attribute(&self, name: &str, value: &str) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.attributes.insert(name.to_string(), value.to_string());
        }
    }

    /// Removes an attribute by name.
    ///
    /// Returns the previous value if the attribute existed.
    pub fn remove_attribute(&self, name: &str) -> Option<String> {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            return node.attributes.remove(name);
        }
        None
    }

    /// Sets the text content of this node.
    ///
    /// For element nodes, this replaces all children with a single text node.
    /// For text/cdata/comment nodes, this sets the content directly.
    pub fn set_content(&self, content: &str) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            match node.node_type {
                NodeType::Text | NodeType::CData | NodeType::Comment => {
                    node.content = Some(content.to_string());
                }
                NodeType::Element => {
                    // Remove existing children
                    node.children.clear();
                    // Note: We don't actually remove the child nodes from the storage
                    // for simplicity. They become orphaned but that's OK for this use case.
                    node.content = Some(content.to_string());
                }
                _ => {}
            }
        }
    }

    /// Sets the local name of this element.
    pub fn set_name(&self, name: &str) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.name = name.to_string();
        }
    }

    /// Sets the namespace prefix of this element.
    pub fn set_prefix(&self, prefix: Option<&str>) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.prefix = prefix.map(|s| s.to_string());
        }
    }

    /// Sets the namespace URI of this element.
    pub fn set_namespace_uri(&self, uri: Option<&str>) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.namespace_uri = uri.map(|s| s.to_string());
        }
    }

    /// Adds a namespace declaration to this element.
    pub fn add_namespace_decl(&self, prefix: &str, uri: &str) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.namespace_decls
                .push(Namespace::new(prefix.to_string(), uri.to_string()));
        }
    }

    /// Removes all children from this node.
    pub fn clear_children(&self) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.children.clear();
        }
    }

    /// Returns true if this is an element node.
    pub fn is_element(&self) -> bool {
        self.get_type() == NodeType::Element
    }

    /// Returns true if this is a text node.
    pub fn is_text(&self) -> bool {
        self.get_type() == NodeType::Text
    }
}

impl std::fmt::Debug for XmlNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XmlNode")
            .field("id", &self.id)
            .field("type", &self.get_type())
            .field("name", &self.get_name())
            .finish()
    }
}

impl PartialEq for XmlNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.nodes, &other.nodes)
    }
}

impl Eq for XmlNode {}

impl std::hash::Hash for XmlNode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        Arc::as_ptr(&self.nodes).hash(state);
    }
}

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
    pub fn get_attributes(&self) -> HashMap<String, String> {
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
