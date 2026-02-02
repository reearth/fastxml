//! Number Functions.
//!
//! - `number([object])` - converts to number
//! - `sum(node-set)` - sums node values
//! - `floor(number)` - rounds down
//! - `ceiling(number)` - rounds up
//! - `round(number)` - rounds to nearest integer

use crate::error::Result;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

/// `number([object])` - converts the argument to a number.
pub fn fn_number(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let value = if args.is_empty() {
        XPathValue::NodeSet(vec![ctx.node.clone()])
    } else if args.len() == 1 {
        args.into_iter().next().unwrap()
    } else {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "number".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
    };

    Ok(XPathValue::Number(value.to_number()))
}

/// `sum(node-set)` - returns the sum of the numeric values of nodes.
pub fn fn_sum(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "sum".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let nodes = args.into_iter().next().unwrap();
    match nodes {
        XPathValue::NodeSet(ns) => {
            let sum: f64 = ns
                .iter()
                .map(|n| {
                    n.get_content()
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .unwrap_or(f64::NAN)
                })
                .fold(0.0, |acc, v| if v.is_nan() { f64::NAN } else { acc + v });
            Ok(XPathValue::Number(sum))
        }
        _ => Err(XPathEvalError::InvalidArgumentType {
            function: "sum".to_string(),
            expected: "node-set".to_string(),
        }
        .into()),
    }
}

/// `floor(number)` - returns the largest integer not greater than the argument.
pub fn fn_floor(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "floor".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let value = args.into_iter().next().unwrap().to_number();
    Ok(XPathValue::Number(value.floor()))
}

/// `ceiling(number)` - returns the smallest integer not less than the argument.
pub fn fn_ceiling(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "ceiling".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let value = args.into_iter().next().unwrap().to_number();
    Ok(XPathValue::Number(value.ceil()))
}

/// `round(number)` - rounds to the nearest integer.
///
/// Note: XPath rounds .5 towards positive infinity (not banker's rounding).
pub fn fn_round(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "round".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let value = args.into_iter().next().unwrap().to_number();

    // Handle special cases: NaN, Infinity, and zero are returned as-is
    let result = if value.is_nan() || value.is_infinite() || value == 0.0 {
        value
    } else {
        // XPath rounds .5 towards positive infinity
        (value + 0.5).floor()
    };

    Ok(XPathValue::Number(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::XmlDocument;
    use crate::namespace::NamespaceResolver;
    use crate::xpath::functions::evaluate_function;

    fn create_test_document() -> XmlDocument {
        crate::parse(
            "<root><item id=\"1\">10</item><item id=\"2\">20</item><item id=\"3\">30</item></root>",
        )
        .unwrap()
    }

    fn create_context<'a>(
        doc: &'a XmlDocument,
        node: &crate::node::XmlNode,
    ) -> EvaluationContext<'a> {
        EvaluationContext::new(node.clone(), doc, NamespaceResolver::new())
    }

    #[test]
    fn test_fn_number_with_arg() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("number", vec![XPathValue::String("42.5".to_string())], &ctx)
                .unwrap();
        assert_eq!(result.to_number(), 42.5);
    }

    #[test]
    fn test_fn_number_no_arg() {
        let doc = crate::parse("<root>123</root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("number", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 123.0);
    }

    #[test]
    fn test_fn_number_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "number",
            vec![XPathValue::Number(1.0), XPathValue::Number(2.0)],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_sum() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let result = evaluate_function("sum", vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(result.to_number(), 60.0); // 10 + 20 + 30
    }

    #[test]
    fn test_fn_sum_empty() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("sum", vec![XPathValue::NodeSet(vec![])], &ctx).unwrap();
        assert_eq!(result.to_number(), 0.0);
    }

    #[test]
    fn test_fn_sum_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        // No arguments
        let result = evaluate_function("sum", vec![], &ctx);
        assert!(result.is_err());

        // Wrong type
        let result = evaluate_function("sum", vec![XPathValue::Number(42.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_floor() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("floor", vec![XPathValue::Number(2.9)], &ctx).unwrap();
        assert_eq!(result.to_number(), 2.0);

        let result = evaluate_function("floor", vec![XPathValue::Number(-2.1)], &ctx).unwrap();
        assert_eq!(result.to_number(), -3.0);
    }

    #[test]
    fn test_fn_floor_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("floor", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_ceiling() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("ceiling", vec![XPathValue::Number(2.1)], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);

        let result = evaluate_function("ceiling", vec![XPathValue::Number(-2.9)], &ctx).unwrap();
        assert_eq!(result.to_number(), -2.0);
    }

    #[test]
    fn test_fn_ceiling_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("ceiling", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_round_basic() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("round", vec![XPathValue::Number(1.5)], &ctx).unwrap();
        assert_eq!(result.to_number(), 2.0);

        let result = evaluate_function("round", vec![XPathValue::Number(2.5)], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_round_negative() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("round", vec![XPathValue::Number(-0.5)], &ctx).unwrap();
        assert_eq!(result.to_number(), 0.0);

        let result = evaluate_function("round", vec![XPathValue::Number(-1.5)], &ctx).unwrap();
        assert_eq!(result.to_number(), -1.0);
    }

    #[test]
    fn test_fn_round_special_values() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("round", vec![XPathValue::Number(f64::NAN)], &ctx).unwrap();
        assert!(result.to_number().is_nan());

        let result =
            evaluate_function("round", vec![XPathValue::Number(f64::INFINITY)], &ctx).unwrap();
        assert!(result.to_number().is_infinite());

        let result = evaluate_function("round", vec![XPathValue::Number(0.0)], &ctx).unwrap();
        assert_eq!(result.to_number(), 0.0);
    }

    #[test]
    fn test_fn_round_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("round", vec![], &ctx);
        assert!(result.is_err());
    }
}
