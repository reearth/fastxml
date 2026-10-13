//! Unparsing: turn an XPath [`Expr`] AST back into an XPath 1.0 string.
//!
//! Provides `Display` for [`Expr`] and [`PathExpr`]. The output is a normalized
//! but *equivalent* expression: spacing and redundant parentheses may differ
//! from the original source, and it re-parses to the same AST with one
//! exception. XPath 1.0 string literals have no escapes, so a string that
//! contains both `'` and `"` is rendered as a `concat()` call, which evaluates
//! to the same string but re-parses as an [`Expr::Function`]. This backs
//! `Query::to_string()` / `StreamableQuery::to_string()` and is used in error
//! messages.

use std::fmt;

use super::parser::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step};

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render_expr(self))
    }
}

impl fmt::Display for PathExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render_path(self))
    }
}

// Precedence levels of the XPath 1.0 grammar, loosest first.
const OR: u8 = 1;
const AND: u8 = 2;
const EQUALITY: u8 = 3;
const RELATIONAL: u8 = 4;
const ADDITIVE: u8 = 5;
const MULTIPLICATIVE: u8 = 6;
const UNARY: u8 = 7;
const UNION: u8 = 8;
const PATH: u8 = 9;
const PRIMARY: u8 = 10;

fn precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Or(..) => OR,
        Expr::And(..) => AND,
        Expr::Comparison { op, .. } => comparison_precedence(*op),
        Expr::Add(..) | Expr::Subtract(..) => ADDITIVE,
        Expr::Multiply(..) | Expr::Divide(..) | Expr::Modulo(..) => MULTIPLICATIVE,
        Expr::Negate(..) => UNARY,
        Expr::Union(..) => UNION,
        Expr::Path(..) | Expr::Filter { .. } | Expr::PathFrom { .. } => PATH,
        Expr::String(..) | Expr::Number(..) | Expr::Variable(..) | Expr::Function { .. } => PRIMARY,
    }
}

fn comparison_precedence(op: ComparisonOp) -> u8 {
    match op {
        ComparisonOp::Equal | ComparisonOp::NotEqual => EQUALITY,
        _ => RELATIONAL,
    }
}

fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Path(path) => render_path(path),
        Expr::String(s) => fmt_string_literal(s),
        Expr::Number(n) => n.to_string(),
        Expr::Variable(name) => format!("${name}"),
        Expr::Function { name, args } => {
            let args = args.iter().map(render_expr).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
        Expr::Union(operands) => operands
            .iter()
            .enumerate()
            .map(|(i, e)| operand(e, UNION, i == 0))
            .collect::<Vec<_>>()
            .join(" | "),
        Expr::Or(l, r) => binary(l, "or", r, OR),
        Expr::And(l, r) => binary(l, "and", r, AND),
        Expr::Comparison { left, op, right } => {
            binary(left, comparison_op(*op), right, comparison_precedence(*op))
        }
        Expr::Add(l, r) => binary(l, "+", r, ADDITIVE),
        Expr::Subtract(l, r) => binary(l, "-", r, ADDITIVE),
        Expr::Multiply(l, r) => binary(l, "*", r, MULTIPLICATIVE),
        Expr::Divide(l, r) => binary(l, "div", r, MULTIPLICATIVE),
        Expr::Modulo(l, r) => binary(l, "mod", r, MULTIPLICATIVE),
        Expr::Negate(e) => format!("-{}", operand(e, UNARY, false)),
        Expr::Filter { expr, predicates } => {
            let mut out = filter_base(expr);
            for predicate in predicates {
                out.push('[');
                out.push_str(&render_predicate(predicate));
                out.push(']');
            }
            out
        }
        Expr::PathFrom { base, steps } => {
            // The steps render like an absolute path: `/b` or `//b`.
            let rest = render_path(&PathExpr {
                absolute: true,
                steps: steps.clone(),
            });
            format!("{}{}", filter_base(base), rest)
        }
    }
}

/// Renders a left-associative binary operation at precedence `level`.
fn binary(left: &Expr, op: &str, right: &Expr, level: u8) -> String {
    format!(
        "{} {op} {}",
        operand(left, level, true),
        operand(right, level + 1, false)
    )
}

/// Renders an operand, parenthesized when it binds looser than `min_level`.
///
/// A bare root path `/` on the left is parenthesized as well: in `/ * 2` or
/// `/ div 2` the parser must read `*` / `div` as a name test (§3.7).
fn operand(expr: &Expr, min_level: u8, is_left: bool) -> String {
    let bare_root = matches!(expr, Expr::Path(p) if p.absolute && p.steps.is_empty());
    if precedence(expr) < min_level || (is_left && bare_root) {
        format!("({})", render_expr(expr))
    } else {
        render_expr(expr)
    }
}

/// Renders the start of a filter expression or of a path continuing from one:
/// a primary expression or a filter as is, anything else in parentheses.
fn filter_base(expr: &Expr) -> String {
    if precedence(expr) == PRIMARY || matches!(expr, Expr::Filter { .. }) {
        render_expr(expr)
    } else {
        format!("({})", render_expr(expr))
    }
}

fn is_descendant_marker(step: &Step) -> bool {
    step.axis == Axis::DescendantOrSelf
        && step.node_test == NodeTest::Node
        && step.predicates.is_empty()
}

fn render_path(path: &PathExpr) -> String {
    let steps = &path.steps;
    let mut out = String::new();

    // Leading `/` for an absolute path, unless the first step is the `//` marker
    // (`/descendant-or-self::node()/`), which emits its own leading slashes.
    if path.absolute && !steps.first().map(is_descendant_marker).unwrap_or(false) {
        out.push('/');
    }

    let mut i = 0;
    let mut first = true;
    while i < steps.len() {
        let step = &steps[i];
        if is_descendant_marker(step) {
            out.push_str("//");
            i += 1;
            if i < steps.len() {
                out.push_str(&render_step(&steps[i]));
                i += 1;
            }
        } else {
            if !first {
                out.push('/');
            }
            out.push_str(&render_step(step));
            i += 1;
        }
        first = false;
    }

    out
}

fn render_step(step: &Step) -> String {
    // `.` and `..` abbreviate exactly these predicate-less steps.
    if step.node_test == NodeTest::Node && step.predicates.is_empty() {
        match step.axis {
            Axis::SelfNode => return ".".to_string(),
            Axis::Parent => return "..".to_string(),
            _ => {}
        }
    }
    let mut out = String::new();
    match step.axis {
        Axis::Child => {}
        Axis::Attribute => out.push('@'),
        other => {
            out.push_str(axis_name(other));
            out.push_str("::");
        }
    }
    out.push_str(&render_node_test(&step.node_test));
    for predicate in &step.predicates {
        out.push('[');
        out.push_str(&render_predicate(predicate));
        out.push(']');
    }
    out
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Child => "child",
        Axis::Descendant => "descendant",
        Axis::Parent => "parent",
        Axis::SelfNode => "self",
        Axis::DescendantOrSelf => "descendant-or-self",
        Axis::Ancestor => "ancestor",
        Axis::AncestorOrSelf => "ancestor-or-self",
        Axis::FollowingSibling => "following-sibling",
        Axis::PrecedingSibling => "preceding-sibling",
        Axis::Following => "following",
        Axis::Preceding => "preceding",
        Axis::Attribute => "attribute",
        Axis::Namespace => "namespace",
    }
}

fn render_node_test(test: &NodeTest) -> String {
    match test {
        NodeTest::Any => "*".to_string(),
        NodeTest::Name(name) => name.clone(),
        NodeTest::QName { prefix, local } => format!("{prefix}:{local}"),
        NodeTest::Text => "text()".to_string(),
        NodeTest::Node => "node()".to_string(),
        NodeTest::Comment => "comment()".to_string(),
        NodeTest::ProcessingInstruction(None) => "processing-instruction()".to_string(),
        NodeTest::ProcessingInstruction(Some(target)) => {
            format!("processing-instruction({})", fmt_string_literal(target))
        }
    }
}

fn render_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Comparison { left, op, right } => {
            let level = comparison_precedence(*op);
            format!(
                "{}{}{}",
                operand(left, level, true),
                comparison_op(*op),
                operand(right, level + 1, false)
            )
        }
        Predicate::And(a, b) => format!("{} and {}", wrap_predicate(a), wrap_predicate(b)),
        Predicate::Or(a, b) => format!("{} or {}", wrap_predicate(a), wrap_predicate(b)),
        Predicate::Not(inner) => format!("not({})", render_predicate(inner)),
        Predicate::Position(n) => n.to_string(),
        Predicate::Expr(expr) => render_expr(expr),
    }
}

/// Parenthesizes nested `and`/`or` to preserve precedence.
fn wrap_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::And(..) | Predicate::Or(..) => format!("({})", render_predicate(predicate)),
        _ => render_predicate(predicate),
    }
}

fn comparison_op(op: ComparisonOp) -> &'static str {
    match op {
        ComparisonOp::Equal => "=",
        ComparisonOp::NotEqual => "!=",
        ComparisonOp::LessThan => "<",
        ComparisonOp::LessOrEqual => "<=",
        ComparisonOp::GreaterThan => ">",
        ComparisonOp::GreaterOrEqual => ">=",
    }
}

/// Renders an XPath 1.0 string literal, choosing quotes (XPath 1.0 has no
/// escapes). If the value contains both quote kinds, it falls back to `concat()`.
fn fmt_string_literal(s: &str) -> String {
    if !s.contains('\'') {
        format!("'{s}'")
    } else if !s.contains('"') {
        format!("\"{s}\"")
    } else {
        let mut parts: Vec<String> = Vec::new();
        for (i, segment) in s.split('\'').enumerate() {
            if i > 0 {
                parts.push("\"'\"".to_string());
            }
            if !segment.is_empty() {
                parts.push(format!("'{segment}'"));
            }
        }
        format!("concat({})", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse_xpath;

    /// Unparsing then re-parsing must yield the same AST.
    fn assert_roundtrips(xpath: &str) {
        let expr = parse_xpath(xpath).unwrap();
        let rendered = expr.to_string();
        let reparsed = parse_xpath(&rendered)
            .unwrap_or_else(|e| panic!("re-parse of {rendered:?} (from {xpath:?}) failed: {e}"));
        assert_eq!(
            expr, reparsed,
            "roundtrip changed AST: {xpath:?} -> {rendered:?}"
        );
    }

    #[test]
    fn roundtrip_paths() {
        for xpath in [
            "//item",
            "/root/item",
            "/root//item",
            "item",
            "a/b/c",
            "//ns:item",
            "@id",
            "//item/@id",
            "//*",
            "//item[@id='2']",
            "//item[2]",
            "//item[position()=1]",
            "//item[@a='1' and @b='2']",
            "//item[@a='1' or @b='2']",
            "//item[not(@hidden)]",
            "count(//item)",
            "//item[contains(@id, 'x')]",
            "/root/* | //other",
            "//item[@n > 3]",
            "//item[@n <= 5]",
            "2 * 3",
            "2 + 3 * 4",
            "(2 + 3) * 4",
            "-(1 - 2)",
            "//a[1.5]",
            "//a[0]",
            "count(//a | //b)",
            "concat('a', 1 + 2)",
            "//div div 2",
            "/root/text",
            "self::node()",
            "../x",
            "count(//a) = 3",
            "1 < 2 = true()",
            "a or b and c",
            "(a or b) and c",
            "(1 = 1) = (2 = 2)",
            "(//a)[1]",
            "(//a)[1][@x]",
            "(//a)/text()",
            "(//a | //b)[last()]//c",
            "$v[1]/x",
            "id('x')/b",
            "//comment()",
            "//processing-instruction()",
            "//processing-instruction('pi')",
            "(/) * 2",
            "/ | //a",
            "//a[(. = 1 or . = 2) and . != 2]",
            "//a[(@x | @y) = 'v']",
            "-(-1)",
            "--1",
        ] {
            assert_roundtrips(xpath);
        }
    }

    #[test]
    fn string_with_both_quotes_renders_as_concat() {
        use super::super::parser::Expr;
        let expr = Expr::String(r#"it's "x""#.to_string());
        let rendered = expr.to_string();
        assert_eq!(rendered, r#"concat('it', "'", 's "x"')"#);
        // Not the same AST, but the same value.
        let doc = crate::parse("<r/>").unwrap();
        let value = crate::xpath::evaluate(&doc, &rendered).unwrap();
        assert_eq!(value.to_string_value(), r#"it's "x""#);
    }

    #[test]
    fn renders_descendant_abbreviation() {
        let expr = parse_xpath("//item").unwrap();
        assert_eq!(expr.to_string(), "//item");
    }

    #[test]
    fn renders_attribute_abbreviation() {
        let expr = parse_xpath("//item[@id='2']").unwrap();
        assert_eq!(expr.to_string(), "//item[@id='2']");
    }
}
