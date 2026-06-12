//! Node types and internal data structures.

use std::num::NonZeroU32;
use std::sync::Arc;

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
    /// Local name (for elements and attributes); interned so repeated
    /// names share one allocation
    pub name: Arc<str>,
    /// Namespace prefix (if any)
    pub prefix: Option<Arc<str>>,
    /// Namespace URI (if any)
    pub namespace_uri: Option<Arc<str>>,
    /// Text content (for text, CDATA, and comment nodes)
    pub content: Option<String>,
    /// Attribute and namespace data, boxed because most nodes (all text
    /// nodes, attribute-less elements) carry none — this keeps the inline
    /// size of `NodeData` small, which dominates DOM memory.
    pub extra: Option<Box<NodeExtra>>,
    /// Parent node ID
    pub parent: Option<NodeId>,
    /// Child node IDs
    pub children: SmallVec<[NodeId; 4]>,
    /// Line number in source (if available)
    pub line: Option<NonZeroU32>,
    /// Column number in source (if available)
    pub column: Option<NonZeroU32>,
}

/// Per-node data that only some nodes carry (attributes, namespace
/// declarations); boxed out of [`NodeData`] to keep the node array compact.
#[derive(Debug, Default)]
pub(crate) struct NodeExtra {
    /// Attributes (for element nodes); IndexMap preserves source order
    pub attributes: IndexMap<String, String>,
    /// Namespace info for attributes: local_name → (prefix, namespace_uri)
    pub attribute_ns_info: IndexMap<String, (String, String)>,
    /// Namespace declarations on this element
    pub namespace_decls: Vec<Namespace>,
}

impl NodeData {
    /// The node's attributes, or a shared empty map when it has none.
    pub fn attrs(&self) -> &IndexMap<String, String> {
        static EMPTY: std::sync::OnceLock<IndexMap<String, String>> = std::sync::OnceLock::new();
        match &self.extra {
            Some(extra) => &extra.attributes,
            None => EMPTY.get_or_init(IndexMap::new),
        }
    }

    /// Mutable access to the attributes, allocating the extra block on
    /// first use.
    pub fn attrs_mut(&mut self) -> &mut IndexMap<String, String> {
        &mut self.extra.get_or_insert_default().attributes
    }

    /// Attribute namespace info, or a shared empty map.
    pub fn attr_ns_info(&self) -> &IndexMap<String, (String, String)> {
        static EMPTY: std::sync::OnceLock<IndexMap<String, (String, String)>> =
            std::sync::OnceLock::new();
        match &self.extra {
            Some(extra) => &extra.attribute_ns_info,
            None => EMPTY.get_or_init(IndexMap::new),
        }
    }

    /// Mutable attribute namespace info, allocating on first use.
    pub fn attr_ns_info_mut(&mut self) -> &mut IndexMap<String, (String, String)> {
        &mut self.extra.get_or_insert_default().attribute_ns_info
    }

    /// Namespace declarations on this node (empty slice when none).
    pub fn ns_decls(&self) -> &[Namespace] {
        self.extra
            .as_deref()
            .map(|e| e.namespace_decls.as_slice())
            .unwrap_or(&[])
    }

    /// Mutable namespace declarations, allocating on first use.
    pub fn ns_decls_mut(&mut self) -> &mut Vec<Namespace> {
        &mut self.extra.get_or_insert_default().namespace_decls
    }

    /// Creates a new document node.
    pub fn document() -> Self {
        Self {
            id: 0,
            node_type: NodeType::Document,
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: None,
            extra: None,
            parent: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new element node.
    pub fn element(
        id: NodeId,
        name: Arc<str>,
        prefix: Option<Arc<str>>,
        namespace_uri: Option<Arc<str>>,
    ) -> Self {
        Self {
            id,
            node_type: NodeType::Element,
            name,
            prefix,
            namespace_uri,
            content: None,
            extra: None,
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
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
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
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
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
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
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
            name: Arc::from(target.as_str()),
            prefix: None,
            namespace_uri: None,
            content,
            extra: None,
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
            name: Arc::from(name.as_str()),
            prefix: prefix.map(|p| Arc::from(p.as_str())),
            namespace_uri: namespace_uri.map(|n| Arc::from(n.as_str())),
            content: Some(value),
            extra: None,
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
            name: Arc::from(prefix.as_str()),
            prefix: None,
            namespace_uri: None,
            content: Some(uri),
            extra: None,
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
            _ => self.name.to_string(),
        }
    }
}

#[cfg(test)]
mod size_tests {
    /// DOM memory is dominated by the node array; keep `NodeData` compact.
    #[test]
    fn node_data_stays_compact() {
        assert!(
            std::mem::size_of::<super::NodeData>() <= 176,
            "NodeData grew to {} bytes; keep rarely-used fields in NodeExtra",
            std::mem::size_of::<super::NodeData>()
        );
    }
}
