//! Mutable XML node reference.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::RwLock;

use crate::namespace::Namespace;

use super::types::{NodeData, NodeId, NodeType};

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
            .map(|n| n.name.to_string())
            .unwrap_or_default()
    }

    /// Returns the namespace prefix (if any).
    pub fn get_prefix(&self) -> Option<String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.prefix.as_deref().map(str::to_string))
    }

    /// Returns the namespace URI (if any).
    pub fn get_namespace_uri(&self) -> Option<String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.namespace_uri.as_deref().map(str::to_string))
    }

    /// Returns the namespace (if any).
    pub fn get_namespace(&self) -> Option<Namespace> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| {
            n.namespace_uri
                .as_ref()
                .map(|uri| Namespace::new(n.prefix.as_deref().unwrap_or_default(), uri.as_ref()))
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
                self.collect_text_content_recursive(self.id, &nodes, &mut content);
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
                    for child_id in node.child_ids() {
                        self.collect_text_content_recursive(child_id, nodes, content);
                    }
                }
                _ => {}
            }
        }
    }

    /// Returns an attribute value by name.
    ///
    /// `name` may be a qualified name as written in the document
    /// (`"gml:id"`, `"xml:lang"`), which matches exactly that prefix and
    /// local name, or a bare local name (`"id"`). A bare local name returns
    /// the unprefixed attribute of that name if the element has one;
    /// otherwise it returns the first attribute in document order whose
    /// local name matches, whatever its prefix (so `"id"` finds `gml:id`).
    /// Use [`get_attribute_ns`](Self::get_attribute_ns) to select by
    /// namespace when several attributes share a local name.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.attr(name).map(str::to_string))
    }

    /// Returns the value of the attribute with local name `name` in the
    /// namespace `ns_uri`. An empty `ns_uri` selects an attribute in no
    /// namespace.
    pub fn get_attribute_ns(&self, name: &str, ns_uri: &str) -> Option<String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)?
            .attr_ns(name, ns_uri)
            .map(str::to_string)
    }

    /// Returns all attributes as a map, in document order.
    ///
    /// Keys are local names, except that when several attributes on the
    /// element share a local name, each prefixed one among them is keyed by
    /// its qualified name (`<e a:x="1" x="2">` gives keys `a:x` and `x`), so
    /// no attribute is dropped. Every key can be passed back to
    /// [`get_attribute`](Self::get_attribute) and
    /// [`get_attribute_ns_info`](Self::get_attribute_ns_info).
    pub fn get_attributes(&self) -> IndexMap<String, String> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| {
                n.attr_entries()
                    .map(|(key, a)| (key, a.value.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the attributes with full name information, in document order.
    pub(crate) fn attrs(&self) -> Vec<crate::node::types::Attr> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.attrs().to_vec())
            .unwrap_or_default()
    }

    /// Returns (prefix, namespace_uri) for a namespaced attribute, looked up
    /// by name the same way as [`get_attribute`](Self::get_attribute).
    /// Returns `None` when the attribute is absent or has no namespace.
    pub fn get_attribute_ns_info(&self, name: &str) -> Option<(String, String)> {
        let nodes = self.nodes.read();
        nodes.get(self.id).and_then(|n| {
            n.attr_ns_info(name)
                .map(|(p, u)| (p.to_string(), u.to_string()))
        })
    }

    /// Returns namespace declarations on this element.
    pub fn get_namespace_declarations(&self) -> Vec<Namespace> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .map(|n| n.ns_decls().to_vec())
            .unwrap_or_default()
    }

    /// Returns the parent node (if any).
    pub fn get_parent(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let parent_id = nodes.get(self.id)?.parent()?;
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
                n.child_ids()
                    .map(|id| XmlNode {
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
                n.child_ids()
                    .filter_map(|id| {
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

    /// Returns the first child node of any type (if any).
    pub fn first_child(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;
        node.child_ids().next().map(|id| XmlNode {
            id,
            nodes: Arc::clone(&self.nodes),
        })
    }

    /// Returns the last child node of any type (if any).
    pub fn last_child(&self) -> Option<XmlNode> {
        let nodes = self.nodes.read();
        let node = nodes.get(self.id)?;
        node.child_ids().next_back().map(|id| XmlNode {
            id,
            nodes: Arc::clone(&self.nodes),
        })
    }

    /// Returns the line number (if available).
    pub fn line(&self) -> Option<usize> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.line.map(|v| v.get() as usize))
    }

    /// Returns the column number (if available).
    pub fn column(&self) -> Option<usize> {
        let nodes = self.nodes.read();
        nodes
            .get(self.id)
            .and_then(|n| n.column.map(|v| v.get() as usize))
    }

    /// Sets an attribute value.
    ///
    /// If [`get_attribute(name)`](Self::get_attribute) finds an attribute,
    /// its value is replaced and its prefix and namespace are kept (so
    /// setting `"id"` on an element with `gml:id` updates `gml:id`).
    /// Otherwise a new attribute is added; a qualified `name` keeps its
    /// prefix, resolved against the namespace declarations in scope on this
    /// element and its ancestors.
    pub fn set_attribute(&self, name: &str, value: &str) {
        let mut nodes = self.nodes.write();
        if let Some(existing) = nodes.get(self.id).and_then(|n| n.find_attr(name)) {
            let mut attr = existing.clone();
            attr.value = Box::from(value);
            if let Some(node) = nodes.get_mut(self.id) {
                node.set_attr(attr);
            }
            return;
        }
        let (prefix, local) = crate::namespace::split_qname(name);
        let ns_uri = prefix.and_then(|p| resolve_in_scope(&nodes, self.id, p));
        if let Some(node) = nodes.get_mut(self.id) {
            node.set_attr(crate::node::types::Attr {
                name: std::sync::Arc::from(local),
                value: Box::from(value),
                prefix: prefix.map(std::sync::Arc::from),
                ns_uri: ns_uri.map(|u| std::sync::Arc::from(u.as_str())),
            });
        }
    }

    /// Removes an attribute, looked up by name the same way as
    /// [`get_attribute`](Self::get_attribute).
    ///
    /// Returns the previous value if the attribute existed.
    pub fn remove_attribute(&self, name: &str) -> Option<String> {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            return node.remove_attr(name).map(String::from);
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
            node.name = std::sync::Arc::from(name);
        }
    }

    /// Sets the namespace prefix of this element.
    pub fn set_prefix(&self, prefix: Option<&str>) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.prefix = prefix.map(std::sync::Arc::from);
        }
    }

    /// Sets the namespace URI of this element.
    pub fn set_namespace_uri(&self, uri: Option<&str>) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.namespace_uri = uri.map(std::sync::Arc::from);
        }
    }

    /// Adds a namespace declaration to this element.
    pub fn add_namespace_decl(&self, prefix: &str, uri: &str) {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(self.id) {
            node.ns_decls_mut()
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

/// Resolves `prefix` against the namespace declarations on node `id` and its
/// ancestors (`xml` is always bound).
fn resolve_in_scope(nodes: &[NodeData], id: NodeId, prefix: &str) -> Option<String> {
    if prefix == "xml" {
        return Some(crate::namespace::common::XML_NS.to_string());
    }
    let mut current = Some(id);
    while let Some(n) = current.and_then(|i| nodes.get(i)) {
        if let Some(ns) = n.ns_decls().iter().find(|ns| ns.prefix() == prefix) {
            return (!ns.uri().is_empty()).then(|| ns.uri().to_string());
        }
        current = n.parent();
    }
    None
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
