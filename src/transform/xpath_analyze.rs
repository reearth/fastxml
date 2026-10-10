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
    /// Uses a predicate, axis or node test that the single-pass matcher cannot
    /// evaluate exactly (e.g. `and`/`or`/`not()`, numeric or relational
    /// comparisons, a position after another predicate, `following-sibling::`,
    /// `self::`, `text()`)
    ComplexPredicate,
    /// Uses a union (`|`); unions are never streamed
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
            Self::ComplexPredicate => write!(
                f,
                "uses a predicate, axis or node test that cannot be evaluated in a single streaming pass"
            ),
            Self::IncompatibleUnion => write!(f, "uses a union, which is not streamable"),
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
            // Report a non-streamable branch first; otherwise the union itself
            // is not streamable (its branches would have to be merged in
            // document order).
            for path in paths {
                if let not_streamable @ XPathAnalysis::NotStreamable(_) = analyze_path(path) {
                    return not_streamable;
                }
            }
            XPathAnalysis::NotStreamable(NotStreamableReason::IncompatibleUnion)
        }
        _ => XPathAnalysis::NotStreamable(NotStreamableReason::NotPathExpr),
    }
}

fn analyze_path(path: &PathExpr) -> XPathAnalysis {
    let mut streamable_steps = Vec::new();
    let mut max_position: Option<usize> = None;
    // Set by a `//` (`/descendant-or-self::node()/`) step: the next step may
    // select any descendant of the current context, not only its children.
    let mut pending_descendant = false;

    for step in &path.steps {
        // Check for backward axes
        if is_backward_axis(step.axis) {
            return XPathAnalysis::NotStreamable(NotStreamableReason::UsesBackwardAxis(step.axis));
        }

        // `//` in XPath is `/descendant-or-self::node()/`
        if step.axis == Axis::DescendantOrSelf
            && step.node_test == NodeTest::Node
            && step.predicates.is_empty()
        {
            pending_descendant = true;
            continue;
        }

        let descendant = match step.axis {
            Axis::Child => pending_descendant,
            Axis::Descendant => true,
            // self::, following::, following-sibling::, attribute::, namespace::
            // and descendant-or-self:: with a name test select nodes the
            // streaming matcher cannot track exactly.
            _ => return XPathAnalysis::NotStreamable(NotStreamableReason::ComplexPredicate),
        };
        pending_descendant = false;

        match analyze_step(step, descendant) {
            Ok((s, pos)) => {
                if let Some(p) = pos {
                    max_position = Some(max_position.map_or(p, |m| m.max(p)));
                }
                streamable_steps.push(s);
            }
            Err(reason) => return XPathAnalysis::NotStreamable(reason),
        }
    }

    // A trailing `//` (or an empty path) selects non-element nodes too.
    if pending_descendant || streamable_steps.is_empty() {
        return XPathAnalysis::NotStreamable(NotStreamableReason::ComplexPredicate);
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
        NodeTest::QName { prefix, local } if local == "*" => (None, Some(prefix.clone())),
        NodeTest::QName { prefix, local } => (Some(local.clone()), Some(prefix.clone())),
        // text() / node() select non-element nodes, which are not matched
        _ => return Err(NotStreamableReason::ComplexPredicate),
    };

    let mut attribute_predicates = Vec::new();
    let mut position_predicate = None;
    let mut namespace_uri = None;
    let mut local_name_from_predicate = false;
    let mut max_pos: Option<usize> = None;

    for (index, pred) in step.predicates.iter().enumerate() {
        match analyze_predicate(pred)? {
            PredicateAnalysis::Attribute(ap) => attribute_predicates.push(ap),
            PredicateAnalysis::Position(pp) => {
                // A position is only the sibling position when it is the first
                // predicate of a child step: after another predicate it counts
                // within the filtered set, and on `descendant::` it counts all
                // descendants of the context.
                if index != 0 || step.axis == Axis::Descendant {
                    return Err(NotStreamableReason::ComplexPredicate);
                }
                if let Some(max) = position_max(&pp) {
                    max_pos = Some(max_pos.map_or(max, |m| m.max(max)));
                }
                position_predicate = Some(pp);
            }
            PredicateAnalysis::NamespaceUri(uri) => {
                if namespace_uri.is_some() {
                    return Err(NotStreamableReason::ComplexPredicate);
                }
                namespace_uri = Some(uri);
            }
            PredicateAnalysis::LocalName(local) => {
                // Folding `local-name()='x'` into the name test is only exact
                // when the node test does not already constrain the name.
                if name.is_some() {
                    return Err(NotStreamableReason::ComplexPredicate);
                }
                name = Some(local);
                local_name_from_predicate = true;
            }
        }
    }

    // The sibling position is counted per node test; a name folded in from
    // `local-name()` would change which siblings are counted.
    if local_name_from_predicate && position_predicate.is_some() {
        return Err(NotStreamableReason::ComplexPredicate);
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
}

/// Returns the attribute name (`local` or `prefix:local`) of an `@name` path.
fn attribute_name(expr: &Expr) -> Option<String> {
    let Expr::Path(path) = expr else {
        return None;
    };
    if path.absolute || path.steps.len() != 1 {
        return None;
    }
    let step = &path.steps[0];
    if step.axis != Axis::Attribute || !step.predicates.is_empty() {
        return None;
    }
    match &step.node_test {
        NodeTest::Name(n) => Some(n.clone()),
        NodeTest::QName { prefix, local } if local != "*" => Some(format!("{prefix}:{local}")),
        _ => None,
    }
}

/// Converts an XPath number to a non-negative integer position, if it is one.
fn as_position(n: f64) -> Option<usize> {
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0).then_some(n as usize)
}

fn analyze_predicate(pred: &Predicate) -> Result<PredicateAnalysis, NotStreamableReason> {
    if predicate_uses_last(pred) {
        return Err(NotStreamableReason::UsesLast);
    }

    match pred {
        Predicate::Position(n) => Ok(PredicateAnalysis::Position(PositionPredicate::Exact(*n))),

        Predicate::Comparison { left, op, right } => {
            // @attr = 'value' / @attr != 'value' (string comparison only:
            // numeric and relational comparisons convert values to numbers)
            if let (Some(attr_name), Expr::String(value)) = (attribute_name(left), right.as_ref()) {
                return match op {
                    ComparisonOp::Equal => Ok(PredicateAnalysis::Attribute(AttributePredicate {
                        name: attr_name,
                        op: *op,
                        value: value.clone(),
                    })),
                    // `@a != ''` is not representable: an empty NotEqual value
                    // encodes the `[@a]` existence check.
                    ComparisonOp::NotEqual if !value.is_empty() => {
                        Ok(PredicateAnalysis::Attribute(AttributePredicate {
                            name: attr_name,
                            op: *op,
                            value: value.clone(),
                        }))
                    }
                    _ => Err(NotStreamableReason::ComplexPredicate),
                };
            }

            if let (Expr::Function { name, args }, right) = (left.as_ref(), right.as_ref()) {
                if !args.is_empty() {
                    return Err(NotStreamableReason::ComplexPredicate);
                }
                match (name.as_str(), right) {
                    ("position", Expr::Number(n)) => {
                        let pos = as_position(*n).ok_or(NotStreamableReason::ComplexPredicate)?;
                        let pp = match op {
                            ComparisonOp::Equal => PositionPredicate::Exact(pos),
                            ComparisonOp::LessOrEqual => PositionPredicate::LessOrEqual(pos),
                            ComparisonOp::LessThan => PositionPredicate::LessThan(pos),
                            ComparisonOp::GreaterOrEqual => PositionPredicate::GreaterOrEqual(pos),
                            ComparisonOp::GreaterThan => PositionPredicate::GreaterThan(pos),
                            ComparisonOp::NotEqual => {
                                return Err(NotStreamableReason::ComplexPredicate);
                            }
                        };
                        return Ok(PredicateAnalysis::Position(pp));
                    }
                    ("namespace-uri", Expr::String(uri)) if *op == ComparisonOp::Equal => {
                        return Ok(PredicateAnalysis::NamespaceUri(uri.clone()));
                    }
                    ("local-name", Expr::String(local)) if *op == ComparisonOp::Equal => {
                        return Ok(PredicateAnalysis::LocalName(local.clone()));
                    }
                    _ => {}
                }
            }

            Err(NotStreamableReason::ComplexPredicate)
        }

        Predicate::Expr(expr) => {
            // @attr (existence check)
            if let Some(attr_name) = attribute_name(expr) {
                return Ok(PredicateAnalysis::Attribute(AttributePredicate {
                    name: attr_name,
                    op: ComparisonOp::NotEqual, // existence check
                    value: String::new(),
                }));
            }

            Err(NotStreamableReason::ComplexPredicate)
        }

        Predicate::And(..) | Predicate::Or(..) | Predicate::Not(..) => {
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
    fn test_double_slash_at_end_not_streamable() {
        // A bare `//` selects every node, not only elements
        assert!(!is_streamable("//"));
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
    fn test_position_not_equal_not_streamable() {
        // position() != n has no streaming representation; it must not be dropped
        assert!(!is_streamable("//item[position()!=3]"));
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
    fn test_sibling_and_self_axes_not_streamable() {
        // The streaming matcher only tracks the ancestor chain of the current
        // element, so these axes cannot be evaluated exactly.
        assert!(!is_streamable("/root/item/following-sibling::*"));
        assert!(!is_streamable("/root/item/following::*"));
        assert!(!is_streamable("/root/self::*"));
        assert!(!is_streamable("//a/descendant-or-self::item"));
    }

    #[test]
    fn test_descendant_axis_is_descendant_step() {
        let result = get_streamable("/root/descendant::item").unwrap();
        assert_eq!(result.steps.len(), 2);
        assert!(result.steps[1].descendant_or_self);
        // A position on descendant:: counts all descendants, not siblings
        assert!(!is_streamable("/root/descendant::item[1]"));
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
    fn test_non_element_node_tests_not_streamable() {
        // Only elements are matched while streaming
        assert!(!is_streamable("//text()"));
        assert!(!is_streamable("//node()"));
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
