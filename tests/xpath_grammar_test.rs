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

// =============================================================================
// Full expression grammar: comparisons, and/or, filter expressions, node types
// =============================================================================

fn boolean(d: &XmlDocument, xpath: &str) -> bool {
    match evaluate(d, xpath) {
        Ok(XPathResult::Boolean(b)) => b,
        other => panic!("{xpath}: expected a boolean, got {other:?}"),
    }
}

#[test]
fn comparisons_and_logic_at_top_level() {
    let d = doc(DOC);
    for (xpath, expected) in [
        ("1 = 1", true),
        ("count(//a) = 3", true),
        ("count(//a) != 3", false),
        ("'a' = //nonexistent", false),
        ("true() and false()", false),
        ("true() or false()", true),
        ("1 < 2 = true()", true),
        ("-1 = 1 - 2", true),
        ("1 = 1 and 2 = 2", true),
        ("1 = 2 or 2 = 2 and 3 = 4", false),
        ("not(1 = 2)", true),
        ("//a = 2", true),
        ("//a[1] = 1", true),
        ("boolean(//a = '3')", true),
        ("2 > 1 > 0", true),
    ] {
        assert_eq!(boolean(&d, xpath), expected, "{xpath}");
        compare_with_libxml!(xpath: DOC, xpath, &d);
    }
}

#[test]
fn comparison_inside_function_arguments() {
    let d = doc(DOC);
    assert_eq!(number(&d, "count(//a[. = 1 or . = 3])"), 2.0);
    assert_eq!(string(&d, "string(count(//a) = 3)"), "true");
    assert_eq!(number(&d, "number(1 < 2) + 1"), 2.0);
    compare_with_libxml!(xpath: DOC, "string(count(//a) = 3)", &d);
}

#[test]
fn filter_expressions_take_predicates_and_paths() {
    let d = doc(DOC);
    assert_eq!(node_count(&d, "(//a)[1]"), 1);
    assert_eq!(string(&d, "string((//a)[2])"), "2");
    assert_eq!(string(&d, "string((//a)[last()])"), "3");
    assert_eq!(string(&d, "string((//b | //a)[1])"), "1");
    assert_eq!(number(&d, "count((//a)/text())"), 3.0);
    assert_eq!(number(&d, "count((/root)//a)"), 3.0);
    assert_eq!(number(&d, "count((//a)[1] | //b)"), 2.0);
    assert_eq!(number(&d, "count((//a)[. > 1][1])"), 1.0);
    for xpath in [
        "(//a)[1]",
        "(//a)[2]",
        "(//a)/text()",
        "string((//a)[last()])",
        "string((//b | //a)[1])",
        "count((/root)//a)",
        "count((//a)[1] | //b)",
    ] {
        compare_with_libxml!(xpath: DOC, xpath, &d);
    }
}

#[test]
fn union_operands_must_be_node_sets() {
    let d = doc(DOC);
    let err = evaluate(&d, "1 | //a").unwrap_err().to_string();
    assert!(err.contains("node-set"), "{err}");
    assert!(evaluate(&d, "(1)[1]").is_err());
    assert!(evaluate(&d, "'x'/a").is_err());
}

#[test]
fn predicates_use_the_full_expression_grammar() {
    let d = doc(DOC);
    for (xpath, expected) in [
        ("//a[(. = 1 or . = 2) and . != 2]", 1),
        ("//a[(1 + 1) * 1 = position()]", 1),
        ("//a[. * 2 = 4]", 1),
        ("//a[1 and 2]", 3),
        ("//a[not(1)]", 0),
        ("//a[position() = 1 or position() = last()]", 2),
        ("//a[(//b)]", 3),
        ("//a[. = (//a)[3]]", 1),
    ] {
        assert_eq!(node_count(&d, xpath), expected, "{xpath}");
        compare_with_libxml!(xpath: DOC, xpath, &d);
    }
}

const NODE_TYPE_DOC: &str = r#"<root><!--c1--><?pi data?><?other?><a>1<!--c2--></a></root>"#;

#[test]
fn comment_and_processing_instruction_node_tests() {
    let d = doc(NODE_TYPE_DOC);
    for (xpath, expected) in [
        ("count(//comment())", 2.0),
        ("count(/root/comment())", 1.0),
        ("count(//processing-instruction())", 2.0),
        ("count(//processing-instruction('pi'))", 1.0),
        ("count(//processing-instruction(\"other\"))", 1.0),
        ("count(//processing-instruction('zz'))", 0.0),
        ("count(/root/node())", 4.0),
        ("count(/root/*)", 1.0),
    ] {
        assert_eq!(number(&d, xpath), expected, "{xpath}");
        compare_with_libxml!(xpath: NODE_TYPE_DOC, xpath, &d);
    }
    assert_eq!(string(&d, "string(//a/comment())"), "c2");
    assert_eq!(string(&d, "name(//processing-instruction())"), "pi");
    // An element named `comment` is still reachable by name.
    let d2 = doc("<root><comment>x</comment></root>");
    assert_eq!(node_count(&d2, "/root/comment"), 1);
    assert_eq!(number(&d2, "count(/root/comment())"), 0.0);
}

#[test]
fn display_round_trips_the_new_forms() {
    for (src, rendered) in [
        ("count(//a) = 3", "count(//a) = 3"),
        ("(//a)[1]", "(//a)[1]"),
        ("(//a)/text()", "(//a)/text()"),
        ("(//a)[1]//b", "(//a)[1]//b"),
        ("//comment()", "//comment()"),
        (
            "//processing-instruction('pi')",
            "//processing-instruction('pi')",
        ),
        ("1 = 1 and 2 = 2", "1 = 1 and 2 = 2"),
        // `=` is left-associative, so only the right operand needs parentheses.
        ("(1 = 1) = (2 = 2)", "1 = 1 = (2 = 2)"),
        ("a or b and c", "a or b and c"),
        ("(a or b) and c", "(a or b) and c"),
        ("1 - (2 - 3)", "1 - (2 - 3)"),
        ("-(1 + 2)", "-(1 + 2)"),
        ("(//a | //b)[1]", "(//a | //b)[1]"),
        ("$v[1]/x", "$v[1]/x"),
        (
            "//a[(. = 1 or . = 2) and . != 2]",
            "//a[(.=1 or .=2) and .!=2]",
        ),
    ] {
        let q = Query::compile(src).unwrap();
        assert_eq!(q.to_string(), rendered, "display of {src:?}");
        let again = Query::compile(rendered).unwrap();
        assert_eq!(again.to_string(), rendered, "re-parse of {rendered:?}");
    }
}
