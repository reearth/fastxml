//! Unparsing: turn an XPath [`Expr`] AST back into an XPath 1.0 string.
//!
//! Provides `Display` for [`Expr`] and [`PathExpr`]. The output is a normalized
//! but *equivalent* expression — it re-parses to the same AST, though spacing and
//! redundant parentheses may differ from the original source. This backs
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
        Expr::Union(paths) => paths
            .iter()
            .map(render_path)
            .collect::<Vec<_>>()
            .join(" | "),
        Expr::Add(l, r) => format!("{} + {}", arith_operand(l), arith_operand(r)),
        Expr::Subtract(l, r) => format!("{} - {}", arith_operand(l), arith_operand(r)),
        Expr::Multiply(l, r) => format!("{} * {}", arith_operand(l), arith_operand(r)),
        Expr::Divide(l, r) => format!("{} div {}", arith_operand(l), arith_operand(r)),
        Expr::Modulo(l, r) => format!("{} mod {}", arith_operand(l), arith_operand(r)),
        Expr::Negate(e) => format!("-{}", arith_operand(e)),
    }
}

/// Parenthesizes a binary/unary arithmetic operand to preserve precedence.
///
/// This is conservative (it may add parentheses a hand-written expression would
/// omit) but always yields an equivalent expression.
fn arith_operand(expr: &Expr) -> String {
    match expr {
        Expr::Add(..)
        | Expr::Subtract(..)
        | Expr::Multiply(..)
        | Expr::Divide(..)
        | Expr::Modulo(..)
        | Expr::Negate(..) => format!("({})", render_expr(expr)),
        _ => render_expr(expr),
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
    }
}

fn render_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Comparison { left, op, right } => {
            format!(
                "{}{}{}",
                render_expr(left),
                comparison_op(*op),
                render_expr(right)
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
        ] {
            assert_roundtrips(xpath);
        }
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
