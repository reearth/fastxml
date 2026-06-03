//! Compile-once, evaluate-many XPath queries.
//!
//! [`Query`] parses an XPath expression a single time and can then be evaluated
//! against many documents, avoiding the re-parse that the free
//! [`evaluate`](crate::compat::evaluate) function performs on every call.

use std::fmt;

use crate::document::XmlDocument;
use crate::error::Result;
use crate::node::XmlNode;

use super::XPathResult;
use super::evaluator::XPathEvaluator;
use super::parser::{Expr, parse_xpath};

/// A compiled XPath expression, reusable across documents.
///
/// `Query::compile(expr)?` parses once; `eval` / `eval_from` / `find_nodes`
/// then run that compiled expression against any [`XmlDocument`]. Namespaces
/// declared on each document's root are picked up automatically; additional
/// bindings can be registered with [`namespace`](Query::namespace).
///
/// # Example
///
/// ```
/// use fastxml::{Parser, Query};
///
/// let query = Query::compile("//item")?;
///
/// let a = Parser::from("<root><item/><item/></root>").parse()?;
/// let b = Parser::from("<root><item/></root>").parse()?;
///
/// assert_eq!(query.find_nodes(&a)?.len(), 2);
/// assert_eq!(query.find_nodes(&b)?.len(), 1);
/// # Ok::<(), fastxml::error::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Query {
    expr: Expr,
    namespaces: Vec<(String, String)>,
}

impl Query {
    /// Compiles an XPath expression, returning an error if it is invalid.
    pub fn compile(xpath: &str) -> Result<Self> {
        Ok(Self {
            expr: parse_xpath(xpath)?,
            namespaces: Vec::new(),
        })
    }

    /// Builds a query from an already-parsed expression (no namespaces).
    ///
    /// Internal: used by conversions such as `From<StreamableQuery>`.
    pub(crate) fn from_expr(expr: Expr) -> Self {
        Self {
            expr,
            namespaces: Vec::new(),
        }
    }

    /// The compiled expression. Internal, for conversions and analysis.
    pub(crate) fn expr(&self) -> &Expr {
        &self.expr
    }

    /// Registers an extra namespace binding used for every evaluation.
    ///
    /// Bindings declared on a document's root element are registered
    /// automatically; use this for prefixes that are not declared in the
    /// document itself.
    pub fn namespace(mut self, prefix: impl Into<String>, uri: impl Into<String>) -> Self {
        self.namespaces.push((prefix.into(), uri.into()));
        self
    }

    fn evaluator<'a>(&self, doc: &'a XmlDocument) -> XPathEvaluator<'a> {
        let mut evaluator = XPathEvaluator::new(doc);
        for (prefix, uri) in &self.namespaces {
            evaluator.register_namespace(prefix, uri);
        }
        evaluator
    }

    /// Evaluates the query against `doc` from its root element.
    pub fn eval(&self, doc: &XmlDocument) -> Result<XPathResult> {
        self.evaluator(doc).evaluate_expr(&self.expr)
    }

    /// Evaluates the query against `doc` relative to `context`.
    pub fn eval_from(&self, doc: &XmlDocument, context: &XmlNode) -> Result<XPathResult> {
        self.evaluator(doc).evaluate_expr_from(&self.expr, context)
    }

    /// Convenience: evaluates and returns the matched nodes.
    pub fn find_nodes(&self, doc: &XmlDocument) -> Result<Vec<XmlNode>> {
        Ok(self.eval(doc)?.into_nodes())
    }
}

/// Renders the compiled expression back to an (equivalent) XPath string.
impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.expr.fmt(f)
    }
}

/// Something usable as an XPath query against a document.
///
/// Implemented for a compiled [`Query`] (evaluated directly, no re-parse) and
/// for `str` / `String` (compiled on the fly). This is what lets [`QueryExt`]
/// accept a string and a compiled query interchangeably.
///
/// A plain `Into<Query>` can't serve this role because compiling a string is
/// fallible, so the conversion is folded into the evaluation here.
pub trait AsQuery {
    /// Evaluates as a query against `doc`, from its root element.
    fn eval_on(&self, doc: &XmlDocument) -> Result<XPathResult>;
}

impl AsQuery for Query {
    fn eval_on(&self, doc: &XmlDocument) -> Result<XPathResult> {
        self.eval(doc)
    }
}

impl AsQuery for str {
    fn eval_on(&self, doc: &XmlDocument) -> Result<XPathResult> {
        Query::compile(self)?.eval(doc)
    }
}

impl AsQuery for String {
    fn eval_on(&self, doc: &XmlDocument) -> Result<XPathResult> {
        self.as_str().eval_on(doc)
    }
}

/// Method-call ergonomics for evaluating XPath directly on a document.
///
/// This extension trait lets you write `doc.query("//item")` instead of
/// `evaluate(&doc, "//item")`, without the `document` module having to depend on
/// `xpath`. The argument is anything that is [`AsQuery`], so a string literal
/// and a pre-compiled [`Query`] work interchangeably:
///
/// ```
/// use fastxml::{Parser, Query, QueryExt};
///
/// let doc = Parser::from("<root><item/><item/></root>").parse()?;
///
/// // String: compiled on the fly.
/// assert_eq!(doc.query_nodes("//item")?.len(), 2);
/// assert_eq!(doc.query("count(//item)")?.to_number(), 2.0);
///
/// // Pre-compiled query: reused without re-parsing.
/// let q = Query::compile("//item")?;
/// assert_eq!(doc.query_nodes(&q)?.len(), 2);
/// # Ok::<(), fastxml::error::Error>(())
/// ```
pub trait QueryExt {
    /// Evaluates `query` (a string or a compiled [`Query`]) against this document.
    fn query<Q: AsQuery + ?Sized>(&self, query: &Q) -> Result<XPathResult>;

    /// Evaluates `query`, returning the matched nodes.
    fn query_nodes<Q: AsQuery + ?Sized>(&self, query: &Q) -> Result<Vec<XmlNode>>;
}

impl QueryExt for XmlDocument {
    fn query<Q: AsQuery + ?Sized>(&self, query: &Q) -> Result<XPathResult> {
        query.eval_on(self)
    }

    fn query_nodes<Q: AsQuery + ?Sized>(&self, query: &Q) -> Result<Vec<XmlNode>> {
        Ok(query.eval_on(self)?.into_nodes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    #[test]
    fn compile_once_eval_many() {
        let query = Query::compile("//item").unwrap();
        let a = Parser::from("<root><item/><item/></root>").parse().unwrap();
        let b = Parser::from("<root><item/></root>").parse().unwrap();
        assert_eq!(query.find_nodes(&a).unwrap().len(), 2);
        assert_eq!(query.find_nodes(&b).unwrap().len(), 1);
    }

    #[test]
    fn invalid_expression_errors_at_compile() {
        assert!(Query::compile("///bad[").is_err());
    }

    #[test]
    fn eval_returns_typed_result() {
        let query = Query::compile("count(//item)").unwrap();
        let doc = Parser::from("<root><item/><item/><item/></root>")
            .parse()
            .unwrap();
        assert_eq!(query.eval(&doc).unwrap().to_number(), 3.0);
    }

    #[test]
    fn eval_from_context_node() {
        let query = Query::compile("item").unwrap();
        let doc = Parser::from("<root><group><item/><item/></group></root>")
            .parse()
            .unwrap();
        let group = Query::compile("//group")
            .unwrap()
            .find_nodes(&doc)
            .unwrap()
            .remove(0);
        assert_eq!(query.eval_from(&doc, &group).unwrap().into_nodes().len(), 2);
    }

    #[test]
    fn query_to_string_roundtrips() {
        let q = Query::compile("//item[@id='2']").unwrap();
        let rendered = q.to_string();
        // Re-compiling the rendered string yields an equivalent query.
        let doc = Parser::from(r#"<root><item id="1"/><item id="2"/></root>"#)
            .parse()
            .unwrap();
        let again = Query::compile(&rendered).unwrap();
        assert_eq!(
            q.find_nodes(&doc).unwrap().len(),
            again.find_nodes(&doc).unwrap().len()
        );
        assert_eq!(rendered, "//item[@id='2']");
    }

    #[test]
    fn query_ext_accepts_str_and_compiled_query() {
        let doc = Parser::from("<root><item/><item/><item/></root>")
            .parse()
            .unwrap();

        // String form (compiled on the fly).
        assert_eq!(doc.query_nodes("//item").unwrap().len(), 3);
        assert_eq!(doc.query("count(//item)").unwrap().to_number(), 3.0);

        // Pre-compiled query, passed by reference (no re-parse).
        let q = Query::compile("//item").unwrap();
        assert_eq!(doc.query_nodes(&q).unwrap().len(), 3);
        assert_eq!(doc.query(&q).unwrap().into_nodes().len(), 3);

        // String form via owned String also works.
        let owned = String::from("//item");
        assert_eq!(doc.query_nodes(&owned).unwrap().len(), 3);
    }

    #[test]
    fn explicit_namespace_binding() {
        let query = Query::compile("//g:point")
            .unwrap()
            .namespace("g", "http://example.com/gml");
        let doc = Parser::from(
            r#"<root xmlns:gml="http://example.com/gml"><gml:point/><gml:point/></root>"#,
        )
        .parse()
        .unwrap();
        assert_eq!(query.find_nodes(&doc).unwrap().len(), 2);
    }
}
