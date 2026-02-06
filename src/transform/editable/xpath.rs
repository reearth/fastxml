//! XPath evaluation API for EditableNode.

use crate::namespace::NamespaceResolver;
use crate::xpath::evaluator::{XPathEvaluator, XPathResult};

use super::super::error::TransformResult;
use super::EditableNode;
use super::reference::EditableNodeRef;

impl EditableNode {
    /// Evaluates an XPath expression against this node's subtree.
    ///
    /// This uses the internal `XmlDocument` directly, avoiding the need to
    /// serialize to XML string and re-parse for XPath evaluation.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("root", None, None, vec![], vec![], vec![]);
    /// builder.start_element("child", None, None, vec![("id", "1")], vec![], vec![]);
    /// builder.text("Hello");
    /// builder.end_element();
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let result = node.evaluate_xpath("//child[@id='1']").unwrap();
    /// assert_eq!(result.into_nodes().len(), 1);
    /// ```
    pub fn evaluate_xpath(&self, xpath: &str) -> TransformResult<XPathResult> {
        let resolver = self.build_namespace_resolver();
        let evaluator = XPathEvaluator::with_resolver(&self.doc, resolver);
        let root = self.root_node();
        Ok(evaluator.evaluate_from(xpath, &root)?)
    }

    /// Finds all nodes matching an XPath expression and returns them as `EditableNodeRef`s.
    ///
    /// This is a convenience wrapper around `evaluate_xpath()` that filters
    /// to element nodes only.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("root", None, None, vec![], vec![], vec![]);
    /// builder.start_element("item", None, None, vec![("id", "1")], vec![], vec![]);
    /// builder.text("A");
    /// builder.end_element();
    /// builder.start_element("item", None, None, vec![("id", "2")], vec![], vec![]);
    /// builder.text("B");
    /// builder.end_element();
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let items = node.find_by_xpath("//item").unwrap();
    /// assert_eq!(items.len(), 2);
    /// assert_eq!(items[0].get_attribute("id"), Some("1".to_string()));
    /// ```
    pub fn find_by_xpath(&self, xpath: &str) -> TransformResult<Vec<EditableNodeRef<'_>>> {
        let result = self.evaluate_xpath(xpath)?;
        Ok(result
            .into_nodes()
            .into_iter()
            .map(|node| EditableNodeRef {
                node,
                doc: &self.doc,
            })
            .collect())
    }

    /// Gets an attribute value by namespace URI and local name.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// # use fastxml::namespace::Namespace;
    /// let mut builder = EditableNodeBuilder::new();
    /// let ns = Namespace::new("gml", "http://www.opengis.net/gml");
    /// builder.start_element(
    ///     "Feature", None, None,
    ///     vec![("id", "f1")],
    ///     vec![("id", "gml", "http://www.opengis.net/gml")],
    ///     vec![ns],
    /// );
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// assert_eq!(
    ///     node.get_attribute_ns("http://www.opengis.net/gml", "id"),
    ///     Some("f1".to_string()),
    /// );
    /// ```
    pub fn get_attribute_ns(&self, namespace_uri: &str, local_name: &str) -> Option<String> {
        let root = self.root_node();
        if let Some((_prefix, uri)) = root.get_attribute_ns_info(local_name) {
            if uri == namespace_uri {
                return root.get_attribute(local_name);
            }
        }
        None
    }

    /// Builds a namespace resolver that combines the document's own namespaces
    /// with the externally registered namespaces (from StreamTransformer).
    pub(crate) fn build_namespace_resolver(&self) -> NamespaceResolver {
        let mut resolver = self.doc.namespace_resolver().read().clone();
        for (prefix, uri) in &self.namespaces {
            resolver.register(prefix, uri);
        }
        resolver
    }
}
