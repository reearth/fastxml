//! Boolean Functions.
//!
//! - `boolean(object)` - converts to boolean
//! - `not(boolean)` - negates boolean
//! - `true()` - returns true
//! - `false()` - returns false
//! - `lang(string)` - checks language

use crate::error::Result;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

/// `boolean(object)` - converts the argument to boolean.
pub fn fn_boolean(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "boolean".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(value.to_boolean()))
}

/// `not(boolean)` - negates the boolean value.
pub fn fn_not(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "not".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(!value.to_boolean()))
}

/// `true()` - returns true.
pub fn fn_true(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "true".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
    }
    Ok(XPathValue::Boolean(true))
}

/// `false()` - returns false.
pub fn fn_false(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "false".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
    }
    Ok(XPathValue::Boolean(false))
}

/// `lang(string)` - checks if the context node's language matches.
pub fn fn_lang(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "lang".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }

    let lang_arg = args
        .into_iter()
        .next()
        .unwrap()
        .to_string_value()
        .to_lowercase();

    // Search for xml:lang attribute in context node and ancestors
    let mut node = Some(ctx.node.clone());
    while let Some(n) = node {
        if let Some(lang_attr) = n
            .get_attribute("xml:lang")
            .or_else(|| n.get_attribute("lang"))
        {
            let lang_lower = lang_attr.to_lowercase();
            // Check if lang matches or is a sublanguage
            let matches =
                lang_lower == lang_arg || lang_lower.starts_with(&format!("{}-", lang_arg));
            return Ok(XPathValue::Boolean(matches));
        }
        node = n.get_parent();
    }

    Ok(XPathValue::Boolean(false))
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
    fn test_fn_boolean_true_string() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "boolean",
            vec![XPathValue::String("hello".to_string())],
            &ctx,
        )
        .unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_boolean_false_empty_string() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("boolean", vec![XPathValue::String("".to_string())], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_boolean_number() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("boolean", vec![XPathValue::Number(1.0)], &ctx).unwrap();
        assert!(result.to_boolean());

        let result = evaluate_function("boolean", vec![XPathValue::Number(0.0)], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_boolean_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("boolean", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_not_true() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("not", vec![XPathValue::Boolean(false)], &ctx).unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_not_false() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("not", vec![XPathValue::Boolean(true)], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_not_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("not", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_true() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("true", vec![], &ctx).unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_true_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("true", vec![XPathValue::Boolean(false)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_false() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("false", vec![], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_false_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("false", vec![XPathValue::Boolean(true)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_lang_match() {
        let doc = crate::parse("<root xml:lang=\"en\"><child/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let child = root.get_child_nodes().into_iter().next().unwrap();
        let ctx = create_context(&doc, &child);

        let result =
            evaluate_function("lang", vec![XPathValue::String("en".to_string())], &ctx).unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_lang_sublanguage() {
        let doc = crate::parse("<root xml:lang=\"en-US\"><child/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let child = root.get_child_nodes().into_iter().next().unwrap();
        let ctx = create_context(&doc, &child);

        let result =
            evaluate_function("lang", vec![XPathValue::String("en".to_string())], &ctx).unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_lang_no_match() {
        let doc = crate::parse("<root xml:lang=\"fr\"><child/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let child = root.get_child_nodes().into_iter().next().unwrap();
        let ctx = create_context(&doc, &child);

        let result =
            evaluate_function("lang", vec![XPathValue::String("en".to_string())], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_lang_no_attribute() {
        let doc = crate::parse("<root><child/></root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("lang", vec![XPathValue::String("en".to_string())], &ctx).unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_lang_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("lang", vec![], &ctx);
        assert!(result.is_err());
    }
}
