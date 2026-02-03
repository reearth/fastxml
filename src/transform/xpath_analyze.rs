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

impl std::fmt::Display for NotStreamableReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UsesLast => write!(f, "uses last() function which requires knowing total count"),
            Self::UsesBackwardAxis(axis) => write!(f, "uses backward axis {:?}", axis),
            Self::UsesContextDependentCount => write!(f, "uses context-dependent count"),
            Self::ComplexPredicate => write!(f, "uses complex predicate (and/or/not)"),
            Self::IncompatibleUnion => write!(f, "uses union with incompatible paths"),
            Self::NotPathExpr => write!(f, "expression is not a path expression"),
        }
    }
}

/// A simplified XPath for streaming matching.
#[derive(Debug, Clone, Default)]
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
    /// Namespace URI to match (from namespace-uri() predicate)
    pub namespace_uri: Option<String>,
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
    /// Exact position: `[n]`
    Exact(usize),
    /// Position range: `[position() <= n]`
    LessOrEqual(usize),
    /// Position range: `[position() >= n]`
    GreaterOrEqual(usize),
    /// Position range: `[position() > n]`
    GreaterThan(usize),
    /// Position range: `[position() < n]`
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
        // Note: // in XPath is defined as /descendant-or-self::node()/, so we check for NodeTest::Node
        let descendant_or_self =
            step.axis == Axis::DescendantOrSelf && step.node_test == NodeTest::Node;

        if descendant_or_self {
            // This is the // shorthand, next step is the actual match
            i += 1;
            if i >= path.steps.len() {
                // Just // at the end - match any element
                streamable_steps.push(StreamableStep {
                    descendant_or_self: true,
                    name: None,
                    prefix: None,
                    namespace_uri: None,
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
    let (mut name, prefix) = match &step.node_test {
        NodeTest::Any => (None, None),
        NodeTest::Name(n) => (Some(n.clone()), None),
        NodeTest::QName { prefix, local } => (Some(local.clone()), Some(prefix.clone())),
        NodeTest::Text | NodeTest::Node => (None, None),
    };

    let mut attribute_predicates = Vec::new();
    let mut position_predicate = None;
    let mut namespace_uri = None;
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
            PredicateAnalysis::NamespaceUri(uri) => {
                namespace_uri = Some(uri);
            }
            PredicateAnalysis::LocalName(local) => {
                // local-name() predicate overrides the name from node test
                name = Some(local);
            }
            PredicateAnalysis::Ignored => {}
        }
    }

    Ok((
        StreamableStep {
            descendant_or_self,
            name,
            prefix,
            namespace_uri,
            attribute_predicates,
            position_predicate,
        },
        max_pos,
    ))
}

enum PredicateAnalysis {
    Attribute(AttributePredicate),
    Position(PositionPredicate),
    NamespaceUri(String),
    LocalName(String),
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

                // Check for namespace-uri() = 'URI'
                if name == "namespace-uri" && args.is_empty() {
                    if let Expr::String(uri) = right.as_ref() {
                        if *op == ComparisonOp::Equal {
                            return Ok(PredicateAnalysis::NamespaceUri(uri.clone()));
                        }
                    }
                }

                // Check for local-name() = 'name'
                if name == "local-name" && args.is_empty() {
                    if let Expr::String(local) = right.as_ref() {
                        if *op == ComparisonOp::Equal {
                            return Ok(PredicateAnalysis::LocalName(local.clone()));
                        }
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
        Expr::String(_) | Expr::Number(_) | Expr::Variable(_) => false,
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

    fn get_streamable(xpath: &str) -> Option<StreamableXPath> {
        let expr = parse_xpath(xpath).unwrap();
        match analyze_xpath(&expr) {
            XPathAnalysis::Streamable(s) => Some(s),
            _ => None,
        }
    }

    fn get_not_streamable_reason(xpath: &str) -> Option<NotStreamableReason> {
        let expr = parse_xpath(xpath).unwrap();
        match analyze_xpath(&expr) {
            XPathAnalysis::NotStreamable(r) => Some(r),
            _ => None,
        }
    }

    // =============================================================================
    // Basic Streamable Paths
    // =============================================================================

    #[test]
    fn test_simple_paths_are_streamable() {
        assert!(is_streamable("/root/child"));
        assert!(is_streamable("//item"));
        assert!(is_streamable("/root/items/item"));
    }

    #[test]
    fn test_absolute_vs_relative() {
        let abs = get_streamable("/root/child").unwrap();
        assert!(abs.absolute);

        let rel = get_streamable("item").unwrap();
        assert!(!rel.absolute);
    }

    #[test]
    fn test_descendant_or_self() {
        let result = get_streamable("//item").unwrap();
        assert!(result.steps.iter().any(|s| s.descendant_or_self));
    }

    #[test]
    fn test_double_slash_at_end() {
        // Just // should be streamable
        assert!(is_streamable("//"));
    }

    #[test]
    fn test_wildcard() {
        assert!(is_streamable("//*"));
        assert!(is_streamable("/root/*"));
    }

    // =============================================================================
    // Attribute Predicates
    // =============================================================================

    #[test]
    fn test_attribute_predicates_are_streamable() {
        assert!(is_streamable("//item[@id='2']"));
        assert!(is_streamable("/root/item[@name='test']"));
    }

    #[test]
    fn test_attribute_predicate_details() {
        let result = get_streamable("//item[@id='123']").unwrap();
        let step = result
            .steps
            .iter()
            .find(|s| s.name.as_deref() == Some("item"))
            .unwrap();
        assert_eq!(step.attribute_predicates.len(), 1);
        assert_eq!(step.attribute_predicates[0].name, "id");
        assert_eq!(step.attribute_predicates[0].value, "123");
    }

    #[test]
    fn test_attribute_existence_check() {
        // @attr without value is existence check
        assert!(is_streamable("//item[@id]"));
    }

    // =============================================================================
    // Position Predicates
    // =============================================================================

    #[test]
    fn test_position_predicates_are_streamable() {
        assert!(is_streamable("//item[1]"));
        assert!(is_streamable("//item[position()<=3]"));
    }

    #[test]
    fn test_exact_position() {
        let result = get_streamable("//item[2]").unwrap();
        let step = result
            .steps
            .iter()
            .find(|s| s.name.as_deref() == Some("item"))
            .unwrap();
        assert!(matches!(
            step.position_predicate,
            Some(PositionPredicate::Exact(2))
        ));
    }

    #[test]
    fn test_position_less_or_equal() {
        let result = get_streamable("//item[position()<=5]").unwrap();
        assert_eq!(result.max_position, Some(5));
    }

    #[test]
    fn test_position_less_than() {
        let result = get_streamable("//item[position()<5]").unwrap();
        // max_position for < should be n-1 = 4
        assert_eq!(result.max_position, Some(4));
    }

    #[test]
    fn test_position_greater_or_equal() {
        let result = get_streamable("//item[position()>=3]").unwrap();
        // >= doesn't have upper bound
        assert_eq!(result.max_position, None);
    }

    #[test]
    fn test_position_greater_than() {
        let result = get_streamable("//item[position()>3]").unwrap();
        // > doesn't have upper bound
        assert_eq!(result.max_position, None);
    }

    #[test]
    fn test_position_not_equal_ignored() {
        // position() != n is ignored for optimization
        assert!(is_streamable("//item[position()!=3]"));
    }

    // =============================================================================
    // Has Position Predicates
    // =============================================================================

    #[test]
    fn test_has_position_predicates_true() {
        let result = get_streamable("//item[1]").unwrap();
        assert!(result.has_position_predicates());
    }

    #[test]
    fn test_has_position_predicates_false() {
        let result = get_streamable("//item[@id='1']").unwrap();
        assert!(!result.has_position_predicates());
    }

    // =============================================================================
    // Not Streamable - last()
    // =============================================================================

    #[test]
    fn test_last_is_not_streamable() {
        assert!(!is_streamable("//item[last()]"));
        assert!(!is_streamable("//item[position()=last()]"));
    }

    #[test]
    fn test_last_reason() {
        let reason = get_not_streamable_reason("//item[last()]").unwrap();
        assert!(matches!(reason, NotStreamableReason::UsesLast));
    }

    #[test]
    fn test_last_in_predicate_comparison() {
        // last() in comparison
        assert!(!is_streamable("//item[position()=last()]"));
    }

    // =============================================================================
    // Not Streamable - Backward Axes
    // =============================================================================

    #[test]
    fn test_backward_axes_not_streamable() {
        assert!(!is_streamable("//item/parent::*"));
        assert!(!is_streamable("//item/ancestor::root"));
    }

    #[test]
    fn test_parent_axis_reason() {
        let reason = get_not_streamable_reason("//item/parent::*").unwrap();
        assert!(matches!(
            reason,
            NotStreamableReason::UsesBackwardAxis(Axis::Parent)
        ));
    }

    #[test]
    fn test_ancestor_axis_reason() {
        let reason = get_not_streamable_reason("//item/ancestor::root").unwrap();
        assert!(matches!(
            reason,
            NotStreamableReason::UsesBackwardAxis(Axis::Ancestor)
        ));
    }

    #[test]
    fn test_preceding_sibling_axis_reason() {
        let reason = get_not_streamable_reason("//item/preceding-sibling::*").unwrap();
        assert!(matches!(
            reason,
            NotStreamableReason::UsesBackwardAxis(Axis::PrecedingSibling)
        ));
    }

    #[test]
    fn test_preceding_axis_reason() {
        let reason = get_not_streamable_reason("//item/preceding::*").unwrap();
        assert!(matches!(
            reason,
            NotStreamableReason::UsesBackwardAxis(Axis::Preceding)
        ));
    }

    #[test]
    fn test_ancestor_or_self_axis_reason() {
        let reason = get_not_streamable_reason("//item/ancestor-or-self::*").unwrap();
        assert!(matches!(
            reason,
            NotStreamableReason::UsesBackwardAxis(Axis::AncestorOrSelf)
        ));
    }

    // =============================================================================
    // Not Streamable - Union
    // =============================================================================

    #[test]
    fn test_union_not_streamable() {
        assert!(!is_streamable("//a | //b"));
    }

    #[test]
    fn test_union_reason() {
        let reason = get_not_streamable_reason("//a | //b").unwrap();
        assert!(matches!(reason, NotStreamableReason::IncompatibleUnion));
    }

    // =============================================================================
    // Not Streamable - Complex Predicates
    // =============================================================================

    #[test]
    fn test_and_predicate_not_streamable() {
        // And/Or predicates are complex
        assert!(!is_streamable("//item[@a='1' and @b='2']"));
    }

    #[test]
    fn test_complex_predicate_reason() {
        let reason = get_not_streamable_reason("//item[@a='1' and @b='2']").unwrap();
        assert!(matches!(reason, NotStreamableReason::ComplexPredicate));
    }

    #[test]
    fn test_not_predicate() {
        // not() predicates are complex
        let reason = get_not_streamable_reason("//item[not(@a)]").unwrap();
        assert!(matches!(reason, NotStreamableReason::ComplexPredicate));
    }

    // =============================================================================
    // Not Streamable - Non-Path Expressions
    // =============================================================================

    #[test]
    fn test_number_literal_not_streamable() {
        let expr = Expr::Number(42.0);
        let result = analyze_xpath(&expr);
        assert!(matches!(
            result,
            XPathAnalysis::NotStreamable(NotStreamableReason::NotPathExpr)
        ));
    }

    #[test]
    fn test_string_literal_not_streamable() {
        let expr = Expr::String("test".to_string());
        let result = analyze_xpath(&expr);
        assert!(matches!(
            result,
            XPathAnalysis::NotStreamable(NotStreamableReason::NotPathExpr)
        ));
    }

    // =============================================================================
    // QName Support
    // =============================================================================

    #[test]
    fn test_qname_streamable() {
        assert!(is_streamable("//ns:item"));
    }

    #[test]
    fn test_qname_prefix_captured() {
        let result = get_streamable("//ns:item").unwrap();
        let step = result
            .steps
            .iter()
            .find(|s| s.name.as_deref() == Some("item"))
            .unwrap();
        assert_eq!(step.prefix.as_deref(), Some("ns"));
    }

    // =============================================================================
    // Forward Axes
    // =============================================================================

    #[test]
    fn test_child_axis_streamable() {
        assert!(is_streamable("/root/child::item"));
    }

    #[test]
    fn test_descendant_axis_streamable() {
        assert!(is_streamable("/root/descendant::item"));
    }

    #[test]
    fn test_following_sibling_axis_streamable() {
        assert!(is_streamable("/root/item/following-sibling::*"));
    }

    #[test]
    fn test_following_axis_streamable() {
        assert!(is_streamable("/root/item/following::*"));
    }

    #[test]
    fn test_self_axis_streamable() {
        assert!(is_streamable("/root/self::*"));
    }

    // =============================================================================
    // is_backward_axis helper
    // =============================================================================

    #[test]
    fn test_is_backward_axis() {
        assert!(is_backward_axis(Axis::Parent));
        assert!(is_backward_axis(Axis::Ancestor));
        assert!(is_backward_axis(Axis::AncestorOrSelf));
        assert!(is_backward_axis(Axis::Preceding));
        assert!(is_backward_axis(Axis::PrecedingSibling));

        assert!(!is_backward_axis(Axis::Child));
        assert!(!is_backward_axis(Axis::Descendant));
        assert!(!is_backward_axis(Axis::DescendantOrSelf));
        assert!(!is_backward_axis(Axis::Following));
        assert!(!is_backward_axis(Axis::FollowingSibling));
        assert!(!is_backward_axis(Axis::SelfNode));
        assert!(!is_backward_axis(Axis::Attribute));
        assert!(!is_backward_axis(Axis::Namespace));
    }

    // =============================================================================
    // position_max helper
    // =============================================================================

    #[test]
    fn test_position_max() {
        assert_eq!(position_max(&PositionPredicate::Exact(5)), Some(5));
        assert_eq!(position_max(&PositionPredicate::LessOrEqual(10)), Some(10));
        assert_eq!(position_max(&PositionPredicate::LessThan(10)), Some(9));
        assert_eq!(position_max(&PositionPredicate::LessThan(1)), Some(0));
        assert_eq!(position_max(&PositionPredicate::GreaterOrEqual(3)), None);
        assert_eq!(position_max(&PositionPredicate::GreaterThan(3)), None);
    }

    // =============================================================================
    // Edge Cases
    // =============================================================================

    #[test]
    fn test_text_node_test() {
        assert!(is_streamable("//text()"));
    }

    #[test]
    fn test_node_node_test() {
        assert!(is_streamable("//node()"));
    }

    #[test]
    fn test_multiple_predicates() {
        // Multiple attribute predicates on same step
        let result = get_streamable("//item[@id='1'][@type='foo']");
        // Should be streamable if predicates are simple
        assert!(result.is_some());
    }

    #[test]
    fn test_backward_axis_after_descendant() {
        // Even after //, backward axis is not streamable
        assert!(!is_streamable("//item/parent::*"));
    }

    // =============================================================================
    // Namespace URI Matching
    // =============================================================================

    #[test]
    fn test_namespace_uri_predicate_is_streamable() {
        assert!(is_streamable("//*[namespace-uri()='http://example.com']"));
    }

    #[test]
    fn test_namespace_uri_predicate_captured() {
        let result = get_streamable("//*[namespace-uri()='http://example.com']").unwrap();
        let step = result.steps.iter().find(|s| s.descendant_or_self).unwrap();
        assert_eq!(step.namespace_uri.as_deref(), Some("http://example.com"));
    }

    #[test]
    fn test_local_name_predicate_is_streamable() {
        assert!(is_streamable("//*[local-name()='item']"));
    }

    #[test]
    fn test_local_name_predicate_captured() {
        let result = get_streamable("//*[local-name()='item']").unwrap();
        let step = result.steps.iter().find(|s| s.descendant_or_self).unwrap();
        assert_eq!(step.name.as_deref(), Some("item"));
    }

    #[test]
    fn test_namespace_uri_and_local_name_combined() {
        let result =
            get_streamable("//*[namespace-uri()='http://example.com'][local-name()='item']")
                .unwrap();
        let step = result.steps.iter().find(|s| s.descendant_or_self).unwrap();
        assert_eq!(step.namespace_uri.as_deref(), Some("http://example.com"));
        assert_eq!(step.name.as_deref(), Some("item"));
    }

    #[test]
    fn test_namespace_uri_with_attribute_predicate() {
        let result = get_streamable("//*[namespace-uri()='http://example.com'][@id='1']").unwrap();
        let step = result.steps.iter().find(|s| s.descendant_or_self).unwrap();
        assert_eq!(step.namespace_uri.as_deref(), Some("http://example.com"));
        assert_eq!(step.attribute_predicates.len(), 1);
        assert_eq!(step.attribute_predicates[0].name, "id");
    }
}
