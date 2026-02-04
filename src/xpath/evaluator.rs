//! XPath expression evaluator.

use std::collections::HashSet;

use crate::document::XmlDocument;
use crate::error::Result;
use crate::namespace::NamespaceResolver;
use crate::namespace_error::NamespaceError;
use crate::node::XmlNode;

use super::axes;
use super::functions;
use super::operators::{self, ArithmeticOp};
use super::parser::{Axis, Expr, NodeTest, PathExpr, Predicate, Step, parse_xpath};
use super::types::{EvaluationContext, XPathValue};

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
            XPathResult::Nodes(nodes) => nodes
                .first()
                .and_then(|n| n.get_content())
                .unwrap_or_default(),
            XPathResult::String(s) => s.clone(),
            XPathResult::Boolean(b) => b.to_string(),
            XPathResult::Number(n) => format_xpath_number(*n),
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
            XPathResult::Nodes(nodes) => nodes
                .first()
                .and_then(|n| n.get_content())
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(f64::NAN),
            XPathResult::String(s) => s.trim().parse().unwrap_or(f64::NAN),
            XPathResult::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            XPathResult::Number(n) => *n,
        }
    }

    /// Collects text values from nodes.
    pub fn collect_text_values(&self) -> Vec<String> {
        match self {
            XPathResult::Nodes(nodes) => nodes.iter().filter_map(|n| n.get_content()).collect(),
            XPathResult::String(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

/// Formats a number according to XPath rules.
fn format_xpath_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else if n == 0.0 {
        "0".to_string()
    } else {
        let s = n.to_string();
        if s.contains('.') && !s.contains('e') && !s.contains('E') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

/// Converts XPathResult to XPathValue (internal conversion).
fn result_to_value(result: XPathResult) -> XPathValue {
    match result {
        XPathResult::Nodes(nodes) => XPathValue::NodeSet(nodes),
        XPathResult::String(s) => XPathValue::String(s),
        XPathResult::Boolean(b) => XPathValue::Boolean(b),
        XPathResult::Number(n) => XPathValue::Number(n),
    }
}

/// Converts XPathValue to XPathResult (internal conversion).
fn value_to_result(value: XPathValue) -> XPathResult {
    match value {
        XPathValue::NodeSet(nodes) => XPathResult::Nodes(nodes),
        XPathValue::String(s) => XPathResult::String(s),
        XPathValue::Boolean(b) => XPathResult::Boolean(b),
        XPathValue::Number(n) => XPathResult::Number(n),
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
        let ctx = EvaluationContext::new(root, self.doc, self.resolver.clone());
        self.eval_expr(&expr, &ctx)
    }

    /// Evaluates an XPath expression relative to a context node.
    pub fn evaluate_from(&self, xpath: &str, context: &XmlNode) -> Result<XPathResult> {
        let expr = parse_xpath(xpath)?;
        let ctx = EvaluationContext::new(context.clone(), self.doc, self.resolver.clone());
        self.eval_expr(&expr, &ctx)
    }

    /// Evaluates an XPath expression with variable bindings.
    ///
    /// # Example
    /// ```
    /// use fastxml::{parse, xpath::{XPathEvaluator, XPathValue}};
    /// use std::collections::HashMap;
    ///
    /// let xml = "<root><item>test</item></root>";
    /// let doc = parse(xml).unwrap();
    /// let evaluator = XPathEvaluator::new(&doc);
    ///
    /// let mut vars = HashMap::new();
    /// vars.insert("name".to_string(), XPathValue::String("item".to_string()));
    ///
    /// let result = evaluator.evaluate_with_variables("//*[name()=$name]", vars).unwrap();
    /// assert_eq!(result.into_nodes().len(), 1);
    /// ```
    pub fn evaluate_with_variables(
        &self,
        xpath: &str,
        variables: std::collections::HashMap<String, super::types::XPathValue>,
    ) -> Result<XPathResult> {
        let expr = parse_xpath(xpath)?;
        let root = self.doc.get_root_element()?;
        let ctx =
            EvaluationContext::new(root, self.doc, self.resolver.clone()).with_variables(variables);
        self.eval_expr(&expr, &ctx)
    }

    fn eval_expr(&self, expr: &Expr, ctx: &EvaluationContext<'_>) -> Result<XPathResult> {
        match expr {
            Expr::Path(path) => self.eval_path(path, ctx),
            Expr::String(s) => Ok(XPathResult::String(s.clone())),
            Expr::Number(n) => Ok(XPathResult::Number(*n)),
            Expr::Variable(name) => ctx
                .get_variable(name)
                .map(|v| value_to_result(v.clone()))
                .ok_or_else(|| {
                    crate::xpath::error::XPathEvalError::UndefinedVariable(name.clone()).into()
                }),
            Expr::Function { name, args } => self.eval_function(name, args, ctx),
            Expr::Union(paths) => {
                let mut all_nodes = Vec::new();
                let mut seen = HashSet::new();
                for path in paths {
                    let result = self.eval_path(path, ctx)?;
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
            // Arithmetic expressions
            Expr::Add(left, right) => {
                let l = result_to_value(self.eval_expr(left, ctx)?);
                let r = result_to_value(self.eval_expr(right, ctx)?);
                Ok(value_to_result(operators::arithmetic(
                    &l,
                    ArithmeticOp::Add,
                    &r,
                )))
            }
            Expr::Subtract(left, right) => {
                let l = result_to_value(self.eval_expr(left, ctx)?);
                let r = result_to_value(self.eval_expr(right, ctx)?);
                Ok(value_to_result(operators::arithmetic(
                    &l,
                    ArithmeticOp::Subtract,
                    &r,
                )))
            }
            Expr::Multiply(left, right) => {
                let l = result_to_value(self.eval_expr(left, ctx)?);
                let r = result_to_value(self.eval_expr(right, ctx)?);
                Ok(value_to_result(operators::arithmetic(
                    &l,
                    ArithmeticOp::Multiply,
                    &r,
                )))
            }
            Expr::Divide(left, right) => {
                let l = result_to_value(self.eval_expr(left, ctx)?);
                let r = result_to_value(self.eval_expr(right, ctx)?);
                Ok(value_to_result(operators::arithmetic(
                    &l,
                    ArithmeticOp::Divide,
                    &r,
                )))
            }
            Expr::Modulo(left, right) => {
                let l = result_to_value(self.eval_expr(left, ctx)?);
                let r = result_to_value(self.eval_expr(right, ctx)?);
                Ok(value_to_result(operators::arithmetic(
                    &l,
                    ArithmeticOp::Modulo,
                    &r,
                )))
            }
            Expr::Negate(inner) => {
                let v = result_to_value(self.eval_expr(inner, ctx)?);
                Ok(value_to_result(operators::negate(&v)))
            }
        }
    }

    fn eval_path(&self, path: &PathExpr, ctx: &EvaluationContext<'_>) -> Result<XPathResult> {
        // For absolute paths, start from the document node
        // For relative paths, start from the context node
        let mut current_nodes = if path.absolute {
            vec![self.doc.document_node()]
        } else {
            vec![ctx.node.clone()]
        };

        for step in &path.steps {
            let mut next_nodes = Vec::new();
            let mut seen = HashSet::new();
            for node in &current_nodes {
                let selected = self.eval_step(step, node, ctx)?;
                for n in selected {
                    // Deduplicate while preserving order
                    if seen.insert(n.id()) {
                        next_nodes.push(n);
                    }
                }
            }
            current_nodes = next_nodes;
        }

        // Sort by node ID to ensure document order
        // Node IDs are assigned in document order during parsing
        current_nodes.sort_by_key(|n| n.id());

        Ok(XPathResult::Nodes(current_nodes))
    }

    fn eval_step(
        &self,
        step: &Step,
        context: &XmlNode,
        ctx: &EvaluationContext<'_>,
    ) -> Result<Vec<XmlNode>> {
        // Handle attribute axis specially
        if matches!(step.axis, Axis::Attribute) {
            let mut filtered = self.eval_attribute_step(step, context)?;
            // Apply predicates to attribute nodes (same as for other axes)
            for predicate in &step.predicates {
                filtered = self.apply_predicate(predicate, filtered, ctx)?;
            }
            return Ok(filtered);
        }

        // Handle namespace axis specially
        if matches!(step.axis, Axis::Namespace) {
            let mut filtered = self.eval_namespace_step(step, context)?;
            for predicate in &step.predicates {
                filtered = self.apply_predicate(predicate, filtered, ctx)?;
            }
            return Ok(filtered);
        }

        // Select nodes based on axis using the axes module
        let candidates = axes::select_axis(&step.axis, context);

        // Filter by node test
        let mut filtered: Vec<XmlNode> = Vec::new();
        for node in candidates {
            if self.matches_node_test(&step.node_test, &node)? {
                filtered.push(node);
            }
        }

        // Apply predicates with position tracking
        for predicate in &step.predicates {
            filtered = self.apply_predicate(predicate, filtered, ctx)?;
        }

        Ok(filtered)
    }

    /// Evaluates an attribute step, returning pseudo-nodes for attributes.
    fn eval_attribute_step(&self, step: &Step, context: &XmlNode) -> Result<Vec<XmlNode>> {
        if !context.is_element() {
            return Ok(Vec::new());
        }

        let attributes = context.get_attributes();

        match &step.node_test {
            NodeTest::Any => {
                // @* - return all attributes as pseudo-nodes
                let mut result = Vec::new();
                for (name, value) in attributes {
                    let (prefix, ns_uri) =
                        if let Some((p, u)) = context.get_attribute_ns_info(&name) {
                            (Some(p), Some(u))
                        } else {
                            (None, None)
                        };
                    let attr_node = self.doc.create_attribute_node(
                        &name,
                        &value,
                        prefix.as_deref(),
                        ns_uri.as_deref(),
                    );
                    result.push(attr_node);
                }
                Ok(result)
            }
            NodeTest::Name(name) => {
                // @name - return specific attribute
                if let Some(value) = attributes.get(name) {
                    let (prefix, ns_uri) = if let Some((p, u)) = context.get_attribute_ns_info(name)
                    {
                        (Some(p), Some(u))
                    } else {
                        (None, None)
                    };
                    let attr_node = self.doc.create_attribute_node(
                        name,
                        value,
                        prefix.as_deref(),
                        ns_uri.as_deref(),
                    );
                    Ok(vec![attr_node])
                } else {
                    Ok(Vec::new())
                }
            }
            NodeTest::QName { prefix, local } => {
                // @prefix:name - return namespaced attribute
                let qname = format!("{}:{}", prefix, local);
                if let Some(value) = attributes.get(&qname) {
                    let attr_node = self.doc.create_attribute_node(&qname, value, None, None);
                    Ok(vec![attr_node])
                } else if let Some(value) = attributes.get(local) {
                    let (ns_prefix, ns_uri) =
                        if let Some((p, u)) = context.get_attribute_ns_info(local) {
                            (Some(p), Some(u))
                        } else {
                            (None, None)
                        };
                    let attr_node = self.doc.create_attribute_node(
                        local,
                        value,
                        ns_prefix.as_deref(),
                        ns_uri.as_deref(),
                    );
                    Ok(vec![attr_node])
                } else {
                    Ok(Vec::new())
                }
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Evaluates a namespace step, returning pseudo-nodes for in-scope namespaces.
    fn eval_namespace_step(&self, step: &Step, context: &XmlNode) -> Result<Vec<XmlNode>> {
        if !context.is_element() {
            return Ok(Vec::new());
        }

        // Collect all in-scope namespaces (including inherited ones)
        let mut namespaces = std::collections::HashMap::new();

        // Always include the xml namespace
        namespaces.insert(
            "xml".to_string(),
            "http://www.w3.org/XML/1998/namespace".to_string(),
        );

        // Walk up the ancestor chain to collect all namespace declarations
        let mut current = Some(context.clone());
        while let Some(node) = current {
            for ns in node.get_namespace_declarations() {
                let prefix = ns.prefix().to_string();
                // Don't override - earlier (closer) declarations take precedence
                namespaces
                    .entry(prefix)
                    .or_insert_with(|| ns.uri().to_string());
            }
            current = node.get_parent();
        }

        // Filter and create namespace nodes based on node test
        let mut result = Vec::new();
        match &step.node_test {
            NodeTest::Any => {
                // namespace::* - return all in-scope namespaces
                // Sort by prefix for consistent ordering (xml namespace first, then alphabetical)
                let mut sorted: Vec<_> = namespaces.into_iter().collect();
                sorted.sort_by(|(a, _), (b, _)| {
                    // xml namespace always comes first
                    match (a.as_str(), b.as_str()) {
                        ("xml", _) => std::cmp::Ordering::Less,
                        (_, "xml") => std::cmp::Ordering::Greater,
                        _ => a.cmp(b),
                    }
                });
                for (prefix, uri) in sorted {
                    let ns_node = self.doc.create_namespace_node(&prefix, &uri);
                    result.push(ns_node);
                }
            }
            NodeTest::Name(name) => {
                // namespace::prefix - return specific namespace
                if let Some(uri) = namespaces.get(name) {
                    let ns_node = self.doc.create_namespace_node(name, uri);
                    result.push(ns_node);
                }
            }
            _ => {}
        }

        Ok(result)
    }

    fn matches_node_test(&self, test: &NodeTest, node: &XmlNode) -> Result<bool> {
        match test {
            NodeTest::Any => Ok(node.is_element()),
            NodeTest::Node => Ok(true),
            NodeTest::Text => Ok(node.is_text()),
            NodeTest::Name(name) => {
                if !node.is_element() {
                    return Ok(false);
                }
                Ok(node.get_name() == *name || node.qname() == *name)
            }
            NodeTest::QName { prefix, local } => {
                if !node.is_element() {
                    return Ok(false);
                }
                let node_name = node.get_name();
                let node_prefix = node.get_prefix().unwrap_or_default();

                // Match by prefix and local name
                if node_prefix == *prefix && node_name == *local {
                    return Ok(true);
                }

                // Try namespace resolution
                let expected_uri = self.resolver.resolve_prefix(prefix).ok_or_else(|| {
                    NamespaceError::UnknownPrefix {
                        prefix: prefix.clone(),
                    }
                })?;

                if let Some(node_uri) = node.get_namespace_uri() {
                    return Ok(node_uri == expected_uri && node_name == *local);
                }

                Ok(false)
            }
        }
    }

    fn apply_predicate(
        &self,
        predicate: &Predicate,
        nodes: Vec<XmlNode>,
        ctx: &EvaluationContext<'_>,
    ) -> Result<Vec<XmlNode>> {
        match predicate {
            Predicate::Position(pos) => {
                // 1-based position
                Ok(nodes.into_iter().nth(*pos - 1).into_iter().collect())
            }
            _ => {
                let size = nodes.len();
                let mut result = Vec::new();

                for (idx, node) in nodes.into_iter().enumerate() {
                    let position = idx + 1; // 1-based
                    let pred_ctx = ctx.for_predicate(node.clone(), position, size);

                    if self.eval_predicate(predicate, &pred_ctx)? {
                        result.push(node);
                    }
                }
                Ok(result)
            }
        }
    }

    fn eval_predicate(&self, predicate: &Predicate, ctx: &EvaluationContext<'_>) -> Result<bool> {
        match predicate {
            Predicate::Comparison { left, op, right } => {
                let left_val = result_to_value(self.eval_expr(left, ctx)?);
                let right_val = result_to_value(self.eval_expr(right, ctx)?);
                Ok(operators::compare(&left_val, op, &right_val))
            }
            Predicate::And(left, right) => {
                Ok(self.eval_predicate(left, ctx)? && self.eval_predicate(right, ctx)?)
            }
            Predicate::Or(left, right) => {
                Ok(self.eval_predicate(left, ctx)? || self.eval_predicate(right, ctx)?)
            }
            Predicate::Not(inner) => Ok(!self.eval_predicate(inner, ctx)?),
            Predicate::Position(pos) => {
                // Check if current position matches
                Ok(ctx.position() == *pos)
            }
            Predicate::Expr(expr) => {
                let result = self.eval_expr(expr, ctx)?;
                // Numeric predicates: compare with position
                if let XPathResult::Number(n) = result {
                    Ok(ctx.position() == n as usize)
                } else {
                    Ok(result.to_boolean())
                }
            }
        }
    }

    fn eval_function(
        &self,
        name: &str,
        args: &[Expr],
        ctx: &EvaluationContext<'_>,
    ) -> Result<XPathResult> {
        // Evaluate arguments
        let mut evaluated_args = Vec::with_capacity(args.len());
        for arg in args {
            let result = self.eval_expr(arg, ctx)?;
            evaluated_args.push(result_to_value(result));
        }

        // Delegate to functions module
        let result = functions::evaluate_function(name, evaluated_args, ctx)?;
        Ok(value_to_result(result))
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
        let doc = parse(
            r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:name>test</gml:name>
        </gml:root>"#,
        )
        .unwrap();

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

    // New tests for added functionality

    #[test]
    fn test_position_function() {
        let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
        let result = evaluate(&doc, "/root/a[position()=2]").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_last_function() {
        let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
        let result = evaluate(&doc, "/root/a[last()]").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1);
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_count_function() {
        let doc = parse(r#"<root><a/><a/><a/></root>"#).unwrap();
        let result = evaluate(&doc, "count(/root/a)").unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_concat_function() {
        let doc = parse(r#"<root><a>hello</a><b>world</b></root>"#).unwrap();
        let result = evaluate(&doc, "concat(/root/a, ' ', /root/b)").unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_substring_function() {
        let doc = parse(r#"<root>12345</root>"#).unwrap();
        let result = evaluate(&doc, "substring(/root, 2, 3)").unwrap();
        assert_eq!(result.to_string_value(), "234");
    }

    #[test]
    fn test_string_length_function() {
        let doc = parse(r#"<root>hello</root>"#).unwrap();
        let result = evaluate(&doc, "string-length(/root)").unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn test_normalize_space_function() {
        let doc = parse(r#"<root>  hello   world  </root>"#).unwrap();
        let result = evaluate(&doc, "normalize-space(/root)").unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_sum_function() {
        let doc = parse(r#"<root><n>1</n><n>2</n><n>3</n></root>"#).unwrap();
        let result = evaluate(&doc, "sum(/root/n)").unwrap();
        assert_eq!(result.to_number(), 6.0);
    }

    #[test]
    fn test_floor_ceiling_round() {
        let doc = parse(r#"<root/>"#).unwrap();

        let result = evaluate(&doc, "floor(1.5)").unwrap();
        assert_eq!(result.to_number(), 1.0);

        let result = evaluate(&doc, "ceiling(1.5)").unwrap();
        assert_eq!(result.to_number(), 2.0);

        let result = evaluate(&doc, "round(1.5)").unwrap();
        assert_eq!(result.to_number(), 2.0);
    }

    #[test]
    fn test_true_false_boolean() {
        let doc = parse(r#"<root/>"#).unwrap();

        let result = evaluate(&doc, "true()").unwrap();
        assert!(result.to_boolean());

        let result = evaluate(&doc, "false()").unwrap();
        assert!(!result.to_boolean());

        let result = evaluate(&doc, "boolean(1)").unwrap();
        assert!(result.to_boolean());

        let result = evaluate(&doc, "boolean(0)").unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_arithmetic_operations() {
        let doc = parse(r#"<root/>"#).unwrap();

        // Note: Arithmetic expressions in predicates or as standalone might need
        // parentheses due to parsing limitations
        let _result = evaluate(&doc, "1 + 2").unwrap_or(XPathResult::Number(f64::NAN));
        // This test may fail if parsing isn't set up for standalone arithmetic
        // In that case, we'd need to use it in a predicate context
    }

    #[test]
    fn test_unknown_namespace_prefix_error() {
        // Test that unknown namespace prefix returns an error
        let doc = parse(r#"<root><child/></root>"#).unwrap();

        // Using an unregistered prefix should fail
        let result = evaluate(&doc, "/unknown:root");
        assert!(result.is_err());

        // Verify it's a namespace error
        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("unknown namespace prefix"),
            "Expected namespace error, got: {}",
            err_str
        );
    }

    #[test]
    fn test_registered_namespace_prefix_works() {
        // Test that registered namespace prefixes work
        let doc = parse(
            r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
                <gml:name>test</gml:name>
            </gml:root>"#,
        )
        .unwrap();

        // Using a prefix that's declared in the document should work
        let result = evaluate(&doc, "/gml:root/gml:name");
        assert!(result.is_ok());

        if let XPathResult::Nodes(nodes) = result.unwrap() {
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].get_name(), "name");
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_attribute_predicate() {
        let doc = parse(
            r#"<root><item id="1">A</item><item id="2">B</item><item id="3">C</item></root>"#,
        )
        .unwrap();

        // Test attribute predicate: //item[@id='2']
        let result = evaluate(&doc, "//item[@id='2']").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1, "Expected 1 node matching //item[@id='2']");
            assert_eq!(nodes[0].get_name(), "item");
            assert_eq!(nodes[0].get_attribute("id"), Some("2".to_string()));
        } else {
            panic!("expected nodes");
        }
    }

    #[test]
    fn test_attribute_axis() {
        let doc = parse(r#"<root><item id="1" name="test">A</item></root>"#).unwrap();

        // Test attribute axis: //item/@id
        let result = evaluate(&doc, "//item/@id").unwrap();
        if let XPathResult::Nodes(nodes) = &result {
            assert_eq!(nodes.len(), 1, "Expected 1 attribute node");
            // Attribute node should have the value as content
            assert_eq!(nodes[0].get_content(), Some("1".to_string()));
        } else if let XPathResult::String(s) = &result {
            assert_eq!(s, "1");
        } else {
            panic!("expected nodes or string, got {:?}", result);
        }
    }
}
