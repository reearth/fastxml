//! Node types and internal data structures.

use indexmap::IndexMap;
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
