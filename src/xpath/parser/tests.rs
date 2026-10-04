//! Tests for the XPath parser.

use super::ast::{Axis, ComparisonOp, Expr, NodeTest, Predicate};
use super::parse_xpath;

#[test]
fn test_simple_path() {
    let expr = parse_xpath("/root/child").unwrap();
    if let Expr::Path(path) = expr {
        assert!(path.absolute);
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.steps[0].node_test, NodeTest::Name("root".into()));
        assert_eq!(path.steps[1].node_test, NodeTest::Name("child".into()));
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_descendant() {
    let expr = parse_xpath("//element").unwrap();
    if let Expr::Path(path) = expr {
        assert!(path.absolute);
        // Should have descendant-or-self step + element step
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.steps[0].axis, Axis::DescendantOrSelf);
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_predicate_name() {
    let expr = parse_xpath("//*[name()='Building']").unwrap();
    if let Expr::Path(path) = expr {
        assert_eq!(path.steps.len(), 2);
        let step = &path.steps[1];
        assert_eq!(step.predicates.len(), 1);
        if let Predicate::Comparison { op, .. } = &step.predicates[0] {
            assert_eq!(*op, ComparisonOp::Equal);
        } else {
            panic!("expected comparison predicate");
        }
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_logical_or() {
    let expr = parse_xpath("//*[(name()='A' or name()='B')]").unwrap();
    if let Expr::Path(path) = expr {
        let step = &path.steps[1];
        assert!(!step.predicates.is_empty());
        assert!(matches!(&step.predicates[0], Predicate::Or(_, _)));
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_not_predicate() {
    let expr = parse_xpath("//*[not(name()='Window')]").unwrap();
    if let Expr::Path(path) = expr {
        let step = &path.steps[1];
        assert!(!step.predicates.is_empty());
        assert!(matches!(&step.predicates[0], Predicate::Not(_)));
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_namespaced_path() {
    let expr = parse_xpath("/gml:root/gml:child").unwrap();
    if let Expr::Path(path) = expr {
        assert_eq!(path.steps.len(), 2);
        assert_eq!(
            path.steps[0].node_test,
            NodeTest::QName {
                prefix: "gml".into(),
                local: "root".into(),
            }
        );
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_child_axis() {
    let expr = parse_xpath("./child::*").unwrap();
    if let Expr::Path(path) = expr {
        assert!(!path.absolute);
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.steps[1].axis, Axis::Child);
        assert_eq!(path.steps[1].node_test, NodeTest::Any);
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_text_node() {
    let expr = parse_xpath("/root/text()").unwrap();
    if let Expr::Path(path) = expr {
        assert_eq!(path.steps.len(), 2);
        assert_eq!(path.steps[1].node_test, NodeTest::Text);
    } else {
        panic!("expected Path");
    }
}

#[test]
fn test_top_level_comparison_and_logic() {
    let expr = parse_xpath("count(//a) = 3 and true()").unwrap();
    let Expr::And(left, right) = expr else {
        panic!("expected And, got {expr:?}");
    };
    assert!(matches!(
        *left,
        Expr::Comparison {
            op: ComparisonOp::Equal,
            ..
        }
    ));
    assert!(matches!(*right, Expr::Function { ref name, .. } if name == "true"));
}

#[test]
fn test_filter_and_path_from() {
    let expr = parse_xpath("(//a)[1]/b").unwrap();
    let Expr::PathFrom { base, steps } = expr else {
        panic!("expected PathFrom, got {expr:?}");
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].node_test, NodeTest::Name("b".into()));
    let Expr::Filter { expr, predicates } = *base else {
        panic!("expected Filter");
    };
    assert!(matches!(*expr, Expr::Path(_)));
    assert_eq!(predicates, vec![Predicate::Position(1)]);
}

#[test]
fn test_parenthesized_path_without_predicates_is_the_path() {
    assert_eq!(parse_xpath("(//a)").unwrap(), parse_xpath("//a").unwrap());
}

#[test]
fn test_predicate_lowering() {
    let step_predicate = |xpath: &str| {
        let Expr::Path(path) = parse_xpath(xpath).unwrap() else {
            panic!("expected Path");
        };
        path.steps.last().unwrap().predicates[0].clone()
    };
    // Only a whole-predicate positive integer is a position.
    assert_eq!(step_predicate("a[2]"), Predicate::Position(2));
    assert_eq!(
        step_predicate("a[1.5]"),
        Predicate::Expr(Box::new(Expr::Number(1.5)))
    );
    assert_eq!(
        step_predicate("a[1 and 2]"),
        Predicate::And(
            Box::new(Predicate::Expr(Box::new(Expr::Number(1.0)))),
            Box::new(Predicate::Expr(Box::new(Expr::Number(2.0)))),
        )
    );
    assert!(matches!(step_predicate("a[not(@x)]"), Predicate::Not(_)));
}

#[test]
fn test_comment_and_processing_instruction_node_tests() {
    let last_test = |xpath: &str| {
        let Expr::Path(path) = parse_xpath(xpath).unwrap() else {
            panic!("expected Path");
        };
        path.steps.last().unwrap().node_test.clone()
    };
    assert_eq!(last_test("//comment()"), NodeTest::Comment);
    assert_eq!(
        last_test("processing-instruction()"),
        NodeTest::ProcessingInstruction(None)
    );
    assert_eq!(
        last_test("processing-instruction('x')"),
        NodeTest::ProcessingInstruction(Some("x".into()))
    );
    // Without `(` these are element names.
    assert_eq!(last_test("/comment"), NodeTest::Name("comment".into()));
}
