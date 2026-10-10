//! XPath 1.0 expression grammar: operators, literals, end of input, and
//! names that collide with operator or function keywords (§3.7).

mod common;

use fastxml::xpath::{XPathResult, evaluate};
use fastxml::{Parser, Query, XmlDocument};

const DOC: &str = r#"<root><a>1</a><a>2</a><b>x</b><a>3</a></root>"#;

fn doc(xml: &str) -> XmlDocument {
    Parser::from(xml).parse().unwrap()
}

fn number(d: &XmlDocument, xpath: &str) -> f64 {
    match evaluate(d, xpath) {
        Ok(XPathResult::Number(n)) => n,
        other => panic!("{xpath}: expected a number, got {other:?}"),
    }
}

fn string(d: &XmlDocument, xpath: &str) -> String {
    match evaluate(d, xpath) {
        Ok(XPathResult::String(s)) => s,
        other => panic!("{xpath}: expected a string, got {other:?}"),
    }
}

fn node_count(d: &XmlDocument, xpath: &str) -> usize {
    match evaluate(d, xpath) {
        Ok(XPathResult::Nodes(nodes)) => nodes.len(),
        other => panic!("{xpath}: expected a node-set, got {other:?}"),
    }
}

fn assert_syntax_error(xpath: &str) {
    assert!(
        Query::compile(xpath).is_err(),
        "{xpath:?} should be rejected, got {:?}",
        Query::compile(xpath).map(|q| q.to_string())
    );
}

// --- multiplicative operators at top level ----------------------------------

#[test]
fn multiply_at_top_level() {
    let d = doc(DOC);
    assert_eq!(number(&d, "2*3"), 6.0);
    assert_eq!(number(&d, "2 * 3"), 6.0);
    assert_eq!(number(&d, "2+3*4"), 14.0);
    assert_eq!(number(&d, "7 * 2 div 4"), 3.5);
    assert_eq!(number(&d, "count(//a) * 2"), 6.0);
    compare_with_libxml!(xpath: DOC, "2+3*4", &d);
}

// --- number literals ---------------------------------------------------------

#[test]
fn number_literals_with_leading_or_trailing_dot() {
    let d = doc(DOC);
    assert_eq!(number(&d, ".5 + 1"), 1.5);
    assert_eq!(number(&d, "1. + 1"), 2.0);
    assert_eq!(number(&d, ".5"), 0.5);
    compare_with_libxml!(xpath: DOC, ".5 + 1", &d);
}

// --- string literals ---------------------------------------------------------

#[test]
fn unterminated_string_literal_is_rejected() {
    assert_syntax_error("'abc");
    assert_syntax_error("\"abc");
    assert_syntax_error("//a[@id='x]");
    let err = Query::compile("'abc").unwrap_err().to_string();
    assert!(err.contains("unclosed string"), "{err}");
}

// --- end of input ------------------------------------------------------------

#[test]
fn trailing_tokens_are_rejected() {
    assert_syntax_error("1 2");
    assert_syntax_error("//a b");
    assert_syntax_error("//a]");
    assert_syntax_error("//a)");
    assert_syntax_error("count(//a) count(//a)");
    assert_syntax_error("//a,");
}

#[test]
fn double_slash_needs_a_following_step() {
    assert_syntax_error("//");
    assert_syntax_error("/root//");
    assert_syntax_error("/root/");
    assert_syntax_error("count(//)");
    let d = doc(DOC);
    assert_eq!(number(&d, "count(/)"), 1.0);
    compare_with_libxml!(xpath: DOC, "count(/)", &d);
}

// --- function arguments are full expressions ---------------------------------

#[test]
fn function_arguments_accept_operators_and_unions() {
    let d = doc(DOC);
    assert_eq!(string(&d, "string(1 div 0)"), "Infinity");
    assert_eq!(number(&d, "count(//a | //b)"), 4.0);
    assert_eq!(string(&d, "concat('a', 1 + 2)"), "a3");
    assert_eq!(number(&d, "sum(//a) * 2"), 12.0);
    assert_eq!(number(&d, "floor(7 div 2)"), 3.0);
    compare_with_libxml!(xpath: DOC, "count(//a | //b)", &d);
    compare_with_libxml!(xpath: DOC, "concat('a', 1 + 2)", &d);
}

// --- §3.7: keywords are names outside operator position ----------------------

const KEYWORD_DOC: &str =
    r#"<root><div>4</div><and/><or/><mod/><text>t</text><node/><count/></root>"#;

#[test]
fn operator_keywords_are_element_names_in_name_position() {
    let d = doc(KEYWORD_DOC);
    assert_eq!(node_count(&d, "//div"), 1);
    assert_eq!(node_count(&d, "/root/and"), 1);
    assert_eq!(node_count(&d, "/root/or"), 1);
    assert_eq!(node_count(&d, "/root/mod"), 1);
    assert_eq!(node_count(&d, "//*[self::div]"), 1);
    assert_eq!(node_count(&d, "child::div"), 1);
    assert_eq!(number(&d, "//div div 2"), 2.0);
    assert_eq!(number(&d, "//div mod 3"), 1.0);
    assert_eq!(number(&d, "count(//div)"), 1.0);
    compare_with_libxml!(xpath: KEYWORD_DOC, "//div", &d);
    compare_with_libxml!(xpath: KEYWORD_DOC, "/root/and", &d);
}

#[test]
fn node_type_and_function_names_are_element_names_without_parenthesis() {
    let d = doc(KEYWORD_DOC);
    assert_eq!(node_count(&d, "/root/text"), 1);
    assert_eq!(node_count(&d, "/root/node"), 1);
    assert_eq!(node_count(&d, "/root/count"), 1);
    assert_eq!(node_count(&d, "text"), 1);
    assert_eq!(node_count(&d, "count"), 1);
    assert_eq!(string(&d, "string(/root/text)"), "t");
    compare_with_libxml!(xpath: KEYWORD_DOC, "/root/text", &d);
}

#[test]
fn text_node_test_at_expression_start_selects_child_text_nodes() {
    // `text()` is the node test `child::text()`, so it compares each child
    // text node, not the element's whole string-value.
    const MIXED: &str = r#"<root><a>x<b>y</b></a></root>"#;
    let d = doc(MIXED);
    assert_eq!(node_count(&d, "//a[text()='x']"), 1);
    assert_eq!(node_count(&d, "//a[text()='xy']"), 0);
    compare_with_libxml!(xpath: MIXED, "//a[text()='x']", &d);
    compare_with_libxml!(xpath: MIXED, "//a[text()='xy']", &d);
}

// --- numeric predicates compare with position() -----------------------------

#[test]
fn numeric_predicate_compares_with_position() {
    let d = doc(DOC);
    assert_eq!(node_count(&d, "//a[0]"), 0);
    assert_eq!(node_count(&d, "//a[1.5]"), 0);
    assert_eq!(node_count(&d, "//a[2]"), 1);
    assert_eq!(node_count(&d, "//a[1 + 1]"), 1);
    compare_with_libxml!(xpath: DOC, "//a[1.5]", &d);
    compare_with_libxml!(xpath: DOC, "//a[0]", &d);
}

// --- Display renders what was written ---------------------------------------

#[test]
fn display_renders_the_whole_expression() {
    for (src, rendered) in [
        ("2*3", "2 * 3"),
        ("//a[1.5]", "//a[1.5]"),
        ("count(//a | //b)", "count(//a | //b)"),
        ("string(1 div 0)", "string(1 div 0)"),
        ("//div div 2", "//div div 2"),
        (".5", "0.5"),
    ] {
        let q = Query::compile(src).unwrap();
        assert_eq!(q.to_string(), rendered, "display of {src:?}");
        let again = Query::compile(&q.to_string()).unwrap();
        assert_eq!(again.to_string(), rendered, "re-parse of {rendered:?}");
    }
}
