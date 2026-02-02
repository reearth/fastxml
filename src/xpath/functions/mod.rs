//! XPath function library.
//!
//! This module implements the core XPath 1.0 function library:
//!
//! ## Node Set Functions
//! - `last()` - returns the context size
//! - `position()` - returns the context position
//! - `count(node-set)` - returns the number of nodes
//! - `name([node-set])` - returns the expanded-name
//! - `local-name([node-set])` - returns the local part of the name
//! - `namespace-uri([node-set])` - returns the namespace URI
//!
//! ## String Functions
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
//!
//! ## Boolean Functions
//! - `boolean(object)` - converts to boolean
//! - `not(boolean)` - negates boolean
//! - `true()` - returns true
//! - `false()` - returns false
//!
//! ## Number Functions
//! - `number([object])` - converts to number
//! - `sum(node-set)` - sums node values
//! - `floor(number)` - rounds down
//! - `ceiling(number)` - rounds up
//! - `round(number)` - rounds to nearest integer

mod boolean;
mod helpers;
mod nodeset;
mod number;
mod string;

use crate::error::Result;
use crate::xpath::error::XPathEvalError;

use super::types::{EvaluationContext, XPathValue};

// Re-export for tests
pub use boolean::{fn_boolean, fn_false, fn_lang, fn_not, fn_true};
pub use helpers::{fn_text, get_first_node_or_context};
pub use nodeset::{
    fn_count, fn_id, fn_last, fn_local_name, fn_name, fn_namespace_uri, fn_position,
};
pub use number::{fn_ceiling, fn_floor, fn_number, fn_round, fn_sum};
pub use string::{
    fn_concat, fn_contains, fn_normalize_space, fn_starts_with, fn_string, fn_string_length,
    fn_substring, fn_substring_after, fn_substring_before, fn_translate,
};

/// Evaluates an XPath function call.
pub fn evaluate_function(
    name: &str,
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    match name {
        // Node Set Functions
        "last" => nodeset::fn_last(args, ctx),
        "position" => nodeset::fn_position(args, ctx),
        "count" => nodeset::fn_count(args, ctx),
        "name" => nodeset::fn_name(args, ctx),
        "local-name" => nodeset::fn_local_name(args, ctx),
        "namespace-uri" => nodeset::fn_namespace_uri(args, ctx),
        "id" => nodeset::fn_id(args, ctx),

        // String Functions
        "string" => string::fn_string(args, ctx),
        "concat" => string::fn_concat(args, ctx),
        "starts-with" => string::fn_starts_with(args, ctx),
        "contains" => string::fn_contains(args, ctx),
        "substring" => string::fn_substring(args, ctx),
        "substring-before" => string::fn_substring_before(args, ctx),
        "substring-after" => string::fn_substring_after(args, ctx),
        "string-length" => string::fn_string_length(args, ctx),
        "normalize-space" => string::fn_normalize_space(args, ctx),
        "translate" => string::fn_translate(args, ctx),

        // Boolean Functions
        "boolean" => boolean::fn_boolean(args, ctx),
        "not" => boolean::fn_not(args, ctx),
        "true" => boolean::fn_true(args, ctx),
        "false" => boolean::fn_false(args, ctx),
        "lang" => boolean::fn_lang(args, ctx),

        // Number Functions
        "number" => number::fn_number(args, ctx),
        "sum" => number::fn_sum(args, ctx),
        "floor" => number::fn_floor(args, ctx),
        "ceiling" => number::fn_ceiling(args, ctx),
        "round" => number::fn_round(args, ctx),

        // text() is handled as a node test, but if called as function
        "text" => helpers::fn_text(args, ctx),

        _ => Err(XPathEvalError::UnknownFunction {
            name: name.to_string(),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::XmlDocument;
    use crate::namespace::NamespaceResolver;

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
    fn test_unknown_function() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("unknown-function", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_substring() {
        // Test basic substring
        assert_eq!(extract_substring("12345", 2.0, None), "2345");
        assert_eq!(extract_substring("12345", 2.0, Some(3.0)), "234");
        assert_eq!(extract_substring("12345", 0.0, Some(3.0)), "12");
        assert_eq!(extract_substring("12345", -1.0, Some(5.0)), "123");
    }

    fn extract_substring(s: &str, start: f64, len: Option<f64>) -> String {
        let chars: Vec<char> = s.chars().collect();
        let start_idx = (start.round() as i64 - 1).max(0) as usize;

        if let Some(length) = len {
            if length.is_nan() || length <= 0.0 {
                return String::new();
            }
            let actual_start = (start.round() as i64 - 1).max(0) as usize;
            let end_idx = ((start.round() + length.round()) as i64 - 1).max(0) as usize;
            let actual_len = end_idx.saturating_sub(actual_start);
            chars.iter().skip(actual_start).take(actual_len).collect()
        } else {
            chars.iter().skip(start_idx).collect()
        }
    }

    #[test]
    fn test_normalize_space_helper() {
        let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };

        assert_eq!(normalize("  hello   world  "), "hello world");
        assert_eq!(normalize("no\textra\nspace"), "no extra space");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn test_translate_helper() {
        let translate = |s: &str, from: &str, to: &str| -> String {
            let from_chars: Vec<char> = from.chars().collect();
            let to_chars: Vec<char> = to.chars().collect();

            s.chars()
                .filter_map(|c| {
                    if let Some(idx) = from_chars.iter().position(|&fc| fc == c) {
                        if idx < to_chars.len() {
                            Some(to_chars[idx])
                        } else {
                            None
                        }
                    } else {
                        Some(c)
                    }
                })
                .collect()
        };

        assert_eq!(translate("bar", "abc", "ABC"), "BAr");
        assert_eq!(translate("--aaa--", "abc-", "ABC"), "AAA");
    }

    #[test]
    fn test_round_helper() {
        // XPath rounding (0.5 rounds up)
        let xpath_round = |n: f64| -> f64 {
            if n.is_nan() || n.is_infinite() || n == 0.0 {
                n
            } else {
                (n + 0.5).floor()
            }
        };

        assert_eq!(xpath_round(1.5), 2.0);
        assert_eq!(xpath_round(2.5), 3.0);
        assert_eq!(xpath_round(-0.5), 0.0);
        assert_eq!(xpath_round(-1.5), -1.0);
    }
}
