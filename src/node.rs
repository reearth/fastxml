//! XML node representation and operations.

use std::sync::Arc;

use indexmap::IndexMap;
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
    /// Namespace node (virtual, for XPath)
    Namespace,
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
    /// Uses IndexMap to preserve insertion order (XML source order)
    pub attributes: IndexMap<String, String>,
    /// Namespace info for attributes: local_name → (prefix, namespace_uri)
    /// Only populated for element nodes with namespaced attributes.
    pub attribute_ns_info: IndexMap<String, (String, String)>,
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new attribute node (for XPath evaluation).
    pub fn attribute(
        id: NodeId,
        name: String,
        value: String,
        prefix: Option<String>,
        namespace_uri: Option<String>,
    ) -> Self {
        Self {
            id,
            node_type: NodeType::Attribute,
            name,
            prefix,
            namespace_uri,
            content: Some(value),
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
            namespace_decls: Vec::new(),
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new namespace node (for XPath evaluation).
    ///
    /// In XPath, namespace nodes have:
    /// - name: the namespace prefix (or empty string for default namespace)
    /// - content: the namespace URI
    pub fn namespace_node(id: NodeId, prefix: String, uri: String) -> Self {
        Self {
            id,
            node_type: NodeType::Namespace,
            name: prefix,
            prefix: None,
            namespace_uri: None,
            content: Some(uri),
            attributes: IndexMap::new(),
            attribute_ns_info: IndexMap::new(),
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
            NodeType::Text
            | NodeType::CData
            | NodeType::Comment
            | NodeType::Attribute
            | NodeType::Namespace => node.content.clone(),
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
    pub fn get_attributes(&self) -> IndexMap<String, String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.attributes.clone())
            .unwrap_or_default()
    }

    /// Returns namespace info for a specific attribute by local name.
    /// Returns (prefix, namespace_uri) if the attribute is namespaced.
    pub fn get_attribute_ns_info(&self, local_name: &str) -> Option<(String, String)> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.attribute_ns_info.get(local_name).cloned())
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
            return node.attributes.shift_remove(name);
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

#[cfg(test)]
mod tests {
    use super::*;

    // NodeType tests
    #[test]
    fn test_node_type_can_have_children() {
        assert!(NodeType::Document.can_have_children());
        assert!(NodeType::Element.can_have_children());
        assert!(!NodeType::Text.can_have_children());
        assert!(!NodeType::CData.can_have_children());
        assert!(!NodeType::Comment.can_have_children());
        assert!(!NodeType::ProcessingInstruction.can_have_children());
        assert!(!NodeType::Attribute.can_have_children());
        assert!(!NodeType::Namespace.can_have_children());
    }

    #[test]
    fn test_node_type_debug() {
        let debug = format!("{:?}", NodeType::Element);
        assert_eq!(debug, "Element");
    }

    #[test]
    fn test_node_type_clone_eq() {
        let t1 = NodeType::Element;
        let t2 = t1;
        assert_eq!(t1, t2);
    }

    // NodeData tests
    #[test]
    fn test_node_data_document() {
        let node = NodeData::document();
        assert_eq!(node.id, 0);
        assert_eq!(node.node_type, NodeType::Document);
        assert!(node.name.is_empty());
        assert!(node.prefix.is_none());
        assert!(node.namespace_uri.is_none());
        assert!(node.content.is_none());
        assert!(node.attributes.is_empty());
        assert!(node.namespace_decls.is_empty());
        assert!(node.parent.is_none());
        assert!(node.children.is_empty());
    }

    #[test]
    fn test_node_data_element() {
        let node = NodeData::element(
            1,
            "test".to_string(),
            Some("ns".to_string()),
            Some("http://example.com".to_string()),
        );
        assert_eq!(node.id, 1);
        assert_eq!(node.node_type, NodeType::Element);
        assert_eq!(node.name, "test");
        assert_eq!(node.prefix, Some("ns".to_string()));
        assert_eq!(node.namespace_uri, Some("http://example.com".to_string()));
    }

    #[test]
    fn test_node_data_text() {
        let node = NodeData::text(2, "hello world".to_string());
        assert_eq!(node.id, 2);
        assert_eq!(node.node_type, NodeType::Text);
        assert_eq!(node.content, Some("hello world".to_string()));
    }

    #[test]
    fn test_node_data_cdata() {
        let node = NodeData::cdata(3, "<special>".to_string());
        assert_eq!(node.id, 3);
        assert_eq!(node.node_type, NodeType::CData);
        assert_eq!(node.content, Some("<special>".to_string()));
    }

    #[test]
    fn test_node_data_comment() {
        let node = NodeData::comment(4, "a comment".to_string());
        assert_eq!(node.id, 4);
        assert_eq!(node.node_type, NodeType::Comment);
        assert_eq!(node.content, Some("a comment".to_string()));
    }

    #[test]
    fn test_node_data_processing_instruction() {
        let node = NodeData::processing_instruction(
            5,
            "xml".to_string(),
            Some("version=\"1.0\"".to_string()),
        );
        assert_eq!(node.id, 5);
        assert_eq!(node.node_type, NodeType::ProcessingInstruction);
        assert_eq!(node.name, "xml");
        assert_eq!(node.content, Some("version=\"1.0\"".to_string()));
    }

    #[test]
    fn test_node_data_processing_instruction_no_content() {
        let node = NodeData::processing_instruction(6, "target".to_string(), None);
        assert_eq!(node.node_type, NodeType::ProcessingInstruction);
        assert_eq!(node.name, "target");
        assert!(node.content.is_none());
    }

    #[test]
    fn test_node_data_attribute() {
        let node = NodeData::attribute(7, "id".to_string(), "123".to_string(), None, None);
        assert_eq!(node.id, 7);
        assert_eq!(node.node_type, NodeType::Attribute);
        assert_eq!(node.name, "id");
        assert_eq!(node.content, Some("123".to_string()));
    }

    #[test]
    fn test_node_data_namespace_node() {
        let node = NodeData::namespace_node(
            8,
            "gml".to_string(),
            "http://www.opengis.net/gml".to_string(),
        );
        assert_eq!(node.id, 8);
        assert_eq!(node.node_type, NodeType::Namespace);
        assert_eq!(node.name, "gml");
        assert_eq!(node.content, Some("http://www.opengis.net/gml".to_string()));
    }

    #[test]
    fn test_node_data_qname_with_prefix() {
        let node = NodeData::element(1, "name".to_string(), Some("ns".to_string()), None);
        assert_eq!(node.qname(), "ns:name");
    }

    #[test]
    fn test_node_data_qname_without_prefix() {
        let node = NodeData::element(1, "name".to_string(), None, None);
        assert_eq!(node.qname(), "name");
    }

    #[test]
    fn test_node_data_qname_with_empty_prefix() {
        let node = NodeData::element(1, "name".to_string(), Some("".to_string()), None);
        assert_eq!(node.qname(), "name");
    }

    #[test]
    fn test_node_data_debug() {
        let node = NodeData::element(1, "test".to_string(), None, None);
        let debug = format!("{:?}", node);
        assert!(debug.contains("NodeData"));
        assert!(debug.contains("test"));
    }

    // XmlNode tests (using parse helper)
    fn create_test_document() -> crate::document::XmlDocument {
        crate::parse("<root attr=\"val\"><child>text</child></root>").unwrap()
    }

    #[test]
    fn test_xml_node_id() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        // Root node should have id > 0 (document is 0)
        assert!(root.id() > 0);
    }

    #[test]
    fn test_xml_node_get_type() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        assert_eq!(root.get_type(), NodeType::Element);
    }

    #[test]
    fn test_xml_node_get_name() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        assert_eq!(root.get_name(), "root");
    }

    #[test]
    fn test_xml_node_get_attribute() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        assert_eq!(root.get_attribute("attr"), Some("val".to_string()));
        assert_eq!(root.get_attribute("nonexistent"), None);
    }

    #[test]
    fn test_xml_node_get_attributes() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let attrs = root.get_attributes();
        assert_eq!(attrs.get("attr"), Some(&"val".to_string()));
    }

    #[test]
    fn test_xml_node_get_child_nodes() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let children = root.get_child_nodes();
        assert!(!children.is_empty());
    }

    #[test]
    fn test_xml_node_get_child_elements() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let children = root.get_child_elements();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get_name(), "child");
    }

    #[test]
    fn test_xml_node_first_child() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let first = root.first_child();
        assert!(first.is_some());
    }

    #[test]
    fn test_xml_node_last_child() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let last = root.last_child();
        assert!(last.is_some());
    }

    #[test]
    fn test_xml_node_get_parent() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        let parent = child.get_parent();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().get_name(), "root");
    }

    #[test]
    fn test_xml_node_get_content_element() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        assert_eq!(child.get_content(), Some("text".to_string()));
    }

    #[test]
    fn test_xml_node_is_element() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        assert!(root.is_element());
    }

    #[test]
    fn test_xml_node_is_text() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        let text_node = child.get_child_nodes().into_iter().find(|n| n.is_text());
        assert!(text_node.is_some());
    }

    #[test]
    fn test_xml_node_set_attribute() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_attribute("new_attr", "new_value");
        assert_eq!(
            root.get_attribute("new_attr"),
            Some("new_value".to_string())
        );
    }

    #[test]
    fn test_xml_node_remove_attribute() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let removed = root.remove_attribute("attr");
        assert_eq!(removed, Some("val".to_string()));
        assert_eq!(root.get_attribute("attr"), None);
    }

    #[test]
    fn test_xml_node_remove_attribute_nonexistent() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let removed = root.remove_attribute("nonexistent");
        assert_eq!(removed, None);
    }

    #[test]
    fn test_xml_node_set_content() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        child.set_content("new content");
        // After set_content, children are cleared and content is set directly
    }

    #[test]
    fn test_xml_node_set_name() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_name("newroot");
        assert_eq!(root.get_name(), "newroot");
    }

    #[test]
    fn test_xml_node_set_prefix() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_prefix(Some("ns"));
        assert_eq!(root.get_prefix(), Some("ns".to_string()));
    }

    #[test]
    fn test_xml_node_set_prefix_none() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_prefix(None);
        assert_eq!(root.get_prefix(), None);
    }

    #[test]
    fn test_xml_node_set_namespace_uri() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_namespace_uri(Some("http://example.com"));
        assert_eq!(
            root.get_namespace_uri(),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn test_xml_node_add_namespace_decl() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.add_namespace_decl("ns", "http://example.com");
        let decls = root.get_namespace_declarations();
        assert!(
            decls
                .iter()
                .any(|d| d.prefix() == "ns" && d.uri() == "http://example.com")
        );
    }

    #[test]
    fn test_xml_node_clear_children() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        assert!(!root.get_child_nodes().is_empty());
        root.clear_children();
        assert!(root.get_child_nodes().is_empty());
    }

    #[test]
    fn test_xml_node_qname() {
        let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        assert_eq!(root.qname(), "ns:root");
    }

    #[test]
    fn test_xml_node_debug() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let debug = format!("{:?}", root);
        assert!(debug.contains("XmlNode"));
        assert!(debug.contains("root"));
    }

    #[test]
    fn test_xml_node_clone() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let cloned = root.clone();
        assert_eq!(root.id(), cloned.id());
        assert_eq!(root.get_name(), cloned.get_name());
    }

    #[test]
    fn test_xml_node_eq() {
        let doc = create_test_document();
        let root1 = crate::get_root_node(&doc).unwrap();
        let root2 = crate::get_root_node(&doc).unwrap();
        assert_eq!(root1, root2);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn test_xml_node_hash() {
        use std::collections::HashSet;
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let mut set = HashSet::new();
        set.insert(root.clone());
        assert!(set.contains(&root));
    }

    // XmlRoNode tests
    #[test]
    fn test_xml_ro_node_from_node() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root.clone());
        assert_eq!(ro_node.id(), root.id());
    }

    #[test]
    fn test_xml_ro_node_get_type() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(ro_node.get_type(), NodeType::Element);
    }

    #[test]
    fn test_xml_ro_node_get_name() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(ro_node.get_name(), "root");
    }

    #[test]
    fn test_xml_ro_node_get_prefix() {
        let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(ro_node.get_prefix(), Some("ns".to_string()));
    }

    #[test]
    fn test_xml_ro_node_get_namespace_uri() {
        // Test with manually set namespace URI
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_namespace_uri(Some("http://example.com"));
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(
            ro_node.get_namespace_uri(),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn test_xml_ro_node_get_namespace() {
        // Test with manually set namespace
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        root.set_prefix(Some("ns"));
        root.set_namespace_uri(Some("http://example.com"));
        let ro_node = XmlRoNode::from_node(root);
        let ns = ro_node.get_namespace();
        assert!(ns.is_some());
        let ns = ns.unwrap();
        assert_eq!(ns.prefix(), "ns");
        assert_eq!(ns.uri(), "http://example.com");
    }

    #[test]
    fn test_xml_ro_node_qname() {
        let doc = crate::parse("<ns:root xmlns:ns=\"http://example.com\"/>").unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(ro_node.qname(), "ns:root");
    }

    #[test]
    fn test_xml_ro_node_get_content() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        let ro_node = XmlRoNode::from_node(child);
        assert_eq!(ro_node.get_content(), Some("text".to_string()));
    }

    #[test]
    fn test_xml_ro_node_get_attribute() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert_eq!(ro_node.get_attribute("attr"), Some("val".to_string()));
    }

    #[test]
    fn test_xml_ro_node_get_attributes() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let attrs = ro_node.get_attributes();
        assert!(attrs.contains_key("attr"));
    }

    #[test]
    fn test_xml_ro_node_get_namespace_declarations() {
        let doc = crate::parse("<root xmlns:ns=\"http://example.com\"/>").unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let decls = ro_node.get_namespace_declarations();
        assert!(decls.iter().any(|d| d.prefix() == "ns"));
    }

    #[test]
    fn test_xml_ro_node_get_parent() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        let ro_child = XmlRoNode::from_node(child);
        let parent = ro_child.get_parent();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().get_name(), "root");
    }

    #[test]
    fn test_xml_ro_node_get_child_nodes() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let children = ro_node.get_child_nodes();
        assert!(!children.is_empty());
    }

    #[test]
    fn test_xml_ro_node_get_child_elements() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let children = ro_node.get_child_elements();
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_xml_ro_node_first_child() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert!(ro_node.first_child().is_some());
    }

    #[test]
    fn test_xml_ro_node_last_child() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert!(ro_node.last_child().is_some());
    }

    #[test]
    fn test_xml_ro_node_is_element() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        assert!(ro_node.is_element());
    }

    #[test]
    fn test_xml_ro_node_is_text() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let child = root.get_child_elements()[0].clone();
        let text_node = child
            .get_child_nodes()
            .into_iter()
            .find(|n| n.is_text())
            .unwrap();
        let ro_node = XmlRoNode::from_node(text_node);
        assert!(ro_node.is_text());
    }

    #[test]
    fn test_xml_ro_node_into_node() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root.clone());
        let back_to_node = ro_node.into_node();
        assert_eq!(back_to_node.id(), root.id());
    }

    #[test]
    fn test_xml_ro_node_debug() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let debug = format!("{:?}", ro_node);
        assert!(debug.contains("XmlRoNode"));
        assert!(debug.contains("root"));
    }

    #[test]
    fn test_xml_ro_node_clone() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let cloned = ro_node.clone();
        assert_eq!(ro_node.id(), cloned.id());
    }

    #[test]
    fn test_xml_ro_node_eq() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro1 = XmlRoNode::from_node(root.clone());
        let ro2 = XmlRoNode::from_node(root);
        assert_eq!(ro1, ro2);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn test_xml_ro_node_hash() {
        use std::collections::HashSet;
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let mut set = HashSet::new();
        set.insert(ro_node.clone());
        assert!(set.contains(&ro_node));
    }

    // get_attribute_ns tests
    #[test]
    fn test_xml_node_get_attribute_ns() {
        let doc = crate::parse(r#"<root xmlns:ns="http://example.com" ns:attr="value"/>"#).unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        // Attributes are stored with local names only (libxml compatible)
        assert_eq!(root.get_attribute("attr"), Some("value".to_string()));
    }

    #[test]
    fn test_xml_ro_node_get_attribute_ns() {
        let doc = crate::parse(r#"<root xmlns:ns="http://example.com" ns:attr="value"/>"#).unwrap();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        // get_attribute_ns searches by namespace URI
        let result = ro_node.get_attribute_ns("attr", "http://example.com");
        assert_eq!(result, Some("value".to_string()));
        // Attributes are keyed by local name only (libxml compatible)
        let attrs = ro_node.get_attributes();
        assert!(attrs.contains_key("attr"));
    }

    // line and column tests (usually None unless explicitly set during parsing)
    #[test]
    fn test_xml_node_line_column() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        // Line/column may or may not be available depending on parser config
        let _ = root.line();
        let _ = root.column();
    }

    #[test]
    fn test_xml_ro_node_line_column() {
        let doc = create_test_document();
        let root = crate::get_root_node(&doc).unwrap();
        let ro_node = XmlRoNode::from_node(root);
        let _ = ro_node.line();
        let _ = ro_node.column();
    }
}
