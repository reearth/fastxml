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
