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

use crate::error::Result;
use crate::node::XmlNode;
use crate::xpath::error::XPathEvalError;

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

        _ => Err(XPathEvalError::UnknownFunction {
            name: name.to_string(),
        }
        .into()),
    }
}

// =============================================================================
// Node Set Functions
// =============================================================================

/// `last()` - returns the context size.
fn fn_last(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "last".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
    }
    Ok(XPathValue::Number(ctx.size() as f64))
}

/// `position()` - returns the context position.
fn fn_position(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "position".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
    }
    Ok(XPathValue::Number(ctx.position() as f64))
}

/// `count(node-set)` - returns the number of nodes in the node-set.
fn fn_count(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if args.len() != 1 {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "count".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
    }
    let nodes = args.into_iter().next().unwrap();
    match nodes {
        XPathValue::NodeSet(ns) => Ok(XPathValue::Number(ns.len() as f64)),
        _ => Err(XPathEvalError::InvalidArgumentType {
            function: "count".to_string(),
            expected: "node-set".to_string(),
        }
        .into()),
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
        return Err(XPathEvalError::WrongArgumentCount {
            function: "id".to_string(),
            expected: "1".to_string(),
            found: args.len(),
        }
        .into());
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
fn fn_concat(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_starts_with(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_contains(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_substring(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_substring_before(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_substring_after(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_string_length(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_normalize_space(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_translate(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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

// =============================================================================
// Boolean Functions
// =============================================================================

/// `boolean(object)` - converts the argument to boolean.
fn fn_boolean(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_not(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_true(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_false(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_lang(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_sum(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_floor(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_ceiling(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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
fn fn_round(args: Vec<XPathValue>, _ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
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

// =============================================================================
// Helper Functions
// =============================================================================

/// `text()` - returns the text content of the context node.
/// (Usually handled as a node test, but can be called as function)
fn fn_text(args: Vec<XPathValue>, ctx: &EvaluationContext<'_>) -> Result<XPathValue> {
    if !args.is_empty() {
        return Err(XPathEvalError::WrongArgumentCount {
            function: "text".to_string(),
            expected: "0".to_string(),
            found: args.len(),
        }
        .into());
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
        return Err(XPathEvalError::WrongArgumentCount {
            function: "(node function)".to_string(),
            expected: "0 or 1".to_string(),
            found: args.len(),
        }
        .into());
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
    use crate::document::XmlDocument;
    use crate::namespace::NamespaceResolver;

    fn create_test_document() -> XmlDocument {
        crate::parse(
            "<root><item id=\"1\">10</item><item id=\"2\">20</item><item id=\"3\">30</item></root>",
        )
        .unwrap()
    }

    fn create_context<'a>(doc: &'a XmlDocument, node: &XmlNode) -> EvaluationContext<'a> {
        EvaluationContext::new(node.clone(), doc, NamespaceResolver::new())
    }

    // =============================================================================
    // Node Set Functions Tests
    // =============================================================================

    #[test]
    fn test_fn_last() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root).with_position(1, 5);

        let result = evaluate_function("last", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 5.0);
    }

    #[test]
    fn test_fn_last_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("last", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_position() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root).with_position(3, 5);

        let result = evaluate_function("position", vec![], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_position_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("position", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_count() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let result = evaluate_function("count", vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(result.to_number(), 3.0);
    }

    #[test]
    fn test_fn_count_empty() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("count", vec![XPathValue::NodeSet(vec![])], &ctx).unwrap();
        assert_eq!(result.to_number(), 0.0);
    }

    #[test]
    fn test_fn_count_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        // No arguments
        let result = evaluate_function("count", vec![], &ctx);
        assert!(result.is_err());

        // Wrong type
        let result = evaluate_function("count", vec![XPathValue::String("test".to_string())], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_name() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("name", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "root");
    }

    #[test]
    fn test_fn_name_with_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let result = evaluate_function("name", vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "item");
    }

    #[test]
    fn test_fn_local_name() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("local-name", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "root");
    }

    #[test]
    fn test_fn_namespace_uri() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("namespace-uri", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_fn_id() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("id", vec![XPathValue::String("2".to_string())], &ctx).unwrap();
        match result {
            XPathValue::NodeSet(nodes) => {
                assert_eq!(nodes.len(), 1);
                assert_eq!(nodes[0].get_attribute("id").unwrap(), "2");
            }
            _ => panic!("Expected NodeSet"),
        }
    }

    #[test]
    fn test_fn_id_multiple() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("id", vec![XPathValue::String("1 3".to_string())], &ctx).unwrap();
        match result {
            XPathValue::NodeSet(nodes) => {
                assert_eq!(nodes.len(), 2);
            }
            _ => panic!("Expected NodeSet"),
        }
    }

    #[test]
    fn test_fn_id_not_found() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("id", vec![XPathValue::String("999".to_string())], &ctx).unwrap();
        match result {
            XPathValue::NodeSet(nodes) => {
                assert_eq!(nodes.len(), 0);
            }
            _ => panic!("Expected NodeSet"),
        }
    }

    #[test]
    fn test_fn_id_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("id", vec![], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_fn_id_with_nodeset() {
        let doc = crate::parse(
            "<root><ids>1 2</ids><item id=\"1\">A</item><item id=\"2\">B</item></root>",
        )
        .unwrap();
        let root = doc.get_root_element().unwrap();
        let ids_node = root.get_child_nodes().into_iter().next().unwrap();
        let ctx = create_context(&doc, &root);

        let result =
            evaluate_function("id", vec![XPathValue::NodeSet(vec![ids_node])], &ctx).unwrap();
        match result {
            XPathValue::NodeSet(nodes) => {
                assert_eq!(nodes.len(), 2);
            }
            _ => panic!("Expected NodeSet"),
        }
    }

    // =============================================================================
    // String Functions Tests
    // =============================================================================

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

    // =============================================================================
    // Boolean Functions Tests
    // =============================================================================

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

    // =============================================================================
    // Number Functions Tests
    // =============================================================================

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

    // =============================================================================
    // Other Functions Tests
    // =============================================================================

    #[test]
    fn test_fn_text() {
        let doc = crate::parse("<root>hello world</root>").unwrap();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("text", vec![], &ctx).unwrap();
        assert_eq!(result.to_string_value(), "hello world");
    }

    #[test]
    fn test_fn_text_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("text", vec![XPathValue::Number(1.0)], &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_function() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = evaluate_function("unknown-function", vec![], &ctx);
        assert!(result.is_err());
    }

    // =============================================================================
    // Helper Function Tests
    // =============================================================================

    #[test]
    fn test_get_first_node_or_context_empty() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node = get_first_node_or_context(vec![], &ctx).unwrap();
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);
        let children = root.get_child_nodes();

        let node = get_first_node_or_context(vec![XPathValue::NodeSet(children)], &ctx).unwrap();
        assert_eq!(node.get_name(), "item");
    }

    #[test]
    fn test_get_first_node_or_context_empty_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node = get_first_node_or_context(vec![XPathValue::NodeSet(vec![])], &ctx).unwrap();
        // Returns context node when empty
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_non_nodeset() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let node =
            get_first_node_or_context(vec![XPathValue::String("test".to_string())], &ctx).unwrap();
        // Returns context node for non-nodeset
        assert_eq!(node.get_name(), "root");
    }

    #[test]
    fn test_get_first_node_or_context_wrong_args() {
        let doc = create_test_document();
        let root = doc.get_root_element().unwrap();
        let ctx = create_context(&doc, &root);

        let result = get_first_node_or_context(
            vec![
                XPathValue::String("a".to_string()),
                XPathValue::String("b".to_string()),
            ],
            &ctx,
        );
        assert!(result.is_err());
    }

    // =============================================================================
    // Original Helper Tests (kept for reference)
    // =============================================================================

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
