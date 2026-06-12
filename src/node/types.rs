//! Node types and internal data structures.

use std::num::NonZeroU32;
use std::sync::Arc;

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
///
/// A node's ID is its index in the document's node array, so it is not
/// stored here. Parent/child links are `u32`, capping documents at
/// `u32::MAX` nodes — the array itself would be hundreds of gigabytes
/// before that limit matters.
#[derive(Debug)]
pub(crate) struct NodeData {
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
    /// Parent node ID, stored +1 so the document node (ID 0) fits the
    /// `NonZeroU32` niche; use [`NodeData::parent`].
    parent_plus1: Option<NonZeroU32>,
    /// Child node IDs
    pub children: SmallVec<[u32; 4]>,
    /// Line number in source (if available)
    pub line: Option<NonZeroU32>,
    /// Column number in source (if available)
    pub column: Option<NonZeroU32>,
}

/// One attribute on an element.
///
/// `name` is the local name — prefixed attributes are stored under their
/// local name (libxml-compatible), with `prefix`/`ns_uri` set when the
/// attribute is namespaced. Names are interned so repeated attribute names
/// share one allocation.
#[derive(Debug, Clone)]
pub(crate) struct Attr {
    pub name: Arc<str>,
    pub value: Box<str>,
    pub prefix: Option<Arc<str>>,
    pub ns_uri: Option<Arc<str>>,
}

/// Per-node data that only some nodes carry (attributes, namespace
/// declarations); boxed out of [`NodeData`] to keep the node array compact.
/// Attributes are a small ordered list, not a map: most elements have a
/// handful of attributes, where a linear scan beats a hash table and costs
/// far less memory.
#[derive(Debug, Default)]
pub(crate) struct NodeExtra {
    /// Attributes (for element nodes), in source order
    pub attributes: SmallVec<[Attr; 1]>,
    /// Namespace declarations on this element
    pub namespace_decls: Vec<Namespace>,
}

impl NodeData {
    /// The node's attributes in source order (empty slice when none).
    pub fn attrs(&self) -> &[Attr] {
        self.extra
            .as_deref()
            .map(|e| e.attributes.as_slice())
            .unwrap_or(&[])
    }

    /// Looks up an attribute value by name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs()
            .iter()
            .find(|a| a.name.as_ref() == name)
            .map(|a| a.value.as_ref())
    }

    /// Inserts or replaces an attribute, preserving the original position
    /// on replace (IndexMap-compatible semantics).
    pub fn set_attr(&mut self, attr: Attr) {
        let attrs = &mut self.extra.get_or_insert_default().attributes;
        match attrs.iter_mut().find(|a| a.name == attr.name) {
            Some(existing) => *existing = attr,
            None => attrs.push(attr),
        }
    }

    /// Removes an attribute by name, preserving the order of the rest.
    /// Returns the removed value.
    pub fn remove_attr(&mut self, name: &str) -> Option<Box<str>> {
        let attrs = &mut self.extra.as_deref_mut()?.attributes;
        let pos = attrs.iter().position(|a| a.name.as_ref() == name)?;
        Some(attrs.remove(pos).value)
    }

    /// Namespace info for an attribute: (prefix, namespace_uri) if the
    /// attribute exists and is namespaced.
    pub fn attr_ns_info(&self, local_name: &str) -> Option<(&str, &str)> {
        self.attrs()
            .iter()
            .find(|a| a.name.as_ref() == local_name)
            .and_then(|a| Some((a.prefix.as_deref()?, a.ns_uri.as_deref()?)))
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

    /// The parent node's ID, if any.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent_plus1.map(|p| (p.get() - 1) as NodeId)
    }

    /// Sets the parent node ID.
    pub fn set_parent(&mut self, id: Option<NodeId>) {
        self.parent_plus1 = id.map(|i| {
            NonZeroU32::new(u32::try_from(i + 1).expect("document exceeds u32::MAX nodes"))
                .expect("id + 1 is nonzero")
        });
    }

    /// Appends a child node ID.
    pub fn push_child(&mut self, id: NodeId) {
        self.children
            .push(u32::try_from(id).expect("document exceeds u32::MAX nodes"));
    }

    /// The child node IDs in order.
    pub fn child_ids(&self) -> impl DoubleEndedIterator<Item = NodeId> + ExactSizeIterator + '_ {
        self.children.iter().map(|&c| c as NodeId)
    }

    /// Creates a new document node.
    pub fn document() -> Self {
        Self {
            node_type: NodeType::Document,
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: None,
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new element node.
    pub fn element(
        name: Arc<str>,
        prefix: Option<Arc<str>>,
        namespace_uri: Option<Arc<str>>,
    ) -> Self {
        Self {
            node_type: NodeType::Element,
            name,
            prefix,
            namespace_uri,
            content: None,
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new text node.
    pub fn text(content: String) -> Self {
        Self {
            node_type: NodeType::Text,
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new CDATA node.
    pub fn cdata(content: String) -> Self {
        Self {
            node_type: NodeType::CData,
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new comment node.
    pub fn comment(content: String) -> Self {
        Self {
            node_type: NodeType::Comment,
            name: Arc::from(""),
            prefix: None,
            namespace_uri: None,
            content: Some(content),
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new processing instruction node.
    pub fn processing_instruction(target: String, content: Option<String>) -> Self {
        Self {
            node_type: NodeType::ProcessingInstruction,
            name: Arc::from(target.as_str()),
            prefix: None,
            namespace_uri: None,
            content,
            extra: None,
            parent_plus1: None,
            children: SmallVec::new(),
            line: None,
            column: None,
        }
    }

    /// Creates a new attribute node (for XPath evaluation).
    pub fn attribute(
        name: String,
        value: String,
        prefix: Option<String>,
        namespace_uri: Option<String>,
    ) -> Self {
        Self {
            node_type: NodeType::Attribute,
            name: Arc::from(name.as_str()),
            prefix: prefix.map(|p| Arc::from(p.as_str())),
            namespace_uri: namespace_uri.map(|n| Arc::from(n.as_str())),
            content: Some(value),
            extra: None,
            parent_plus1: None,
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
    pub fn namespace_node(prefix: String, uri: String) -> Self {
        Self {
            node_type: NodeType::Namespace,
            name: Arc::from(prefix.as_str()),
            prefix: None,
            namespace_uri: None,
            content: Some(uri),
            extra: None,
            parent_plus1: None,
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
            std::mem::size_of::<super::NodeData>() <= 128,
            "NodeData grew to {} bytes; keep rarely-used fields in NodeExtra",
            std::mem::size_of::<super::NodeData>()
        );
        println!("NodeData: {} bytes", std::mem::size_of::<super::NodeData>());
    }
}
