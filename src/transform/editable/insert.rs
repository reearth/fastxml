//! Applying structural modifications ([`NewNode`] insertion, text
//! replacement) to the subtree held by an [`EditableNode`](super::EditableNode).

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::XmlDocument;
use crate::node::types::{Attr, NodeData};
use crate::node::{NodeId, NodeType};

use super::NewNode;

/// Where a new child is inserted among the parent's children.
#[derive(Clone, Copy)]
pub(super) enum InsertAt {
    First,
    Last,
}

/// Creates `node` (recursively) in `doc` and attaches it under `parent`.
///
/// A prefixed element gets the namespace URI registered for its prefix in
/// `namespaces`, if any.
pub(super) fn insert_new_node(
    doc: &XmlDocument,
    parent: NodeId,
    node: &NewNode,
    at: InsertAt,
    namespaces: &HashMap<String, String>,
) {
    let mut nodes = doc.nodes.write();
    let id = create_node(&mut nodes, parent, node, namespaces);
    let children = &mut nodes[parent].children;
    let id = u32::try_from(id).expect("document exceeds u32::MAX nodes");
    match at {
        InsertAt::First => children.insert(0, id),
        InsertAt::Last => children.push(id),
    }
}

/// Pushes `node` and its descendants into `nodes`, returning the new node's ID.
/// The node's parent link is set, but it is not added to the parent's children.
fn create_node(
    nodes: &mut Vec<NodeData>,
    parent: NodeId,
    node: &NewNode,
    namespaces: &HashMap<String, String>,
) -> NodeId {
    let mut data = match node {
        NewNode::Element {
            name,
            prefix,
            attributes,
            ..
        } => {
            let namespace_uri = prefix
                .as_ref()
                .and_then(|p| namespaces.get(p))
                .map(|u| Arc::from(u.as_str()));
            let mut data = NodeData::element(
                Arc::from(name.as_str()),
                prefix.as_deref().map(Arc::from),
                namespace_uri,
            );
            for (key, value) in attributes {
                data.set_attr(Attr {
                    name: Arc::from(key.as_str()),
                    value: Box::from(value.as_str()),
                    prefix: None,
                    ns_uri: None,
                });
            }
            data
        }
        NewNode::Text(text) => NodeData::text(text.clone()),
        NewNode::CData(text) => NodeData::cdata(text.clone()),
        NewNode::Comment(text) => NodeData::comment(text.clone()),
    };
    data.set_parent(Some(parent));
    let id = nodes.len();
    nodes.push(data);

    if let NewNode::Element { children, .. } = node {
        for child in children {
            let child_id = create_node(nodes, id, child, namespaces);
            nodes[id].push_child(child_id);
        }
    }
    id
}

/// Replaces every occurrence of `old` with `new` in the text and CDATA
/// descendants of `root`, leaving the element structure unchanged.
pub(super) fn replace_text_in_subtree(doc: &XmlDocument, root: NodeId, old: &str, new: &str) {
    if old.is_empty() {
        return;
    }
    let mut nodes = doc.nodes.write();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = &mut nodes[id];
        match node.node_type {
            NodeType::Text | NodeType::CData => {
                if let Some(content) = node.content.as_mut() {
                    if content.contains(old) {
                        *content = content.replace(old, new);
                    }
                }
            }
            NodeType::Element | NodeType::Document => stack.extend(node.child_ids()),
            _ => {}
        }
    }
}
