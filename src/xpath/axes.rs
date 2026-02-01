//! XPath axis navigation.
//!
//! This module implements all XPath 1.0 axes for navigating the XML document tree.
//!
//! ## Forward Axes (document order)
//! - `child` - direct children
//! - `descendant` - all descendants
//! - `descendant-or-self` - self and all descendants
//! - `following-sibling` - siblings after this node
//! - `following` - all nodes after this node in document order
//! - `attribute` - attributes of the node
//! - `namespace` - namespace nodes
//!
//! ## Reverse Axes (reverse document order)
//! - `parent` - direct parent
//! - `ancestor` - all ancestors
//! - `ancestor-or-self` - self and all ancestors
//! - `preceding-sibling` - siblings before this node
//! - `preceding` - all nodes before this node in document order
//!
//! ## Self Axis
//! - `self` - the context node itself

use crate::node::XmlNode;

use super::parser::Axis;

/// Selects nodes along the specified axis from the context node.
pub fn select_axis(axis: &Axis, context: &XmlNode) -> Vec<XmlNode> {
    match axis {
        Axis::Child => select_child(context),
        Axis::Descendant => select_descendant(context, false),
        Axis::DescendantOrSelf => select_descendant(context, true),
        Axis::Parent => select_parent(context),
        Axis::Ancestor => select_ancestor(context, false),
        Axis::AncestorOrSelf => select_ancestor(context, true),
        Axis::FollowingSibling => select_following_sibling(context),
        Axis::PrecedingSibling => select_preceding_sibling(context),
        Axis::Following => select_following(context),
        Axis::Preceding => select_preceding(context),
        Axis::SelfNode => vec![context.clone()],
        Axis::Attribute => select_attribute(context),
        Axis::Namespace => select_namespace(context),
    }
}

/// Selects direct children of the context node.
pub fn select_child(node: &XmlNode) -> Vec<XmlNode> {
    node.get_child_nodes()
}

/// Selects descendants of the context node.
///
/// If `include_self` is true, includes the context node itself.
pub fn select_descendant(node: &XmlNode, include_self: bool) -> Vec<XmlNode> {
    let mut result = Vec::new();
    if include_self {
        result.push(node.clone());
    }
    collect_descendants(node, &mut result);
    result
}

/// Recursively collects all descendants of a node.
fn collect_descendants(node: &XmlNode, result: &mut Vec<XmlNode>) {
    for child in node.get_child_nodes() {
        result.push(child.clone());
        collect_descendants(&child, result);
    }
}

/// Selects the parent of the context node.
pub fn select_parent(node: &XmlNode) -> Vec<XmlNode> {
    node.get_parent().into_iter().collect()
}

/// Selects ancestors of the context node.
///
/// If `include_self` is true, includes the context node itself.
/// Returns ancestors in reverse document order (parent first, then grandparent, etc.)
pub fn select_ancestor(node: &XmlNode, include_self: bool) -> Vec<XmlNode> {
    let mut result = Vec::new();
    if include_self {
        result.push(node.clone());
    }

    let mut current = node.get_parent();
    while let Some(parent) = current {
        result.push(parent.clone());
        current = parent.get_parent();
    }
    result
}

/// Selects siblings that follow the context node.
pub fn select_following_sibling(node: &XmlNode) -> Vec<XmlNode> {
    if let Some(parent) = node.get_parent() {
        let children = parent.get_child_nodes();
        let mut found = false;
        children
            .into_iter()
            .filter(|child| {
                if child.id() == node.id() {
                    found = true;
                    false
                } else {
                    found
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Selects siblings that precede the context node.
///
/// Returns in reverse document order (immediately preceding sibling first).
pub fn select_preceding_sibling(node: &XmlNode) -> Vec<XmlNode> {
    if let Some(parent) = node.get_parent() {
        let children = parent.get_child_nodes();
        let mut result = Vec::new();
        for child in children {
            if child.id() == node.id() {
                break;
            }
            result.push(child);
        }
        // Reverse for reverse document order
        result.reverse();
        result
    } else {
        Vec::new()
    }
}

/// Selects all nodes that follow the context node in document order.
///
/// This includes:
/// - Following siblings and their descendants
/// - Following siblings of ancestors and their descendants
///
/// Does NOT include descendants of the context node.
pub fn select_following(node: &XmlNode) -> Vec<XmlNode> {
    let mut result = Vec::new();

    // Get ancestors (including self) from bottom to top
    let mut ancestors = vec![node.clone()];
    let mut current = node.get_parent();
    while let Some(parent) = current {
        ancestors.push(parent.clone());
        current = parent.get_parent();
    }

    // For each ancestor (starting from the context node), add following siblings and their descendants
    for ancestor in &ancestors {
        if let Some(parent) = ancestor.get_parent() {
            let children = parent.get_child_nodes();
            let mut found = false;
            for child in children {
                if child.id() == ancestor.id() {
                    found = true;
                    continue;
                }
                if found {
                    // Add this sibling and all its descendants
                    collect_descendants_including_self(&child, &mut result);
                }
            }
        }
    }

    result
}

/// Selects all nodes that precede the context node in document order.
///
/// This includes:
/// - Preceding siblings and their descendants
/// - Preceding siblings of ancestors and their descendants
///
/// Does NOT include ancestors of the context node.
/// Returns in reverse document order.
pub fn select_preceding(node: &XmlNode) -> Vec<XmlNode> {
    let mut result = Vec::new();

    // Get ancestors (including self) from bottom to top
    let mut ancestors = vec![node.clone()];
    let mut ancestor_ids = std::collections::HashSet::new();
    ancestor_ids.insert(node.id());

    let mut current = node.get_parent();
    while let Some(parent) = current {
        ancestor_ids.insert(parent.id());
        ancestors.push(parent.clone());
        current = parent.get_parent();
    }

    // For each ancestor (starting from the context node), add preceding siblings and their descendants
    for ancestor in &ancestors {
        if let Some(parent) = ancestor.get_parent() {
            let children = parent.get_child_nodes();
            let mut preceding_siblings = Vec::new();

            for child in children {
                if child.id() == ancestor.id() {
                    break;
                }
                preceding_siblings.push(child);
            }

            // Add in reverse order (most recent first for reverse document order)
            for sibling in preceding_siblings.into_iter().rev() {
                collect_descendants_reverse(&sibling, &mut result, &ancestor_ids);
            }
        }
    }

    result
}

/// Collects a node and all its descendants in document order.
fn collect_descendants_including_self(node: &XmlNode, result: &mut Vec<XmlNode>) {
    result.push(node.clone());
    for child in node.get_child_nodes() {
        collect_descendants_including_self(&child, result);
    }
}

/// Collects a node and its descendants in reverse document order.
/// Excludes nodes in the ancestor_ids set.
fn collect_descendants_reverse(
    node: &XmlNode,
    result: &mut Vec<XmlNode>,
    ancestor_ids: &std::collections::HashSet<usize>,
) {
    // First collect children (in reverse)
    let children = node.get_child_nodes();
    for child in children.into_iter().rev() {
        if !ancestor_ids.contains(&child.id()) {
            collect_descendants_reverse(&child, result, ancestor_ids);
        }
    }
    // Then add self
    if !ancestor_ids.contains(&node.id()) {
        result.push(node.clone());
    }
}

/// Selects attribute nodes of the context node.
///
/// Note: This returns pseudo-nodes for attributes. In this implementation,
/// we return an empty vec since attribute access is handled separately.
pub fn select_attribute(_node: &XmlNode) -> Vec<XmlNode> {
    // Attribute nodes are handled specially in node test matching
    // We could implement attribute pseudo-nodes here if needed
    Vec::new()
}

/// Selects namespace nodes in scope for the context node.
///
/// Note: Namespace nodes are a special case in XPath. This implementation
/// returns an empty vec as namespace handling is done through the resolver.
pub fn select_namespace(_node: &XmlNode) -> Vec<XmlNode> {
    // Namespace nodes would need special handling
    // Most XPath implementations don't fully implement this axis
    Vec::new()
}

/// Checks if axis is a forward axis (selects nodes after context in document order).
pub fn is_forward_axis(axis: &Axis) -> bool {
    matches!(
        axis,
        Axis::Child
            | Axis::Descendant
            | Axis::DescendantOrSelf
            | Axis::FollowingSibling
            | Axis::Following
            | Axis::Attribute
            | Axis::Namespace
            | Axis::SelfNode
    )
}

/// Checks if axis is a reverse axis (selects nodes before context in document order).
pub fn is_reverse_axis(axis: &Axis) -> bool {
    matches!(
        axis,
        Axis::Parent
            | Axis::Ancestor
            | Axis::AncestorOrSelf
            | Axis::PrecedingSibling
            | Axis::Preceding
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_select_child() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children = select_child(&root);
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_select_descendant() {
        let doc = parse("<root><a><b><c/></b></a></root>").unwrap();
        let root = doc.get_root_element().unwrap();

        // Without self
        let descendants = select_descendant(&root, false);
        assert_eq!(descendants.len(), 3);

        // With self
        let with_self = select_descendant(&root, true);
        assert_eq!(with_self.len(), 4);
    }

    #[test]
    fn test_select_ancestor() {
        let doc = parse("<root><a><b><c/></b></a></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let b = a.get_child_nodes().into_iter().next().unwrap();
        let c = b.get_child_nodes().into_iter().next().unwrap();

        // Without self - includes document node in XPath
        let ancestors = select_ancestor(&c, false);
        assert_eq!(ancestors.len(), 4); // b, a, root element, document node

        // With self
        let with_self = select_ancestor(&c, true);
        assert_eq!(with_self.len(), 5); // c, b, a, root element, document node
    }

    #[test]
    fn test_select_following_sibling() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children = root.get_child_nodes();
        let b = &children[1];

        let following = select_following_sibling(b);
        assert_eq!(following.len(), 1);
        assert_eq!(following[0].get_name(), "c");
    }

    #[test]
    fn test_select_preceding_sibling() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children = root.get_child_nodes();
        let b = &children[1];

        let preceding = select_preceding_sibling(b);
        assert_eq!(preceding.len(), 1);
        assert_eq!(preceding[0].get_name(), "a");
    }

    #[test]
    fn test_is_forward_axis() {
        assert!(is_forward_axis(&Axis::Child));
        assert!(is_forward_axis(&Axis::Descendant));
        assert!(is_forward_axis(&Axis::Following));
        assert!(!is_forward_axis(&Axis::Parent));
        assert!(!is_forward_axis(&Axis::Preceding));
    }

    #[test]
    fn test_is_reverse_axis() {
        assert!(is_reverse_axis(&Axis::Parent));
        assert!(is_reverse_axis(&Axis::Ancestor));
        assert!(is_reverse_axis(&Axis::Preceding));
        assert!(!is_reverse_axis(&Axis::Child));
        assert!(!is_reverse_axis(&Axis::Following));
    }

    // Additional tests for select_axis
    #[test]
    fn test_select_axis_child() {
        let doc = parse("<root><a/><b/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::Child, &root);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_descendant() {
        let doc = parse("<root><a><b/></a></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::Descendant, &root);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_descendant_or_self() {
        let doc = parse("<root><a><b/></a></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::DescendantOrSelf, &root);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_select_axis_parent() {
        let doc = parse("<root><a/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let result = select_axis(&Axis::Parent, &a);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get_name(), "root");
    }

    #[test]
    fn test_select_axis_ancestor() {
        let doc = parse("<root><a><b/></a></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let b = a.get_child_nodes().into_iter().next().unwrap();
        let result = select_axis(&Axis::Ancestor, &b);
        // a, root, document
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_select_axis_ancestor_or_self() {
        let doc = parse("<root><a/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let result = select_axis(&Axis::AncestorOrSelf, &a);
        // a, root, document
        assert!(result.len() >= 2);
    }

    #[test]
    fn test_select_axis_following_sibling() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let result = select_axis(&Axis::FollowingSibling, &a);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_preceding_sibling() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children: Vec<_> = root.get_child_nodes();
        let c = &children[2];
        let result = select_axis(&Axis::PrecedingSibling, c);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_following() {
        let doc = parse("<root><a/><b><c/></b></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let result = select_axis(&Axis::Following, &a);
        // b and c
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_preceding() {
        let doc = parse("<root><a><x/></a><b/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children: Vec<_> = root.get_child_nodes();
        let b = &children[1];
        let result = select_axis(&Axis::Preceding, b);
        // a and x
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_axis_self() {
        let doc = parse("<root/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::SelfNode, &root);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id(), root.id());
    }

    #[test]
    fn test_select_axis_attribute() {
        let doc = parse("<root attr='value'/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::Attribute, &root);
        // Current implementation returns empty vec
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_axis_namespace() {
        let doc = parse("<root xmlns:ns='http://example.com'/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_axis(&Axis::Namespace, &root);
        // Current implementation returns empty vec
        assert!(result.is_empty());
    }

    // Edge case tests
    #[test]
    fn test_select_parent_no_parent() {
        let doc = parse("<root/>").unwrap();
        // Document node has no parent
        let doc_node = doc.get_node(0).unwrap();
        let result = select_parent(&doc_node);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_following_sibling_no_parent() {
        let doc = parse("<root/>").unwrap();
        let doc_node = doc.get_node(0).unwrap();
        let result = select_following_sibling(&doc_node);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_preceding_sibling_no_parent() {
        let doc = parse("<root/>").unwrap();
        let doc_node = doc.get_node(0).unwrap();
        let result = select_preceding_sibling(&doc_node);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_following_sibling_last_child() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children: Vec<_> = root.get_child_nodes();
        let c = &children[2];
        let result = select_following_sibling(c);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_preceding_sibling_first_child() {
        let doc = parse("<root><a/><b/><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let a = root.get_child_nodes().into_iter().next().unwrap();
        let result = select_preceding_sibling(&a);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_following_complex() {
        let doc = parse("<root><a><a1/></a><b><b1/><b2/></b><c/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children: Vec<_> = root.get_child_nodes();
        let a = &children[0];
        let a1 = a.get_child_nodes().into_iter().next().unwrap();

        let result = select_following(&a1);
        // Should get b, b1, b2, c
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_select_preceding_complex() {
        let doc = parse("<root><a><a1/></a><b><b1/></b></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let children: Vec<_> = root.get_child_nodes();
        let b = &children[1];
        let b1 = b.get_child_nodes().into_iter().next().unwrap();

        let result = select_preceding(&b1);
        // Should get a, a1 (not b which is an ancestor)
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_select_descendant_empty() {
        let doc = parse("<root/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_descendant(&root, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_child_empty() {
        let doc = parse("<root/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_child(&root);
        assert!(result.is_empty());
    }

    #[test]
    fn test_is_forward_axis_all() {
        assert!(is_forward_axis(&Axis::Child));
        assert!(is_forward_axis(&Axis::Descendant));
        assert!(is_forward_axis(&Axis::DescendantOrSelf));
        assert!(is_forward_axis(&Axis::FollowingSibling));
        assert!(is_forward_axis(&Axis::Following));
        assert!(is_forward_axis(&Axis::Attribute));
        assert!(is_forward_axis(&Axis::Namespace));
        assert!(is_forward_axis(&Axis::SelfNode));
    }

    #[test]
    fn test_is_reverse_axis_all() {
        assert!(is_reverse_axis(&Axis::Parent));
        assert!(is_reverse_axis(&Axis::Ancestor));
        assert!(is_reverse_axis(&Axis::AncestorOrSelf));
        assert!(is_reverse_axis(&Axis::PrecedingSibling));
        assert!(is_reverse_axis(&Axis::Preceding));
    }

    #[test]
    fn test_select_attribute() {
        let doc = parse("<root attr='value'/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_attribute(&root);
        // Current implementation returns empty
        assert!(result.is_empty());
    }

    #[test]
    fn test_select_namespace() {
        let doc = parse("<root xmlns:ns='http://example.com'/>").unwrap();
        let root = doc.get_root_element().unwrap();
        let result = select_namespace(&root);
        // Current implementation returns empty
        assert!(result.is_empty());
    }
}
