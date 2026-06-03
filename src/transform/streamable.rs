//! Compiled, streamability-validated XPath for [`Transformer`](super::Transformer).
//!
//! A [`StreamableQuery`] is the transform-side counterpart to
//! [`Query`](crate::Query): it parses an XPath once and validates *at compile
//! time* that the expression can be matched in a single streaming pass, so a
//! non-streamable pattern is rejected up front instead of failing mid-run.

use crate::document::XmlDocument;
use crate::error::Result;
use crate::xpath::{AsQuery, Expr, Query, XPathEvaluator, XPathResult, XPathSource, parse_xpath};

use super::error::{TransformError, TransformResult};
use super::xpath_analyze::{XPathAnalysis, analyze_xpath};

/// A pre-compiled, streamable XPath pattern for use with a
/// [`Transformer`](super::Transformer).
///
/// Unlike [`Query`](crate::Query) (which evaluates full XPath 1.0 against a
/// document), a `StreamableQuery` is restricted to the streamable subset and is
/// validated when it is compiled:
///
/// ```
/// use fastxml::transform::{StreamableQuery, Transformer};
///
/// // Compiles: a streamable pattern.
/// let q = StreamableQuery::compile("//item")?;
///
/// // Rejected up front: `last()` needs the whole node set.
/// assert!(StreamableQuery::compile("//item[last()]").is_err());
///
/// let xml = r#"<root><item/><item/></root>"#;
/// let out = Transformer::from(xml)
///     .on(&q, |node| node.set_attribute("seen", "1"))
///     .to_string()?;
/// assert_eq!(out.matches("seen").count(), 2);
/// # Ok::<(), fastxml::transform::TransformError>(())
/// ```
#[derive(Debug, Clone)]
pub struct StreamableQuery {
    expr: Expr,
}

impl StreamableQuery {
    /// Compiles `xpath`, returning an error if it is invalid or not streamable.
    pub fn compile(xpath: &str) -> TransformResult<Self> {
        let expr = parse_xpath(xpath)?;
        match analyze_xpath(&expr) {
            XPathAnalysis::Streamable(_) => Ok(Self { expr }),
            XPathAnalysis::NotStreamable(reason) => Err(TransformError::NotStreamable {
                xpath: xpath.to_string(),
                reason,
            }),
        }
    }
}

// --- Streamable -> Query (a streamable pattern is always a valid query) -------

impl From<StreamableQuery> for Query {
    fn from(streamable: StreamableQuery) -> Self {
        Query::from_expr(streamable.expr)
    }
}

impl From<&StreamableQuery> for Query {
    fn from(streamable: &StreamableQuery) -> Self {
        Query::from_expr(streamable.expr.clone())
    }
}

impl AsQuery for StreamableQuery {
    fn eval_on(&self, doc: &XmlDocument) -> Result<XPathResult> {
        XPathEvaluator::new(doc).evaluate_expr(&self.expr)
    }
}

// --- Query -> Streamable (fallible: must be streamable) ------------------------

impl TryFrom<&Query> for StreamableQuery {
    type Error = TransformError;

    fn try_from(query: &Query) -> TransformResult<Self> {
        let expr = query.expr();
        match analyze_xpath(expr) {
            XPathAnalysis::Streamable(_) => Ok(Self { expr: expr.clone() }),
            XPathAnalysis::NotStreamable(reason) => Err(TransformError::NotStreamable {
                xpath: "<query>".to_string(),
                reason,
            }),
        }
    }
}

impl TryFrom<Query> for StreamableQuery {
    type Error = TransformError;

    fn try_from(query: Query) -> TransformResult<Self> {
        StreamableQuery::try_from(&query)
    }
}

/// A value usable as a transform pattern: a `&str` / `String` (analyzed when the
/// transform runs) or a pre-validated [`StreamableQuery`].
///
/// This is what lets [`Transformer`](super::Transformer)'s `on` / `collect`
/// accept a string and a compiled query interchangeably. A plain `Into` can't
/// serve the role because compiling a string is fallible; that check is folded
/// into [`StreamableQuery::compile`] (eager) or the transform run (for strings).
pub trait IntoStreamable {
    /// Converts into the internal XPath source stored by the transform engine.
    fn into_xpath_source(self) -> XPathSource;
}

impl IntoStreamable for &str {
    fn into_xpath_source(self) -> XPathSource {
        XPathSource::String(self.to_string())
    }
}

impl IntoStreamable for String {
    fn into_xpath_source(self) -> XPathSource {
        XPathSource::String(self)
    }
}

impl IntoStreamable for &String {
    fn into_xpath_source(self) -> XPathSource {
        XPathSource::String(self.clone())
    }
}

impl IntoStreamable for StreamableQuery {
    fn into_xpath_source(self) -> XPathSource {
        XPathSource::Ast(self.expr)
    }
}

impl IntoStreamable for &StreamableQuery {
    fn into_xpath_source(self) -> XPathSource {
        XPathSource::Ast(self.expr.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::Transformer;

    #[test]
    fn compile_accepts_streamable() {
        assert!(StreamableQuery::compile("//item").is_ok());
        assert!(StreamableQuery::compile("//item[@id='2']").is_ok());
    }

    #[test]
    fn compile_rejects_non_streamable() {
        assert!(StreamableQuery::compile("//item[last()]").is_err());
        assert!(StreamableQuery::compile("//item/parent::*").is_err());
    }

    #[test]
    fn compile_rejects_invalid_syntax() {
        assert!(StreamableQuery::compile("///bad[").is_err());
    }

    #[test]
    fn transformer_accepts_str_and_compiled() {
        let xml = r#"<root><item/><item/></root>"#;

        // String form.
        let a = Transformer::from(xml)
            .on("//item", |n| n.set_attribute("a", "1"))
            .to_string()
            .unwrap();
        assert_eq!(a.matches(r#"a="1""#).count(), 2);

        // Pre-compiled form, by reference.
        let q = StreamableQuery::compile("//item").unwrap();
        let b = Transformer::from(xml)
            .on(&q, |n| n.set_attribute("b", "1"))
            .to_string()
            .unwrap();
        assert_eq!(b.matches(r#"b="1""#).count(), 2);
    }

    #[test]
    fn streamable_to_query_and_eval() {
        use crate::{Parser, Query, QueryExt};

        let sq = StreamableQuery::compile("//item").unwrap();
        let doc = Parser::from("<root><item/><item/><item/></root>")
            .parse()
            .unwrap();

        // Via From<StreamableQuery> for Query.
        let q: Query = (&sq).into();
        assert_eq!(q.find_nodes(&doc).unwrap().len(), 3);

        // Via AsQuery: a streamable query works directly with doc.query(..).
        assert_eq!(doc.query_nodes(&sq).unwrap().len(), 3);
    }

    #[test]
    fn query_to_streamable_is_fallible() {
        use crate::Query;

        // Streamable query converts.
        let ok = Query::compile("//item").unwrap();
        assert!(StreamableQuery::try_from(&ok).is_ok());

        // Non-streamable query is rejected.
        let bad = Query::compile("//item[last()]").unwrap();
        assert!(StreamableQuery::try_from(&bad).is_err());
    }

    #[test]
    fn transformer_collect_accepts_compiled() {
        let xml = r#"<root><item id="1"/><item id="2"/></root>"#;
        let q = StreamableQuery::compile("//item").unwrap();
        let ids: Vec<String> = Transformer::from(xml)
            .collect(&q, |n| n.get_attribute("id").unwrap_or_default())
            .unwrap();
        assert_eq!(ids, vec!["1", "2"]);
    }
}
