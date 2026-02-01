//! XPath analysis for determining streamability.
//!
//! Analyzes XPath expressions to determine if they can be processed
//! in a single streaming pass (forward-only) or require two-pass processing.

use crate::xpath::parser::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step};

/// Result of XPath analysis.
#[derive(Debug, Clone)]
pub enum XPathAnalysis {
    /// XPath can be processed in a single streaming pass
    Streamable(StreamableXPath),
    /// XPath requires two-pass processing
    NotStreamable(NotStreamableReason),
}

/// Reason why an XPath is not streamable.
#[derive(Debug, Clone)]
pub enum NotStreamableReason {
    /// Uses last() function which needs total count
    UsesLast,
    /// Uses backward axis (parent, ancestor, preceding, etc.)
    UsesBackwardAxis(Axis),
    /// Uses count() on siblings or other context-dependent count
    UsesContextDependentCount,
    /// Uses complex predicate that requires full tree evaluation
    ComplexPredicate,
    /// Uses union with incompatible paths
    IncompatibleUnion,
    /// Expression is not a path expression
    NotPathExpr,
}

/// A simplified XPath for streaming matching.
#[derive(Debug, Clone)]
pub struct StreamableXPath {
    /// The path steps for matching
    pub steps: Vec<StreamableStep>,
    /// Whether this is an absolute path
    pub absolute: bool,
    /// Maximum position for position() predicates (if bounded)
    pub max_position: Option<usize>,
}

/// A simplified step for streaming matching.
#[derive(Debug, Clone)]
pub struct StreamableStep {
    /// Match any descendant (from //)
    pub descendant_or_self: bool,
    /// Element name to match (None = any)
    pub name: Option<String>,
    /// Namespace prefix to match
    pub prefix: Option<String>,
    /// Attribute predicates to check
    pub attribute_predicates: Vec<AttributePredicate>,
    /// Position predicate (if any)
    pub position_predicate: Option<PositionPredicate>,
}

/// An attribute predicate for streaming matching.
#[derive(Debug, Clone)]
pub struct AttributePredicate {
    /// Attribute name
    pub name: String,
    /// Comparison operator
    pub op: ComparisonOp,
    /// Expected value
    pub value: String,
}

/// A position predicate for streaming matching.
#[derive(Debug, Clone)]
pub enum PositionPredicate {
    /// Exact position: [n]
    Exact(usize),
    /// Position range: [position() <= n]
    LessOrEqual(usize),
    /// Position range: [position() >= n]
    GreaterOrEqual(usize),
    /// Position range: [position() > n]
    GreaterThan(usize),
    /// Position range: [position() < n]
    LessThan(usize),
}

impl StreamableXPath {
    /// Returns true if this XPath has any position predicates.
    pub fn has_position_predicates(&self) -> bool {
        self.steps.iter().any(|s| s.position_predicate.is_some())
    }
}

/// Analyzes an XPath expression to determine if it's streamable.
pub fn analyze_xpath(expr: &Expr) -> XPathAnalysis {
    match expr {
        Expr::Path(path) => analyze_path(path),
        Expr::Union(paths) => {
            // Union is streamable if all paths are streamable and compatible
            let mut results: Vec<StreamableXPath> = Vec::new();
            for path in paths {
                match analyze_path(path) {
                    XPathAnalysis::Streamable(s) => results.push(s),
                    not_streamable => return not_streamable,
                }
            }
            // For now, treat unions as not streamable for simplicity
            XPathAnalysis::NotStreamable(NotStreamableReason::IncompatibleUnion)
        }
        _ => XPathAnalysis::NotStreamable(NotStreamableReason::NotPathExpr),
    }
}

fn analyze_path(path: &PathExpr) -> XPathAnalysis {
    let mut streamable_steps = Vec::new();
    let mut max_position: Option<usize> = None;
    let mut i = 0;

    while i < path.steps.len() {
        let step = &path.steps[i];

        // Check for backward axes
        if is_backward_axis(step.axis) {
            return XPathAnalysis::NotStreamable(NotStreamableReason::UsesBackwardAxis(step.axis));
        }

        // Handle descendant-or-self from //
        let descendant_or_self =
            step.axis == Axis::DescendantOrSelf && step.node_test == NodeTest::Any;

        if descendant_or_self {
            // This is the // shorthand, next step is the actual match
            i += 1;
            if i >= path.steps.len() {
                // Just // at the end - match any element
                streamable_steps.push(StreamableStep {
                    descendant_or_self: true,
                    name: None,
                    prefix: None,
                    attribute_predicates: Vec::new(),
                    position_predicate: None,
                });
                break;
            }
            let next_step = &path.steps[i];

            // Check the actual step after //
            if is_backward_axis(next_step.axis) {
                return XPathAnalysis::NotStreamable(NotStreamableReason::UsesBackwardAxis(
                    next_step.axis,
                ));
            }

            match analyze_step(next_step, true) {
                Ok((s, pos)) => {
                    if let Some(p) = pos {
                        max_position = Some(max_position.map_or(p, |m| m.max(p)));
                    }
                    streamable_steps.push(s);
                }
                Err(reason) => return XPathAnalysis::NotStreamable(reason),
            }
        } else {
            match analyze_step(step, false) {
                Ok((s, pos)) => {
                    if let Some(p) = pos {
                        max_position = Some(max_position.map_or(p, |m| m.max(p)));
                    }
                    streamable_steps.push(s);
                }
                Err(reason) => return XPathAnalysis::NotStreamable(reason),
            }
        }

        i += 1;
    }

    XPathAnalysis::Streamable(StreamableXPath {
        steps: streamable_steps,
        absolute: path.absolute,
        max_position,
    })
}

fn analyze_step(
    step: &Step,
    descendant_or_self: bool,
) -> Result<(StreamableStep, Option<usize>), NotStreamableReason> {
    let (name, prefix) = match &step.node_test {
        NodeTest::Any => (None, None),
        NodeTest::Name(n) => (Some(n.clone()), None),
        NodeTest::QName { prefix, local } => (Some(local.clone()), Some(prefix.clone())),
        NodeTest::Text | NodeTest::Node => (None, None),
    };

    let mut attribute_predicates = Vec::new();
    let mut position_predicate = None;
    let mut max_pos: Option<usize> = None;

    for pred in &step.predicates {
        match analyze_predicate(pred)? {
            PredicateAnalysis::Attribute(ap) => attribute_predicates.push(ap),
            PredicateAnalysis::Position(pp) => {
                if let Some(max) = position_max(&pp) {
                    max_pos = Some(max_pos.map_or(max, |m| m.max(max)));
                }
                position_predicate = Some(pp);
            }
            PredicateAnalysis::Ignored => {}
        }
    }

    Ok((
        StreamableStep {
            descendant_or_self,
            name,
            prefix,
            attribute_predicates,
            position_predicate,
        },
        max_pos,
    ))
}

enum PredicateAnalysis {
    Attribute(AttributePredicate),
    Position(PositionPredicate),
    Ignored,
}

fn analyze_predicate(pred: &Predicate) -> Result<PredicateAnalysis, NotStreamableReason> {
    match pred {
        Predicate::Position(n) => Ok(PredicateAnalysis::Position(PositionPredicate::Exact(*n))),

        Predicate::Comparison { left, op, right } => {
            // Check for @attr = 'value'
            if let Expr::Path(path) = left.as_ref() {
                if path.steps.len() == 1 && path.steps[0].axis == Axis::Attribute {
                    if let NodeTest::Name(attr_name) = &path.steps[0].node_test {
                        if let Expr::String(value) = right.as_ref() {
                            return Ok(PredicateAnalysis::Attribute(AttributePredicate {
                                name: attr_name.clone(),
                                op: *op,
                                value: value.clone(),
                            }));
                        }
                    }
                }
            }

            // Check for position() comparisons
            if let Expr::Function { name, args } = left.as_ref() {
                if name == "position" && args.is_empty() {
                    if let Expr::Number(n) = right.as_ref() {
                        let pos = *n as usize;
                        return match op {
                            ComparisonOp::Equal => {
                                Ok(PredicateAnalysis::Position(PositionPredicate::Exact(pos)))
                            }
                            ComparisonOp::LessOrEqual => Ok(PredicateAnalysis::Position(
                                PositionPredicate::LessOrEqual(pos),
                            )),
                            ComparisonOp::LessThan => Ok(PredicateAnalysis::Position(
                                PositionPredicate::LessThan(pos),
                            )),
                            ComparisonOp::GreaterOrEqual => Ok(PredicateAnalysis::Position(
                                PositionPredicate::GreaterOrEqual(pos),
                            )),
                            ComparisonOp::GreaterThan => Ok(PredicateAnalysis::Position(
                                PositionPredicate::GreaterThan(pos),
                            )),
                            ComparisonOp::NotEqual => {
                                // position() != n is not useful for streaming optimization
                                Ok(PredicateAnalysis::Ignored)
                            }
                        };
                    }
                }
            }

            // Check for last() usage
            if uses_last(left) || uses_last(right) {
                return Err(NotStreamableReason::UsesLast);
            }

            // Other comparisons might work, but complex ones need fallback
            Ok(PredicateAnalysis::Ignored)
        }

        Predicate::Expr(expr) => {
            // Check for last() usage
            if uses_last(expr) {
                return Err(NotStreamableReason::UsesLast);
            }

            // Check for @attr (existence check)
            if let Expr::Path(path) = expr.as_ref() {
                if path.steps.len() == 1 && path.steps[0].axis == Axis::Attribute {
                    if let NodeTest::Name(attr_name) = &path.steps[0].node_test {
                        return Ok(PredicateAnalysis::Attribute(AttributePredicate {
                            name: attr_name.clone(),
                            op: ComparisonOp::NotEqual, // existence check
                            value: String::new(),
                        }));
                    }
                }
            }

            // For complex expressions, we'll rely on fallback
            Err(NotStreamableReason::ComplexPredicate)
        }

        Predicate::And(left, right) | Predicate::Or(left, right) => {
            // Check for last() in either side
            if predicate_uses_last(left) || predicate_uses_last(right) {
                return Err(NotStreamableReason::UsesLast);
            }
            // Complex predicates need fallback
            Err(NotStreamableReason::ComplexPredicate)
        }

        Predicate::Not(inner) => {
            if predicate_uses_last(inner) {
                return Err(NotStreamableReason::UsesLast);
            }
            Err(NotStreamableReason::ComplexPredicate)
        }
    }
}

fn is_backward_axis(axis: Axis) -> bool {
    matches!(
        axis,
        Axis::Parent
            | Axis::Ancestor
            | Axis::AncestorOrSelf
            | Axis::Preceding
            | Axis::PrecedingSibling
    )
}

fn uses_last(expr: &Expr) -> bool {
    match expr {
        Expr::Function { name, args } => {
            if name == "last" {
                return true;
            }
            args.iter().any(uses_last)
        }
        Expr::Path(_) => false,
        Expr::String(_) | Expr::Number(_) => false,
        Expr::Union(paths) => paths.iter().any(|p| p.steps.iter().any(step_uses_last)),
        Expr::Add(l, r)
        | Expr::Subtract(l, r)
        | Expr::Multiply(l, r)
        | Expr::Divide(l, r)
        | Expr::Modulo(l, r) => uses_last(l) || uses_last(r),
        Expr::Negate(e) => uses_last(e),
    }
}

fn step_uses_last(step: &Step) -> bool {
    step.predicates.iter().any(predicate_uses_last)
}

fn predicate_uses_last(pred: &Predicate) -> bool {
    match pred {
        Predicate::Comparison { left, right, .. } => uses_last(left) || uses_last(right),
        Predicate::And(l, r) | Predicate::Or(l, r) => {
            predicate_uses_last(l) || predicate_uses_last(r)
        }
        Predicate::Not(inner) => predicate_uses_last(inner),
        Predicate::Position(_) => false,
        Predicate::Expr(e) => uses_last(e),
    }
}

fn position_max(pp: &PositionPredicate) -> Option<usize> {
    match pp {
        PositionPredicate::Exact(n) => Some(*n),
        PositionPredicate::LessOrEqual(n) => Some(*n),
        PositionPredicate::LessThan(n) => Some(n.saturating_sub(1)),
        PositionPredicate::GreaterOrEqual(_) | PositionPredicate::GreaterThan(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xpath::parser::parse_xpath;

    fn is_streamable(xpath: &str) -> bool {
        let expr = parse_xpath(xpath).unwrap();
        matches!(analyze_xpath(&expr), XPathAnalysis::Streamable(_))
    }

    #[test]
    fn test_simple_paths_are_streamable() {
        assert!(is_streamable("/root/child"));
        assert!(is_streamable("//item"));
        assert!(is_streamable("/root/items/item"));
    }

    #[test]
    fn test_attribute_predicates_are_streamable() {
        assert!(is_streamable("//item[@id='2']"));
        assert!(is_streamable("/root/item[@name='test']"));
    }

    #[test]
    fn test_position_predicates_are_streamable() {
        assert!(is_streamable("//item[1]"));
        assert!(is_streamable("//item[position()<=3]"));
    }

    #[test]
    fn test_last_is_not_streamable() {
        assert!(!is_streamable("//item[last()]"));
        assert!(!is_streamable("//item[position()=last()]"));
    }

    #[test]
    fn test_backward_axes_not_streamable() {
        assert!(!is_streamable("//item/parent::*"));
        assert!(!is_streamable("//item/ancestor::root"));
    }
}
