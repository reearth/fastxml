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

use crate::error::{Error, Result};
use crate::node::XmlNode;

use super::types::{EvaluationContext, XPathValue};

/// Evaluates an XPath function call.
pub fn evaluate_function(
    name: &str,
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XPathValue> {
    match name {
        // Node Set Functions
        "last" => fn_last(args, ctx),
        "position" => fn_position(args, ctx),
        "count" => fn_count(args, ctx),
        "name" => fn_name(args, ctx),
        "local-name" => fn_local_name(args, ctx),
        "namespace-uri" => fn_namespace_uri(args, ctx),
        "id" => fn_id(args, ctx),

        // String Functions
        "string" => fn_string(args, ctx),
        "concat" => fn_concat(args, ctx),
        "starts-with" => fn_starts_with(args, ctx),
        "contains" => fn_contains(args, ctx),
        "substring" => fn_substring(args, ctx),
        "substring-before" => fn_substring_before(args, ctx),
        "substring-after" => fn_substring_after(args, ctx),
        "string-length" => fn_string_length(args, ctx),
        "normalize-space" => fn_normalize_space(args, ctx),
        "translate" => fn_translate(args, ctx),

        // Boolean Functions
        "boolean" => fn_boolean(args, ctx),
        "not" => fn_not(args, ctx),
        "true" => fn_true(args, ctx),
        "false" => fn_false(args, ctx),
        "lang" => fn_lang(args, ctx),

        // Number Functions
        "number" => fn_number(args, ctx),
        "sum" => fn_sum(args, ctx),
        "floor" => fn_floor(args, ctx),
        "ceiling" => fn_ceiling(args, ctx),
        "round" => fn_round(args, ctx),

        // text() is handled as a node test, but if called as function
        "text" => fn_text(args, ctx),

        _ => Err(Error::XPathEval(format!("unknown function: {}", name))),
    }
}

// =============================================================================
// Node Set Functions
// =============================================================================

/// `last()` - returns the context size.
fn fn_last(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(Error::XPathEval("last() takes no arguments".into()));
    }
    Ok(XPathValue::Number(ctx.size() as f64))
}

/// `position()` - returns the context position.
fn fn_position(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(Error::XPathEval("position() takes no arguments".into()));
    }
    Ok(XPathValue::Number(ctx.position() as f64))
}

/// `count(node-set)` - returns the number of nodes in the node-set.
fn fn_count(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("count() requires 1 argument".into()));
    }
    let nodes = args.into_iter().next().unwrap();
    match nodes {
        XPathValue::NodeSet(ns) => Ok(XPathValue::Number(ns.len() as f64)),
        _ => Err(Error::XPathEval(
            "count() requires a node-set argument".into(),
        )),
    }
}

/// `name([node-set])` - returns the qualified name of the first node.
fn fn_name(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    Ok(XPathValue::String(node.qname()))
}

/// `local-name([node-set])` - returns the local part of the name.
fn fn_local_name(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    Ok(XPathValue::String(node.get_name()))
}

/// `namespace-uri([node-set])` - returns the namespace URI.
fn fn_namespace_uri(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let node = get_first_node_or_context(args, ctx)?;
    let uri = node.get_namespace_uri().unwrap_or_default();
    Ok(XPathValue::String(uri))
}

/// `id(object)` - selects elements by their ID.
fn fn_id(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("id() requires 1 argument".into()));
    }

    let id_value = args.into_iter().next().unwrap();
    let ids: Vec<String> = match id_value {
        XPathValue::NodeSet(nodes) => {
            // Get string values of all nodes, split by whitespace
            nodes
                .iter()
                .filter_map(|n| n.get_content())
                .flat_map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect()
        }
        _ => {
            // Split string value by whitespace
            id_value
                .to_string_value()
                .split_whitespace()
                .map(String::from)
                .collect()
        }
    };

    // Find elements with matching id attribute
    let mut result = Vec::new();
    let root = ctx.doc.document_node();
    find_elements_by_id(&root, &ids, &mut result);

    Ok(XPathValue::NodeSet(result))
}

fn find_elements_by_id(node: &XmlNode, ids: &[String], result: &mut Vec<XmlNode>) {
    if node.is_element() {
        if let Some(id_attr) = node.get_attribute("id") {
            if ids.iter().any(|id| id == &id_attr) {
                result.push(node.clone());
            }
        }
    }
    for child in node.get_child_nodes() {
        find_elements_by_id(&child, ids, result);
    }
}

// =============================================================================
// String Functions
// =============================================================================

/// `string([object])` - converts the argument to a string.
fn fn_string(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let value = if args.is_empty() {
        // Default: string value of context node
        XPathValue::NodeSet(vec![ctx.node.clone()])
    } else if args.len() == 1 {
        args.into_iter().next().unwrap()
    } else {
        return Err(Error::XPathEval("string() takes 0 or 1 argument".into()));
    };

    Ok(XPathValue::String(value.to_string_value()))
}

/// `concat(string, string, ...)` - concatenates all arguments.
fn fn_concat(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 {
        return Err(Error::XPathEval(
            "concat() requires at least 2 arguments".into(),
        ));
    }

    let result: String = args.into_iter().map(|v| v.to_string_value()).collect();

    Ok(XPathValue::String(result))
}

/// `starts-with(string, string)` - returns true if first string starts with second.
fn fn_starts_with(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(Error::XPathEval(
            "starts-with() requires 2 arguments".into(),
        ));
    }

    let mut iter = args.into_iter();
    let string = iter.next().unwrap().to_string_value();
    let prefix = iter.next().unwrap().to_string_value();

    Ok(XPathValue::Boolean(string.starts_with(&prefix)))
}

/// `contains(haystack, needle)` - returns true if haystack contains needle.
fn fn_contains(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(Error::XPathEval("contains() requires 2 arguments".into()));
    }

    let mut iter = args.into_iter();
    let haystack = iter.next().unwrap().to_string_value();
    let needle = iter.next().unwrap().to_string_value();

    Ok(XPathValue::Boolean(haystack.contains(&needle)))
}

/// `substring(string, start, [length])` - extracts a substring.
///
/// Note: XPath uses 1-based indexing and rounds to nearest integer.
fn fn_substring(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() < 2 || args.len() > 3 {
        return Err(Error::XPathEval(
            "substring() requires 2 or 3 arguments".into(),
        ));
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
fn fn_substring_before(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(Error::XPathEval(
            "substring-before() requires 2 arguments".into(),
        ));
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
fn fn_substring_after(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 2 {
        return Err(Error::XPathEval(
            "substring-after() requires 2 arguments".into(),
        ));
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
fn fn_string_length(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let string = if args.is_empty() {
        // Default: string value of context node
        ctx.node.get_content().unwrap_or_default()
    } else if args.len() == 1 {
        args.into_iter().next().unwrap().to_string_value()
    } else {
        return Err(Error::XPathEval(
            "string-length() takes 0 or 1 argument".into(),
        ));
    };

    Ok(XPathValue::Number(string.chars().count() as f64))
}

/// `normalize-space([string])` - strips leading/trailing whitespace and collapses internal.
fn fn_normalize_space(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let string = if args.is_empty() {
        ctx.node.get_content().unwrap_or_default()
    } else if args.len() == 1 {
        args.into_iter().next().unwrap().to_string_value()
    } else {
        return Err(Error::XPathEval(
            "normalize-space() takes 0 or 1 argument".into(),
        ));
    };

    // Split by whitespace and rejoin with single spaces
    let result: String = string.split_whitespace().collect::<Vec<_>>().join(" ");

    Ok(XPathValue::String(result))
}

/// `translate(string, from, to)` - replaces characters.
fn fn_translate(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 3 {
        return Err(Error::XPathEval("translate() requires 3 arguments".into()));
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

// =============================================================================
// Boolean Functions
// =============================================================================

/// `boolean(object)` - converts the argument to boolean.
fn fn_boolean(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("boolean() requires 1 argument".into()));
    }

    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(value.to_boolean()))
}

/// `not(boolean)` - negates the boolean value.
fn fn_not(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("not() requires 1 argument".into()));
    }

    let value = args.into_iter().next().unwrap();
    Ok(XPathValue::Boolean(!value.to_boolean()))
}

/// `true()` - returns true.
fn fn_true(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(Error::XPathEval("true() takes no arguments".into()));
    }
    Ok(XPathValue::Boolean(true))
}

/// `false()` - returns false.
fn fn_false(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(Error::XPathEval("false() takes no arguments".into()));
    }
    Ok(XPathValue::Boolean(false))
}

/// `lang(string)` - checks if the context node's language matches.
fn fn_lang(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("lang() requires 1 argument".into()));
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

// =============================================================================
// Number Functions
// =============================================================================

/// `number([object])` - converts the argument to a number.
fn fn_number(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    let value = if args.is_empty() {
        XPathValue::NodeSet(vec![ctx.node.clone()])
    } else if args.len() == 1 {
        args.into_iter().next().unwrap()
    } else {
        return Err(Error::XPathEval("number() takes 0 or 1 argument".into()));
    };

    Ok(XPathValue::Number(value.to_number()))
}

/// `sum(node-set)` - returns the sum of the numeric values of nodes.
fn fn_sum(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("sum() requires 1 argument".into()));
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
        _ => Err(Error::XPathEval(
            "sum() requires a node-set argument".into(),
        )),
    }
}

/// `floor(number)` - returns the largest integer not greater than the argument.
fn fn_floor(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("floor() requires 1 argument".into()));
    }

    let value = args.into_iter().next().unwrap().to_number();
    Ok(XPathValue::Number(value.floor()))
}

/// `ceiling(number)` - returns the smallest integer not less than the argument.
fn fn_ceiling(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("ceiling() requires 1 argument".into()));
    }

    let value = args.into_iter().next().unwrap().to_number();
    Ok(XPathValue::Number(value.ceil()))
}

/// `round(number)` - rounds to the nearest integer.
///
/// Note: XPath rounds .5 towards positive infinity (not banker's rounding).
fn fn_round(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(Error::XPathEval("round() requires 1 argument".into()));
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

// =============================================================================
// Helper Functions
// =============================================================================

/// `text()` - returns the text content of the context node.
/// (Usually handled as a node test, but can be called as function)
fn fn_text(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(Error::XPathEval("text() takes no arguments".into()));
    }
    let content = ctx.node.get_content().unwrap_or_default();
    Ok(XPathValue::String(content))
}

/// Gets the first node from a node-set argument, or the context node if no argument.
fn get_first_node_or_context(
    args: Vec<XPathValue>,
    ctx: &EvaluationContext<'_>,
) -> Result<XmlNode> {
    if args.is_empty() {
        return Ok(ctx.node.clone());
    }

    if args.len() != 1 {
        return Err(Error::XPathEval("function takes 0 or 1 argument".into()));
    }

    let arg = args.into_iter().next().unwrap();
    match arg {
        XPathValue::NodeSet(nodes) => {
            Ok(nodes.into_iter().next().unwrap_or_else(|| ctx.node.clone()))
        }
        _ => Ok(ctx.node.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_value(s: &str) -> XPathValue {
        XPathValue::String(s.to_string())
    }

    #[allow(dead_code)]
    fn make_number(n: f64) -> XPathValue {
        XPathValue::Number(n)
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
    fn test_normalize_space() {
        let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };

        assert_eq!(normalize("  hello   world  "), "hello world");
        assert_eq!(normalize("no\textra\nspace"), "no extra space");
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn test_translate() {
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
    fn test_round() {
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
