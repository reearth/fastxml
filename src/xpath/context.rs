//! XPath context for evaluation.

use parking_lot::RwLock;

use crate::document::XmlDocument;
use crate::error::Result;
use crate::namespace::NamespaceResolver;
use crate::node::{XmlNode, XmlRoNode};

use super::evaluator::{XPathEvaluator, XPathResult};

/// XPath evaluation context.
///
/// Holds namespace bindings and can be reused for multiple evaluations.
pub struct XmlContext {
    doc: XmlDocument,
    resolver: NamespaceResolver,
}

impl XmlContext {
    /// Creates a context for the given document.
    pub fn new(doc: &XmlDocument) -> Self {
        let resolver = doc.namespace_resolver().read().clone();
        Self {
            doc: doc.clone(),
            resolver,
        }
    }

    /// Registers a namespace binding.
    pub fn register_namespace(&mut self, prefix: &str, uri: &str) -> Result<()> {
        self.resolver.register(prefix, uri);
        Ok(())
    }

    /// Evaluates an XPath expression.
    pub fn evaluate(&self, xpath: &str) -> Result<XPathResult> {
        let evaluator = XPathEvaluator::with_resolver(&self.doc, self.resolver.clone());
        evaluator.evaluate(xpath)
    }

    /// Evaluates an XPath expression relative to a context node.
    pub fn evaluate_from(&self, xpath: &str, node: &XmlNode) -> Result<XPathResult> {
        let evaluator = XPathEvaluator::with_resolver(&self.doc, self.resolver.clone());
        evaluator.evaluate_from(xpath, node)
    }

    /// Finds nodes by XPath expression.
    pub fn find_nodes(&self, xpath: &str) -> Result<Vec<XmlNode>> {
        let result = self.evaluate(xpath)?;
        Ok(result.into_nodes())
    }

    /// Finds nodes by XPath expression relative to a context node.
    pub fn find_nodes_from(&self, xpath: &str, node: &XmlNode) -> Result<Vec<XmlNode>> {
        let result = self.evaluate_from(xpath, node)?;
        Ok(result.into_nodes())
    }
}

impl Clone for XmlContext {
    fn clone(&self) -> Self {
        Self {
            doc: self.doc.clone(),
            resolver: self.resolver.clone(),
        }
    }
}

/// Thread-safe XPath evaluation context.
///
/// Wraps an `XmlContext` with a read-write lock for safe concurrent access.
pub struct XmlSafeContext {
    inner: RwLock<XmlContext>,
}

impl XmlSafeContext {
    /// Creates a thread-safe context for the given document.
    pub fn new(doc: &XmlDocument) -> Self {
        Self {
            inner: RwLock::new(XmlContext::new(doc)),
        }
    }

    /// Registers a namespace binding.
    pub fn register_namespace(&self, prefix: &str, uri: &str) -> Result<()> {
        let mut ctx = self.inner.write();
        ctx.register_namespace(prefix, uri)
    }

    /// Evaluates an XPath expression.
    pub fn evaluate(&self, xpath: &str) -> Result<XPathResult> {
        let ctx = self.inner.read();
        ctx.evaluate(xpath)
    }

    /// Evaluates an XPath expression relative to a context node.
    pub fn evaluate_from(&self, xpath: &str, node: &XmlNode) -> Result<XPathResult> {
        let ctx = self.inner.read();
        ctx.evaluate_from(xpath, node)
    }

    /// Finds nodes by XPath expression.
    pub fn find_nodes(&self, xpath: &str) -> Result<Vec<XmlNode>> {
        let ctx = self.inner.read();
        ctx.find_nodes(xpath)
    }

    /// Finds nodes by XPath expression relative to a context node.
    pub fn find_nodes_from(&self, xpath: &str, node: &XmlNode) -> Result<Vec<XmlNode>> {
        let ctx = self.inner.read();
        ctx.find_nodes_from(xpath, node)
    }
}


/// Creates an XPath context for a document.
pub fn create_context(doc: &XmlDocument) -> Result<XmlContext> {
    Ok(XmlContext::new(doc))
}

/// Creates a thread-safe XPath context for a document.
pub fn create_safe_context(doc: &XmlDocument) -> Result<XmlSafeContext> {
    Ok(XmlSafeContext::new(doc))
}

/// Finds nodes by XPath expression.
pub fn find_nodes_by_xpath(ctx: &XmlContext, xpath: &str, node: &XmlNode) -> Result<Vec<XmlNode>> {
    ctx.find_nodes_from(xpath, node)
}

/// Finds read-only nodes by XPath expression.
pub fn find_readonly_nodes_by_xpath(
    ctx: &XmlContext,
    xpath: &str,
    node: &XmlRoNode,
) -> Result<Vec<XmlRoNode>> {
    let result = ctx.find_nodes_from(xpath, &node.clone().into_node())?;
    Ok(result.into_iter().map(XmlRoNode::from_node).collect())
}

/// Finds read-only nodes by XPath expression using a thread-safe context.
pub fn find_safe_readonly_nodes_by_xpath(
    ctx: &XmlSafeContext,
    xpath: &str,
    node: &XmlRoNode,
) -> Result<Vec<XmlRoNode>> {
    let result = ctx.find_nodes_from(xpath, &node.clone().into_node())?;
    Ok(result.into_iter().map(XmlRoNode::from_node).collect())
}

/// Finds read-only nodes matching any of the specified element names.
///
/// Generates an XPath expression like `//*[(name()='A' or name()='B')]`.
pub fn find_readonly_nodes_in_elements(
    ctx: &XmlContext,
    node: &XmlRoNode,
    elements_to_match: &[&str],
) -> Result<Vec<XmlRoNode>> {
    if elements_to_match.is_empty() {
        return Ok(Vec::new());
    }

    let conditions: Vec<String> = elements_to_match
        .iter()
        .map(|name| format!("name()='{}'", name))
        .collect();

    let xpath = if conditions.len() == 1 {
        format!("//*[{}]", conditions[0])
    } else {
        format!("//*[({})]", conditions.join(" or "))
    };

    find_readonly_nodes_by_xpath(ctx, &xpath, node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_context() {
        let doc = parse(r#"<root><child>text</child></root>"#).unwrap();
        let ctx = create_context(&doc).unwrap();

        let nodes = ctx.find_nodes("/root/child").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "child");
    }

    #[test]
    fn test_safe_context() {
        let doc = parse(r#"<root><child>text</child></root>"#).unwrap();
        let ctx = create_safe_context(&doc).unwrap();

        let nodes = ctx.find_nodes("/root/child").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_name(), "child");
    }

    #[test]
    fn test_find_nodes_in_elements() {
        let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
        let ctx = create_context(&doc).unwrap();
        let root = doc.get_root_element_ro().unwrap();

        let nodes = find_readonly_nodes_in_elements(&ctx, &root, &["Building", "Room"]).unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_namespace_registration() {
        let doc = parse(
            r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#,
        )
        .unwrap();

        let mut ctx = create_context(&doc).unwrap();
        ctx.register_namespace("gml", "http://www.opengis.net/gml")
            .unwrap();

        let nodes = ctx.find_nodes("/gml:root/gml:name").unwrap();
        assert_eq!(nodes.len(), 1);
    }
}
