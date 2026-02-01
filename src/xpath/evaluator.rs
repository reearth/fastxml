//! XPath expression evaluator.

use std::collections::HashSet;

use crate::document::XmlDocument;
use crate::error::{Error, Result};
use crate::namespace::NamespaceResolver;
use crate::node::XmlNode;

use super::parser::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step, parse_xpath};

/// Result of XPath evaluation.
#[derive(Debug, Clone)]
pub enum XPathResult {
    /// Node set result
    Nodes(Vec<XmlNode>),
    /// String result
    String(String),
    /// Boolean result
    Boolean(bool),
    /// Number result
    Number(f64),
}

impl XPathResult {
    /// Returns nodes if this is a node set result.
    pub fn into_nodes(self) -> Vec<XmlNode> {
        match self {
            XPathResult::Nodes(nodes) => nodes,
            _ => Vec::new(),
        }
    }

    /// Converts to string.
    pub fn to_string_value(&self) -> String {
        match self {
            XPathResult::Nodes(nodes) => {
                nodes.first()
                    .and_then(|n| n.get_content())
                    .unwrap_or_default()
            }
            XPathResult::String(s) => s.clone(),
            XPathResult::Boolean(b) => b.to_string(),
            XPathResult::Number(n) => n.to_string(),
        }
    }

    /// Converts to boolean.
    pub fn to_boolean(&self) -> bool {
        match self {
            XPathResult::Nodes(nodes) => !nodes.is_empty(),
            XPathResult::String(s) => !s.is_empty(),
            XPathResult::Boolean(b) => *b,
            XPathResult::Number(n) => *n != 0.0 && !n.is_nan(),
        }
    }

    /// Converts to number.
    pub fn to_number(&self) -> f64 {
        match self {
            XPathResult::Nodes(nodes) => {
                nodes.first()
                    .and_then(|n| n.get_content())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(f64::NAN)
            }
            XPathResult::String(s) => s.parse().unwrap_or(f64::NAN),
            XPathResult::Boolean(b) => if *b { 1.0 } else { 0.0 },
            XPathResult::Number(n) => *n,
        }
    }

    /// Collects text values from nodes.
    pub fn collect_text_values(&self) -> Vec<String> {
        match self {
            XPathResult::Nodes(nodes) => {
                nodes.iter()
                    .filter_map(|n| n.get_content())
                    .collect()
            }
            XPathResult::String(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

/// XPath evaluator.
pub struct XPathEvaluator<'a> {
    doc: &'a XmlDocument,
    resolver: NamespaceResolver,
}

impl<'a> XPathEvaluator<'a> {
    /// Creates a new evaluator for the given document.
    pub fn new(doc: &'a XmlDocument) -> Self {
        let resolver = doc.namespace_resolver().read().clone();
        Self { doc, resolver }
    }

    /// Creates an evaluator with a custom namespace resolver.
    pub fn with_resolver(doc: &'a XmlDocument, resolver: NamespaceResolver) -> Self {
        Self { doc, resolver }
    }

    /// Registers a namespace binding.
    pub fn register_namespace(&mut self, prefix: &str, uri: &str) {
        self.resolver.register(prefix, uri);
    }

    /// Evaluates an XPath expression.
    pub fn evaluate(&self, xpath: &str) -> Result<XPathResult> {
        let expr = parse_xpath(xpath)?;
        let root = self.doc.get_root_element()?;
        self.eval_expr(&expr, &root)
    }

    /// Evaluates an XPath expression relative to a context node.
    pub fn evaluate_from(&self, xpath: &str, context: &XmlNode) -> Result<XPathResult> {
        let expr = parse_xpath(xpath)?;
        self.eval_expr(&expr, context)
    }

    fn eval_expr(&self, expr: &Expr, context: &XmlNode) -> Result<XPathResult> {
        match expr {
            Expr::Path(path) => self.eval_path(path, context),
            Expr::String(s) => Ok(XPathResult::String(s.clone())),
            Expr::Number(n) => Ok(XPathResult::Number(*n)),
            Expr::Function { name, args } => self.eval_function(name, args, context),
            Expr::Union(paths) => {
                let mut all_nodes = Vec::new();
                let mut seen = HashSet::new();
                for path in paths {
                    let result = self.eval_path(path, context)?;
                    if let XPathResult::Nodes(nodes) = result {
                        for node in nodes {
                            if seen.insert(node.id()) {
                                all_nodes.push(node);
                            }
                        }
                    }
                }
                Ok(XPathResult::Nodes(all_nodes))
            }
        }
    }

    fn eval_path(&self, path: &PathExpr, context: &XmlNode) -> Result<XPathResult> {
        // For absolute paths, start from the document node
        // For relative paths, start from the context node
        let mut current_nodes = if path.absolute {
            // Start with document node (id 0)
            vec![self.doc.document_node()]
        } else {
            vec![context.clone()]
        };

        for step in &path.steps {
            let mut next_nodes = Vec::new();
            for node in &current_nodes {
                let selected = self.eval_step(step, node)?;
                next_nodes.extend(selected);
            }
            current_nodes = next_nodes;
        }

        Ok(XPathResult::Nodes(current_nodes))
    }

    fn eval_step(&self, step: &Step, context: &XmlNode) -> Result<Vec<XmlNode>> {
        // Select nodes based on axis
        let candidates = self.select_axis(&step.axis, context);

        // Filter by node test
        let mut filtered: Vec<XmlNode> = candidates
            .into_iter()
            .filter(|node| self.matches_node_test(&step.node_test, node))
            .collect();

        // Apply predicates
        for predicate in &step.predicates {
            filtered = self.apply_predicate(predicate, filtered)?;
        }

        Ok(filtered)
    }

    fn select_axis(&self, axis: &Axis, context: &XmlNode) -> Vec<XmlNode> {
        match axis {
            Axis::Child => context.get_child_nodes(),
            Axis::Descendant => self.get_descendants(context, false),
            Axis::DescendantOrSelf => self.get_descendants(context, true),
            Axis::Parent => context.get_parent().into_iter().collect(),
            Axis::SelfNode => vec![context.clone()],
            Axis::Ancestor => self.get_ancestors(context),
            Axis::FollowingSibling => self.get_following_siblings(context),
            Axis::PrecedingSibling => self.get_preceding_siblings(context),
            Axis::Attribute => Vec::new(), // Attributes handled specially
        }
    }

    fn get_descendants(&self, node: &XmlNode, include_self: bool) -> Vec<XmlNode> {
        let mut result = Vec::new();
        if include_self {
            result.push(node.clone());
        }
        self.collect_descendants(node, &mut result);
        result
    }

    fn collect_descendants(&self, node: &XmlNode, result: &mut Vec<XmlNode>) {
        for child in node.get_child_nodes() {
            result.push(child.clone());
            self.collect_descendants(&child, result);
        }
    }

    fn get_ancestors(&self, node: &XmlNode) -> Vec<XmlNode> {
        let mut result = Vec::new();
        let mut current = node.get_parent();
        while let Some(parent) = current {
            result.push(parent.clone());
            current = parent.get_parent();
        }
        result
    }

    fn get_following_siblings(&self, node: &XmlNode) -> Vec<XmlNode> {
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

    fn get_preceding_siblings(&self, node: &XmlNode) -> Vec<XmlNode> {
        if let Some(parent) = node.get_parent() {
            let children = parent.get_child_nodes();
            let mut result = Vec::new();
            for child in children {
                if child.id() == node.id() {
                    break;
                }
                result.push(child);
            }
            result
        } else {
            Vec::new()
        }
    }

    fn matches_node_test(&self, test: &NodeTest, node: &XmlNode) -> bool {
        match test {
            NodeTest::Any => node.is_element(),
            NodeTest::Node => true,
            NodeTest::Text => node.is_text(),
            NodeTest::Name(name) => {
                if !node.is_element() {
                    return false;
                }
                node.get_name() == *name || node.qname() == *name
            }
            NodeTest::QName { prefix, local } => {
                if !node.is_element() {
                    return false;
                }
                let node_name = node.get_name();
                let node_prefix = node.get_prefix().unwrap_or_default();

                // Match by prefix and local name
                if node_prefix == *prefix && node_name == *local {
                    return true;
                }

                // Try namespace resolution
                if let Some(expected_uri) = self.resolver.resolve_prefix(prefix)
                    && let Some(node_uri) = node.get_namespace_uri()
                {
                    return node_uri == expected_uri && node_name == *local;
                }

                false
            }
        }
    }

    fn apply_predicate(&self, predicate: &Predicate, nodes: Vec<XmlNode>) -> Result<Vec<XmlNode>> {
        match predicate {
            Predicate::Position(pos) => {
                Ok(nodes.into_iter().nth(*pos - 1).into_iter().collect())
            }
            _ => {
                let mut result = Vec::new();
                for node in nodes {
                    if self.eval_predicate(predicate, &node)? {
                        result.push(node);
                    }
                }
                Ok(result)
            }
        }
    }

    fn eval_predicate(&self, predicate: &Predicate, context: &XmlNode) -> Result<bool> {
        match predicate {
            Predicate::Comparison { left, op, right } => {
                let left_val = self.eval_expr(left, context)?;
                let right_val = self.eval_expr(right, context)?;
                Ok(self.compare(&left_val, op, &right_val))
            }
            Predicate::And(left, right) => {
                Ok(self.eval_predicate(left, context)? && self.eval_predicate(right, context)?)
            }
            Predicate::Or(left, right) => {
                Ok(self.eval_predicate(left, context)? || self.eval_predicate(right, context)?)
            }
            Predicate::Not(inner) => {
                Ok(!self.eval_predicate(inner, context)?)
            }
            Predicate::Position(pos) => {
                // Position predicates are handled in apply_predicate
                Ok(*pos == 1)
            }
            Predicate::Expr(expr) => {
                let result = self.eval_expr(expr, context)?;
                Ok(result.to_boolean())
            }
        }
    }

    fn compare(&self, left: &XPathResult, op: &ComparisonOp, right: &XPathResult) -> bool {
        // String comparison for now
        let left_str = left.to_string_value();
        let right_str = right.to_string_value();

        match op {
            ComparisonOp::Equal => left_str == right_str,
            ComparisonOp::NotEqual => left_str != right_str,
            ComparisonOp::LessThan => {
                if let (Ok(l), Ok(r)) = (left_str.parse::<f64>(), right_str.parse::<f64>()) {
                    l < r
                } else {
                    left_str < right_str
                }
            }
            ComparisonOp::LessOrEqual => {
                if let (Ok(l), Ok(r)) = (left_str.parse::<f64>(), right_str.parse::<f64>()) {
                    l <= r
                } else {
                    left_str <= right_str
                }
            }
            ComparisonOp::GreaterThan => {
                if let (Ok(l), Ok(r)) = (left_str.parse::<f64>(), right_str.parse::<f64>()) {
                    l > r
                } else {
                    left_str > right_str
                }
            }
            ComparisonOp::GreaterOrEqual => {
                if let (Ok(l), Ok(r)) = (left_str.parse::<f64>(), right_str.parse::<f64>()) {
                    l >= r
                } else {
                    left_str >= right_str
                }
            }
        }
    }

    fn eval_function(&self, name: &str, args: &[Expr], context: &XmlNode) -> Result<XPathResult> {
        match name {
            "name" => {
                let node = if args.is_empty() {
                    context.clone()
                } else {
                    let result = self.eval_expr(&args[0], context)?;
                    if let XPathResult::Nodes(nodes) = result {
                        nodes.into_iter().next().unwrap_or_else(|| context.clone())
                    } else {
                        context.clone()
                    }
                };
                Ok(XPathResult::String(node.qname()))
            }
            "local-name" => {
                let node = if args.is_empty() {
                    context.clone()
                } else {
                    let result = self.eval_expr(&args[0], context)?;
                    if let XPathResult::Nodes(nodes) = result {
                        nodes.into_iter().next().unwrap_or_else(|| context.clone())
                    } else {
                        context.clone()
                    }
                };
                Ok(XPathResult::String(node.get_name()))
            }
            "namespace-uri" => {
                let node = if args.is_empty() {
                    context.clone()
                } else {
                    let result = self.eval_expr(&args[0], context)?;
                    if let XPathResult::Nodes(nodes) = result {
                        nodes.into_iter().next().unwrap_or_else(|| context.clone())
                    } else {
                        context.clone()
                    }
                };
                let uri = node.get_namespace_uri().unwrap_or_default();
                Ok(XPathResult::String(uri))
            }
            "text" => {
                // text() is handled as a node test, not a function
                // But if called as function, return text content
                let content = context.get_content().unwrap_or_default();
                Ok(XPathResult::String(content))
            }
            "contains" => {
                if args.len() != 2 {
                    return Err(Error::XPathEval("contains() requires 2 arguments".into()));
                }
                let haystack = self.eval_expr(&args[0], context)?.to_string_value();
                let needle = self.eval_expr(&args[1], context)?.to_string_value();
                Ok(XPathResult::Boolean(haystack.contains(&needle)))
            }
            "starts-with" => {
                if args.len() != 2 {
                    return Err(Error::XPathEval("starts-with() requires 2 arguments".into()));
                }
                let string = self.eval_expr(&args[0], context)?.to_string_value();
                let prefix = self.eval_expr(&args[1], context)?.to_string_value();
                Ok(XPathResult::Boolean(string.starts_with(&prefix)))
            }
            _ => Err(Error::XPathEval(format!("unknown function: {}", name))),
        }
    }
}

/// Evaluates an XPath expression against a document.
///
/// # Example
/// ```
/// use fastxml::{parse, evaluate};
///
/// let xml = r#"<root><child>text</child></root>"#;
/// let doc = parse(xml).unwrap();
/// let result = evaluate(&doc, "/root/child/text()").unwrap();
/// assert_eq!(result.to_string_value(), "text");
/// ```
pub fn evaluate(doc: &XmlDocument, xpath: &str) -> Result<XPathResult> {
    let evaluator = XPathEvaluator::new(doc);
    evaluator.evaluate(xpath)
}

/// Collects text values from an XPath result.
pub fn collect_text_values(result: &XPathResult) -> Vec<String> {
    result.collect_text_values()
}

/// Collects a single text value from an XPath result.
pub fn collect_text_value(result: &XPathResult) -> String {
    result.to_string_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_simple_path() {
        let doc = parse(r#"<root><child>hello</child></root>"#).unwrap();
        let result = evaluate(&doc, "/root/child").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].get_name(), "child");
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_descendant() {
        let doc = parse(r#"<root><a><b>text</b></a></root>"#).unwrap();
        let result = evaluate(&doc, "//b").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].get_name(), "b");
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_name_predicate() {
        let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
        let result = evaluate(&doc, "//*[name()='Building']").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].get_name(), "Building");
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_or_predicate() {
        let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
        let result = evaluate(&doc, "//*[(name()='Building' or name()='Room')]").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 2);
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_not_predicate() {
        let doc = parse(r#"<root><Building/><Room/><Window/></root>"#).unwrap();
        let result = evaluate(&doc, "/root/*[not(name()='Window')]").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 2);
            assert!(nodes.iter().all(|n| n.get_name() != "Window"));
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_text() {
        let doc = parse(r#"<root><child>hello</child></root>"#).unwrap();
        let result = evaluate(&doc, "/root/child/text()").unwrap();
        assert_eq!(result.to_string_value(), "hello");
    }

    #[test]
    fn test_namespaced_xpath() {
        let doc = parse(r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#).unwrap();

        let result = evaluate(&doc, "/gml:root/gml:name").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].get_name(), "name");
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_child_axis() {
        let doc = parse(r#"<root><a/><b/></root>"#).unwrap();
        let result = evaluate(&doc, "/root/child::*").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 2);
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_collect_text_values() {
        let doc = parse(r#"<root><a>one</a><a>two</a></root>"#).unwrap();
        let result = evaluate(&doc, "/root/a").unwrap();
        let texts = collect_text_values(&result);
        assert_eq!(texts, vec!["one", "two"]);
    }
}
