//! String Functions.
//!
//! - `string([object])` - converts to string
//! - `concat(string, string, ...)` - concatenates strings
//! - `starts-with(string, string)` - tests string prefix
//! - `contains(string, string)` - tests if string contains substring
//! - `substring(string, number, [number])` - extracts substring
//! - `substring-before(string, string)` - returns substring before match
//! - `substring-after(string, string)` - returns substring after match
//! - `string-length([string])` - returns string length
//! - `normalize-space([string])` - normalizes whitespace
//! - `translate(string, string, string)` - character translation

use crate::error::Result;
use crate::xpath::error::XPathEvalError;
use crate::xpath::types::{EvaluationContext, XPathValue};

/// `string([object])` - converts the argument to a string.
pub fn fn_string(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let value = if args.is_empty() {
        // Default: string value of context node
        XPathValue::NodeSet(vec![ctx.node.clone()])
    } else if args.len() == 1 {
        args.into_iter().next().unwrap()
    } else {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "string".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
    };

    Ok(XPathValue::String(value.to_string_value()))
}

/// `concat(string, string, ...)` - concatenates all arguments.
pub fn fn_concat(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "concat".to_string(),
            expected: "at least 2".to_string(),
            found: args.len(),
        }
        .into());
    }

    let result: String = args.into_iter().map(|v| v.to_string_value()).collect();

    Ok(XPathValue::String(result))
}

/// `starts-with(string, string)` - returns true if first string starts with second.
pub fn fn_starts_with(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "starts-with".to_string(),
            expected: "2".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let prefix = iter.next().unwrap().to_string_value();

    Ok(XPathValue::Boolean(string.starts_with(&prefix)))
}

/// `contains(haystack, needle)` - returns true if haystack contains needle.
pub fn fn_contains(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "contains".to_string(),
            expected: "2".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let haystack = iter.next().unwrap().to_string_value();
    let needle = iter.next().unwrap().to_string_value();

    Ok(XPathValue::Boolean(haystack.contains(&needle)))
}

/// `substring(string, start, [length])` - extracts a substring.
///
/// Note: XPath uses 1-based indexing and rounds to nearest integer.
pub fn fn_substring(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 || args.len() > 3 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "substring".to_string(),
            expected: "2 or 3".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let start = iter.next().unwrap().to_number();
    let length = iter.next().map(|v| v.to_number());

    // Handle NaN cases
    if start.is_nan() {
        return Ok(XPathValue::String(String::new()));
    }

    // XPath substring uses 1-based indexing with round()
    let start_idx = (start.round() as i64 - 1).max(0) as usize;

    let chars: Vec<char> = string.chars().collect();

    let result = if let Some(len) = length {
        if len.is_nan() || len <= 0.0 {
            String::new()
        } else {
            // Handle case where start is negative
            let actual_start = (start.round() as i64 - 1).max(0) as usize;
            let end_idx = ((start.round() + len.round()) as i64 - 1).max(0) as usize;
            let actual_len = end_idx.saturating_sub(actual_start);

            chars.iter().skip(actual_start).take(actual_len).collect()
        }
    } else {
        chars.iter().skip(start_idx).collect()
    };

    Ok(XPathValue::String(result))
}

/// `substring-before(string, string)` - returns substring before first occurrence.
pub fn fn_substring_before(
    args: Vec<XPathValue>,
    _ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "substring-before".to_string(),
            expected: "2".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let search = iter.next().unwrap().to_string_value();

    let result = if search.is_empty() {
        String::new()
    } else if let Some(idx) = string.find(&search) {
        string[..idx].to_string()
    } else {
        String::new()
    };

    Ok(XPathValue::String(result))
}

/// `substring-after(string, string)` - returns substring after first occurrence.
pub fn fn_substring_after(
    args: Vec<XPathValue>,
    _ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "substring-after".to_string(),
            expected: "2".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let search = iter.next().unwrap().to_string_value();

    let result = if search.is_empty() {
        string
    } else if let Some(idx) = string.find(&search) {
        string[idx + search.len()..].to_string()
    } else {
        String::new()
    };

    Ok(XPathValue::String(result))
}

/// `string-length([string])` - returns the length of the string.
pub fn fn_string_length(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let string = if args.is_empty() {
        // Default: string value of context node
        ctx.node.get_content().unwrap_or_default()
    } else if args.len() == 1 {
        args.into_iter().next().unwrap().to_string_value()
    } else {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "string-length".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
    };

    Ok(XPathValue::Number(string.chars().count() as f64))
}

/// `normalize-space([string])` - strips leading/trailing whitespace and collapses internal.
pub fn fn_normalize_space(
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    let string = if args.is_empty() {
        ctx.node.get_content().unwrap_or_default()
    } else if args.len() == 1 {
        args.into_iter().next().unwrap().to_string_value()
    } else {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "normalize-space".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
    };

    // Split by whitespace and rejoin with single spaces
    let result: String = string.split_whitespace().collect::<Vec<_>>().join(" ");

    Ok(XPathValue::String(result))
}

/// `translate(string, from, to)` - replaces characters.
pub fn fn_translate(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 3 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "translate".to_string(),
            expected: "3".to_string(),
            found: args.len(),
        }
        .into());
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let from = iter.next().unwrap().to_string_value();
    let to = iter.next().unwrap().to_string_value();

    let from_chars: Vec<char> = from.chars().collect();
    let to_chars: Vec<char> = to.chars().collect();

    let result: String = string
        .chars()
        .filter_map(|c| {
            if let Some(idx) = from_chars.iter().position(|&fc| fc == c) {
                // Character found in 'from' string
                if idx < to_chars.len() {
                    Some(to_chars[idx])
                } else {
                    // No corresponding char in 'to', remove it
                    None
                }
            } else {
                // Character not in 'from', keep it
                Some(c)
            }
        })
        .collect();

    Ok(XPathValue::String(result))
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
    fn test_fn_string_with_arg() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("string", vec![XPathValue::Number(42.0)], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "42");
    }

    #[test]
    fn test_fn_string_no_arg() {
        let doc = crate::parse("<root>hello</root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("string", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "hello");
    }

    #[test]
    fn test_fn_string_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "string",
            vec![XPathValue::Number(1.0), XPathValue::Number(2.0)],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_concat() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "concat",
            vec![
                XPathValue::String("Hello".to_string()),
                XPathValue::String(" ".to_string()),
                XPathValue::String("World".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "Hello World");
    }

    #[test]
    fn test_fn_concat_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "concat",
            vec![XPathValue::String("only one".to_string())],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_starts_with_true() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "starts-with",
            vec![
                XPathValue::String("Hello World".to_string()),
                XPathValue::String("Hello".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_starts_with_false() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "starts-with",
            vec![
                XPathValue::String("Hello World".to_string()),
                XPathValue::String("World".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_contains_true() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "contains",
            vec![
                XPathValue::String("Hello World".to_string()),
                XPathValue::String("o W".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert!(result.to_boolean());
    }

    #[test]
    fn test_fn_substring_two_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring",
            vec![
                XPathValue::String("12345".to_string()),
                XPathValue::Number(2.0),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "2345");
    }

    #[test]
    fn test_fn_substring_three_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring",
            vec![
                XPathValue::String("12345".to_string()),
                XPathValue::Number(2.0),
                XPathValue::Number(3.0),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "234");
    }

    #[test]
    fn test_fn_substring_before() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-before",
            vec![
                XPathValue::String("1999/04/01".to_string()),
                XPathValue::String("/".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "1999");
    }

    #[test]
    fn test_fn_substring_after() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-after",
            vec![
                XPathValue::String("1999/04/01".to_string()),
                XPathValue::String("/".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "04/01");
    }

    #[test]
    fn test_fn_string_length_with_arg() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "string-length",
            vec![XPathValue::String("hello".to_string())],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn test_fn_normalize_space_with_arg() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "normalize-space",
            vec![XPathValue::String("  hello   world  ".to_string())],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_fn_translate_basic() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "translate",
            vec![
                XPathValue::String("bar".to_string()),
                XPathValue::String("abc".to_string()),
                XPathValue::String("ABC".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "BAr");
    }

    #[test]
    fn test_fn_translate_remove_chars() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "translate",
            vec![
                XPathValue::String("--aaa--".to_string()),
                XPathValue::String("abc-".to_string()),
                XPathValue::String("ABC".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "AAA");
    }

    #[test]
    fn test_fn_starts_with_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "starts-with",
            vec![XPathValue::String("test".to_string())],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_contains_false() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "contains",
            vec![
                XPathValue::String("Hello World".to_string()),
                XPathValue::String("xyz".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert!(!result.to_boolean());
    }

    #[test]
    fn test_fn_contains_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("contains", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_substring_nan_start() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring",
            vec![
                XPathValue::String("12345".to_string()),
                XPathValue::Number(f64::NAN),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_substring_nan_length() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring",
            vec![
                XPathValue::String("12345".to_string()),
                XPathValue::Number(1.0),
                XPathValue::Number(f64::NAN),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_substring_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring",
            vec![XPathValue::String("test".to_string())],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_substring_before_not_found() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-before",
            vec![
                XPathValue::String("hello".to_string()),
                XPathValue::String("xyz".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_substring_before_empty_search() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-before",
            vec![
                XPathValue::String("hello".to_string()),
                XPathValue::String("".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_substring_before_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("substring-before", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_substring_after_not_found() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-after",
            vec![
                XPathValue::String("hello".to_string()),
                XPathValue::String("xyz".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_substring_after_empty_search() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "substring-after",
            vec![
                XPathValue::String("hello".to_string()),
                XPathValue::String("".to_string()),
            ],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_string_value(), "hello");
    }

    #[test]
    fn test_fn_substring_after_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("substring-after", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_string_length_no_arg() {
        let doc = crate::parse("<root>hello</root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("string-length", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn test_fn_string_length_unicode() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "string-length",
            vec![XPathValue::String("日本語".to_string())],
            &ctx,
        )
        .unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_string_length_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "string-length",
            vec![
                XPathValue::String("a".to_string()),
                XPathValue::String("b".to_string()),
            ],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_normalize_space_no_arg() {
        let doc = crate::parse("<root>  hello   world  </root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("normalize-space", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_fn_normalize_space_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "normalize-space",
            vec![
                XPathValue::String("a".to_string()),
                XPathValue::String("b".to_string()),
            ],
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_translate_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function(
            "translate",
            vec![
                XPathValue::String("test".to_string()),
                XPathValue::String("abc".to_string()),
            ],
            &ctx,
        );
        assert!(result.is_err());
    }
}
