//! Context information available during streaming transformation.

use std::collections::HashMap;

/// Context information available during streaming transformation.
///
/// Provides access to ancestor elements, position, and depth information
/// while processing matched elements in a streaming fashion.
#[derive(Debug, Clone)]
pub struct TransformContext {
    /// Ancestor chain from root to parent (not including current element)
    ancestors: Vec<AncestorInfo>,
    /// Current element's position among siblings with same name (1-indexed)
    position: usize,
    /// Current depth in the document (1 = root element)
    depth: usize,
}

/// Information about an ancestor element.
#[derive(Debug, Clone)]
pub struct AncestorInfo {
    /// Local name
    pub name: String,
    /// Namespace prefix
    pub prefix: Option<String>,
    /// Qualified name (prefix:name or just name)
    pub qname: String,
    /// Attributes (name -> value)
    pub attributes: HashMap<String, String>,
    /// Position among siblings with same name (1-indexed)
    pub position: usize,
    /// Depth in document (1 = root)
    pub depth: usize,
}

impl TransformContext {
    /// Creates a new transform context.
    pub(crate) fn new(ancestors: Vec<AncestorInfo>, position: usize, depth: usize) -> Self {
        Self {
            ancestors,
            position,
            depth,
        }
    }

    /// Returns the parent element info, if any.
    ///
    /// # Example
    ///
    /// ```ignore
    /// StreamTransformer::new(xml)
    ///     .on_with_context("//item", |node, ctx| {
    ///         if let Some(parent) = ctx.parent() {
    ///             println!("Parent: {}", parent.qname);
    ///         }
    ///     })
    ///     .run()?;
    /// ```
    pub fn parent(&self) -> Option<&AncestorInfo> {
        self.ancestors.last()
    }

    /// Returns all ancestors from root to parent.
    ///
    /// The first element is the root, the last is the immediate parent.
    pub fn ancestors(&self) -> &[AncestorInfo] {
        &self.ancestors
    }

    /// Returns the ancestor at given depth (1 = root).
    ///
    /// Returns `None` if the depth is out of range.
    pub fn ancestor_at_depth(&self, depth: usize) -> Option<&AncestorInfo> {
        if depth == 0 || depth > self.ancestors.len() {
            None
        } else {
            self.ancestors.get(depth - 1)
        }
    }

    /// Returns the current element's position among siblings with same name (1-indexed).
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns the current depth in the document (1 = root element).
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Generates a path-based ID (e.g., "root/items[2]/item[3]").
    ///
    /// Position indices are only included when position > 1.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // For the third <item> under the second <items> under <root>:
    /// assert_eq!(ctx.path_id(), "root/items[2]/item[3]");
    /// ```
    pub fn path_id(&self) -> String {
        if self.ancestors.is_empty() {
            return String::new();
        }

        self.ancestors
            .iter()
            .map(|a| {
                if a.position > 1 {
                    format!("{}[{}]", a.qname, a.position)
                } else {
                    a.qname.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Returns an attribute value from the parent element.
    ///
    /// This is a convenience method equivalent to `ctx.parent()?.attributes.get(name)`.
    pub fn parent_attribute(&self, name: &str) -> Option<&String> {
        self.parent().and_then(|p| p.attributes.get(name))
    }
}

impl AncestorInfo {
    /// Creates a new ancestor info.
    pub(crate) fn new(
        name: String,
        prefix: Option<String>,
        attributes: HashMap<String, String>,
        position: usize,
        depth: usize,
    ) -> Self {
        let qname = match &prefix {
            Some(p) if !p.is_empty() => format!("{}:{}", p, name),
            _ => name.clone(),
        };
        Self {
            name,
            prefix,
            qname,
            attributes,
            position,
            depth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> TransformContext {
        let ancestors = vec![
            AncestorInfo::new("root".to_string(), None, HashMap::new(), 1, 1),
            AncestorInfo::new(
                "items".to_string(),
                None,
                HashMap::from([("id".to_string(), "list1".to_string())]),
                2,
                2,
            ),
        ];
        TransformContext::new(ancestors, 3, 3)
    }

    #[test]
    fn test_parent() {
        let ctx = create_test_context();
        let parent = ctx.parent().unwrap();
        assert_eq!(parent.name, "items");
        assert_eq!(parent.position, 2);
    }

    #[test]
    fn test_ancestors() {
        let ctx = create_test_context();
        assert_eq!(ctx.ancestors().len(), 2);
        assert_eq!(ctx.ancestors()[0].name, "root");
        assert_eq!(ctx.ancestors()[1].name, "items");
    }

    #[test]
    fn test_ancestor_at_depth() {
        let ctx = create_test_context();
        assert_eq!(ctx.ancestor_at_depth(1).unwrap().name, "root");
        assert_eq!(ctx.ancestor_at_depth(2).unwrap().name, "items");
        assert!(ctx.ancestor_at_depth(0).is_none());
        assert!(ctx.ancestor_at_depth(3).is_none());
    }

    #[test]
    fn test_position_and_depth() {
        let ctx = create_test_context();
        assert_eq!(ctx.position(), 3);
        assert_eq!(ctx.depth(), 3);
    }

    #[test]
    fn test_path_id() {
        let ctx = create_test_context();
        assert_eq!(ctx.path_id(), "root/items[2]");
    }

    #[test]
    fn test_path_id_with_prefix() {
        let ancestors = vec![AncestorInfo::new(
            "root".to_string(),
            Some("gml".to_string()),
            HashMap::new(),
            1,
            1,
        )];
        let ctx = TransformContext::new(ancestors, 1, 2);
        assert_eq!(ctx.path_id(), "gml:root");
    }

    #[test]
    fn test_parent_attribute() {
        let ctx = create_test_context();
        assert_eq!(ctx.parent_attribute("id"), Some(&"list1".to_string()));
        assert_eq!(ctx.parent_attribute("nonexistent"), None);
    }

    #[test]
    fn test_empty_context() {
        let ctx = TransformContext::new(vec![], 1, 1);
        assert!(ctx.parent().is_none());
        assert!(ctx.ancestors().is_empty());
        assert_eq!(ctx.path_id(), "");
    }
}
