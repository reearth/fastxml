//! XPath operators.
//!
//! This module implements the XPath 1.0 operators:
//!
//! ## Comparison Operators
//! - `=` - equality
//! - `!=` - inequality
//! - `<` - less than
//! - `<=` - less than or equal
//! - `>` - greater than
//! - `>=` - greater than or equal
//!
//! ## Arithmetic Operators
//! - `+` - addition
//! - `-` - subtraction (binary and unary)
//! - `*` - multiplication
//! - `div` - division
//! - `mod` - modulo
//!
//! ## Boolean Operators
//! - `and` - logical and
//! - `or` - logical or
//!
//! ## Union Operator
//! - `|` - node-set union

use std::collections::HashSet;

use crate::node::XmlNode;

use super::parser::ComparisonOp;
use super::types::XPathValue;

/// Evaluates a comparison operation between two XPath values.
///
/// XPath comparison semantics are complex:
/// - If both are node-sets, compare string values pairwise
/// - If one is a node-set, compare each node's string value against the other value
/// - If either is a number, convert both to numbers
/// - If either is a boolean, convert both to booleans
/// - Otherwise, compare as strings
pub fn compare(left: &XPathValue, op: &ComparisonOp, right: &XPathValue) -> bool {
    match (left, right) {
        // Node-set vs Node-set
        (XPathValue::NodeSet(left_nodes), XPathValue::NodeSet(right_nodes)) => {
            compare_nodesets(left_nodes, right_nodes, op)
        }

        // Node-set vs other
        (XPathValue::NodeSet(nodes), other) | (other, XPathValue::NodeSet(nodes)) => {
            let is_left_nodeset = matches!(left, XPathValue::NodeSet(_));
            compare_nodeset_to_value(nodes, other, op, is_left_nodeset)
        }

        // Boolean comparison (if either is boolean, use boolean comparison)
        (XPathValue::Boolean(_), _) | (_, XPathValue::Boolean(_)) => {
            compare_booleans(left.to_boolean(), right.to_boolean(), op)
        }

        // Number comparison (if either is number, use number comparison)
        (XPathValue::Number(_), _) | (_, XPathValue::Number(_)) => {
            compare_numbers(left.to_number(), right.to_number(), op)
        }

        // String comparison
        (XPathValue::String(l), XPathValue::String(r)) => {
            compare_strings(l, r, op)
        }
    }
}

/// Compares two node-sets.
fn compare_nodesets(left: &[XmlNode], right: &[XmlNode], op: &ComparisonOp) -> bool {
    for l_node in left {
        let l_str = l_node.get_content().unwrap_or_default();
        for r_node in right {
            let r_str = r_node.get_content().unwrap_or_default();
            if compare_strings(&l_str, &r_str, op) {
                return true;
            }
        }
    }
    false
}

/// Compares a node-set to another value.
fn compare_nodeset_to_value(
    nodes: &[XmlNode],
    other: &XPathValue,
    op: &ComparisonOp,
    is_left_nodeset: bool,
) -> bool {
    match other {
        XPathValue::Number(n) => {
            for node in nodes {
                let node_num = node
                    .get_content()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .unwrap_or(f64::NAN);

                let result = if is_left_nodeset {
                    compare_numbers(node_num, *n, op)
                } else {
                    compare_numbers(*n, node_num, op)
                };

                if result {
                    return true;
                }
            }
            false
        }
        XPathValue::Boolean(b) => {
            let node_bool = !nodes.is_empty();
            if is_left_nodeset {
                compare_booleans(node_bool, *b, op)
            } else {
                compare_booleans(*b, node_bool, op)
            }
        }
        XPathValue::String(s) => {
            for node in nodes {
                let node_str = node.get_content().unwrap_or_default();
                let result = if is_left_nodeset {
                    compare_strings(&node_str, s, op)
                } else {
                    compare_strings(s, &node_str, op)
                };
                if result {
                    return true;
                }
            }
            false
        }
        XPathValue::NodeSet(_) => unreachable!(),
    }
}

/// Compares two boolean values.
fn compare_booleans(left: bool, right: bool, op: &ComparisonOp) -> bool {
    match op {
        ComparisonOp::Equal => left == right,
        ComparisonOp::NotEqual => left != right,
        // For ordering, true > false
        ComparisonOp::LessThan => !left && right,
        ComparisonOp::LessOrEqual => !left || right,
        ComparisonOp::GreaterThan => left && !right,
        ComparisonOp::GreaterOrEqual => left || !right,
    }
}

/// Compares two numbers.
fn compare_numbers(left: f64, right: f64, op: &ComparisonOp) -> bool {
    match op {
        ComparisonOp::Equal => left == right,
        ComparisonOp::NotEqual => left != right,
        ComparisonOp::LessThan => left < right,
        ComparisonOp::LessOrEqual => left <= right,
        ComparisonOp::GreaterThan => left > right,
        ComparisonOp::GreaterOrEqual => left >= right,
    }
}

/// Compares two strings.
///
/// For relational comparisons, attempts numeric comparison first if both parse as numbers.
fn compare_strings(left: &str, right: &str, op: &ComparisonOp) -> bool {
    match op {
        ComparisonOp::Equal => left == right,
        ComparisonOp::NotEqual => left != right,
        ComparisonOp::LessThan
        | ComparisonOp::LessOrEqual
        | ComparisonOp::GreaterThan
        | ComparisonOp::GreaterOrEqual => {
            // Try numeric comparison first
            if let (Ok(l), Ok(r)) = (left.parse::<f64>(), right.parse::<f64>()) {
                compare_numbers(l, r, op)
            } else {
                // Fall back to string comparison
                match op {
                    ComparisonOp::LessThan => left < right,
                    ComparisonOp::LessOrEqual => left <= right,
                    ComparisonOp::GreaterThan => left > right,
                    ComparisonOp::GreaterOrEqual => left >= right,
                    _ => unreachable!(),
                }
            }
        }
    }
}

// =============================================================================
// Arithmetic Operators
// =============================================================================

/// Arithmetic operator types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOp {
    /// Addition (+)
    Add,
    /// Subtraction (-)
    Subtract,
    /// Multiplication (*)
    Multiply,
    /// Division (div)
    Divide,
    /// Modulo (mod)
    Modulo,
}

/// Evaluates an arithmetic operation.
///
/// Both operands are converted to numbers before the operation.
pub fn arithmetic(left: &XPathValue, op: ArithmeticOp, right: &XPathValue) -> XPathValue {
    let l = left.to_number();
    let r = right.to_number();

    let result = match op {
        ArithmeticOp::Add => l + r,
        ArithmeticOp::Subtract => l - r,
        ArithmeticOp::Multiply => l * r,
        ArithmeticOp::Divide => l / r,
        ArithmeticOp::Modulo => l % r,
    };

    XPathValue::Number(result)
}

/// Negates a numeric value.
pub fn negate(value: &XPathValue) -> XPathValue {
    XPathValue::Number(-value.to_number())
}

// =============================================================================
// Boolean Operators
// =============================================================================

/// Evaluates logical AND.
///
/// Both operands are converted to boolean.
pub fn logical_and(left: &XPathValue, right: &XPathValue) -> XPathValue {
    XPathValue::Boolean(left.to_boolean() && right.to_boolean())
}

/// Evaluates logical OR.
///
/// Both operands are converted to boolean.
pub fn logical_or(left: &XPathValue, right: &XPathValue) -> XPathValue {
    XPathValue::Boolean(left.to_boolean() || right.to_boolean())
}

/// Evaluates logical NOT.
pub fn logical_not(value: &XPathValue) -> XPathValue {
    XPathValue::Boolean(!value.to_boolean())
}

// =============================================================================
// Union Operator
// =============================================================================

/// Computes the union of two node-sets.
///
/// The result contains all nodes from both sets, without duplicates,
/// in document order.
pub fn union(left: XPathValue, right: XPathValue) -> XPathValue {
    let left_nodes = left.into_nodes();
    let right_nodes = right.into_nodes();

    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for node in left_nodes.into_iter().chain(right_nodes.into_iter()) {
        if seen.insert(node.id()) {
            result.push(node);
        }
    }

    // Note: For full XPath compliance, we should sort by document order here.
    // This implementation preserves left-to-right order.

    XPathValue::NodeSet(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_val(s: &str) -> XPathValue {
        XPathValue::String(s.to_string())
    }

    fn num_val(n: f64) -> XPathValue {
        XPathValue::Number(n)
    }

    fn bool_val(b: bool) -> XPathValue {
        XPathValue::Boolean(b)
    }

    #[test]
    fn test_compare_strings_equal() {
        assert!(compare(&str_val("hello"), &ComparisonOp::Equal, &str_val("hello")));
        assert!(!compare(&str_val("hello"), &ComparisonOp::Equal, &str_val("world")));
        assert!(compare(&str_val("hello"), &ComparisonOp::NotEqual, &str_val("world")));
    }

    #[test]
    fn test_compare_numbers() {
        assert!(compare(&num_val(1.0), &ComparisonOp::LessThan, &num_val(2.0)));
        assert!(compare(&num_val(2.0), &ComparisonOp::GreaterThan, &num_val(1.0)));
        assert!(compare(&num_val(1.0), &ComparisonOp::LessOrEqual, &num_val(1.0)));
        assert!(compare(&num_val(1.0), &ComparisonOp::GreaterOrEqual, &num_val(1.0)));
    }

    #[test]
    fn test_compare_booleans() {
        assert!(compare(&bool_val(true), &ComparisonOp::Equal, &bool_val(true)));
        assert!(compare(&bool_val(false), &ComparisonOp::Equal, &bool_val(false)));
        assert!(compare(&bool_val(true), &ComparisonOp::NotEqual, &bool_val(false)));
    }

    #[test]
    fn test_compare_mixed_types() {
        // Number comparison (number + string)
        assert!(compare(&num_val(42.0), &ComparisonOp::Equal, &str_val("42")));
        assert!(compare(&str_val("42"), &ComparisonOp::Equal, &num_val(42.0)));

        // Boolean comparison (boolean + number)
        assert!(compare(&bool_val(true), &ComparisonOp::Equal, &num_val(1.0)));
        assert!(compare(&bool_val(false), &ComparisonOp::Equal, &num_val(0.0)));
    }

    #[test]
    fn test_arithmetic_add() {
        let result = arithmetic(&num_val(1.0), ArithmeticOp::Add, &num_val(2.0));
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_arithmetic_subtract() {
        let result = arithmetic(&num_val(5.0), ArithmeticOp::Subtract, &num_val(3.0));
        assert_eq!(result.to_number(), 2.0);
    }

    #[test]
    fn test_arithmetic_multiply() {
        let result = arithmetic(&num_val(3.0), ArithmeticOp::Multiply, &num_val(4.0));
        assert_eq!(result.to_number(), 12.0);
    }

    #[test]
    fn test_arithmetic_divide() {
        let result = arithmetic(&num_val(10.0), ArithmeticOp::Divide, &num_val(2.0));
        assert_eq!(result.to_number(), 5.0);

        // Division by zero
        let div_zero = arithmetic(&num_val(1.0), ArithmeticOp::Divide, &num_val(0.0));
        assert!(div_zero.to_number().is_infinite());
    }

    #[test]
    fn test_arithmetic_modulo() {
        let result = arithmetic(&num_val(7.0), ArithmeticOp::Modulo, &num_val(3.0));
        assert_eq!(result.to_number(), 1.0);
    }

    #[test]
    fn test_negate() {
        let result = negate(&num_val(5.0));
        assert_eq!(result.to_number(), -5.0);

        let result2 = negate(&num_val(-3.0));
        assert_eq!(result2.to_number(), 3.0);
    }

    #[test]
    fn test_logical_and() {
        assert!(logical_and(&bool_val(true), &bool_val(true)).to_boolean());
        assert!(!logical_and(&bool_val(true), &bool_val(false)).to_boolean());
        assert!(!logical_and(&bool_val(false), &bool_val(true)).to_boolean());
        assert!(!logical_and(&bool_val(false), &bool_val(false)).to_boolean());
    }

    #[test]
    fn test_logical_or() {
        assert!(logical_or(&bool_val(true), &bool_val(true)).to_boolean());
        assert!(logical_or(&bool_val(true), &bool_val(false)).to_boolean());
        assert!(logical_or(&bool_val(false), &bool_val(true)).to_boolean());
        assert!(!logical_or(&bool_val(false), &bool_val(false)).to_boolean());
    }

    #[test]
    fn test_logical_not() {
        assert!(!logical_not(&bool_val(true)).to_boolean());
        assert!(logical_not(&bool_val(false)).to_boolean());
    }

    #[test]
    fn test_string_numeric_comparison() {
        // Numeric strings should compare numerically
        assert!(compare(&str_val("2"), &ComparisonOp::LessThan, &str_val("10")));
        // Non-numeric strings compare lexicographically
        assert!(!compare(&str_val("2"), &ComparisonOp::LessThan, &str_val("10a")));
    }
}
