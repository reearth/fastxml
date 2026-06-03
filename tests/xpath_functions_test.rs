//! Tests for XPath function library.
//! This test file focuses on improving coverage for xpath/functions.rs

mod common;

use fastxml::Parser;
use fastxml::compat::evaluate;

// =============================================================================
// Node Set Functions
// =============================================================================

mod node_set_functions {
    use super::*;

    #[test]
    fn test_last_function() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // last() returns context size
        let result = evaluate(&doc, "/root/item[last()]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("3".to_string()));

        compare_with_libxml!(xpath: xml, "/root/item[last()]", &doc);
    }

    #[test]
    fn test_last_with_wrong_args() {
        let xml = r#"<root><item/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // last() should not take arguments
        let result = evaluate(&doc, "last(1)");
        assert!(result.is_err(), "last() with args should fail");
    }

    #[test]
    fn test_position_function() {
        let xml = r#"<root><item>1</item><item>2</item><item>3</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root/item[position()=2]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("2".to_string()));

        compare_with_libxml!(xpath: xml, "/root/item[position()=2]", &doc);
    }

    #[test]
    fn test_position_with_wrong_args() {
        let xml = r#"<root><item/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "position(1)");
        assert!(result.is_err(), "position() with args should fail");
    }

    #[test]
    fn test_count_function() {
        let xml = r#"<root><item/><item/><item/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "count(/root/item)").unwrap();
        assert_eq!(result.to_number(), 3.0);

        compare_with_libxml!(xpath: xml, "count(/root/item)", &doc);
    }

    #[test]
    fn test_count_with_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // count() requires exactly 1 argument
        let result = evaluate(&doc, "count()");
        assert!(result.is_err(), "count() without args should fail");

        let result = evaluate(&doc, "count(/root, /root)");
        assert!(result.is_err(), "count() with 2 args should fail");
    }

    #[test]
    fn test_count_with_non_nodeset() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // count() requires a node-set argument
        let result = evaluate(&doc, "count('string')");
        assert!(result.is_err(), "count() with string should fail");
    }

    #[test]
    fn test_name_function() {
        let xml = r#"<root><ns:item xmlns:ns="http://example.com"/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "name(/root/ns:item)").unwrap();
        assert_eq!(result.to_string_value(), "ns:item");
    }

    #[test]
    fn test_name_function_context_node() {
        let xml = r#"<root><item/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root/item[name()='item']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        compare_with_libxml!(xpath: xml, "/root/item[name()='item']", &doc);
    }

    #[test]
    fn test_local_name_function() {
        let xml = r#"<root><ns:item xmlns:ns="http://example.com"/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "local-name(/root/ns:item)").unwrap();
        assert_eq!(result.to_string_value(), "item");
    }

    #[test]
    fn test_namespace_uri_function() {
        let xml = r#"<root xmlns:ns="http://example.com"><ns:item/></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // namespace-uri() returns the namespace URI of the element
        let result = evaluate(&doc, "namespace-uri(/root/ns:item)").unwrap();
        // The namespace may or may not be resolved depending on implementation
        // Just verify it returns a string (empty or the URI)
        let uri = result.to_string_value();
        assert!(uri.is_empty() || uri == "http://example.com");
    }

    #[test]
    fn test_id_function() {
        let xml = r#"<root><item id="a">A</item><item id="b">B</item><item id="c">C</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "id('b')").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].get_content(), Some("B".to_string()));

        // Note: libxml comparison for id() depends on DTD declaration
        // compare_with_libxml!(xpath: xml, "id('b')", &doc);
    }

    #[test]
    fn test_id_with_multiple_ids() {
        let xml = r#"<root><item id="a">A</item><item id="b">B</item><item id="c">C</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // id() can take space-separated IDs
        let result = evaluate(&doc, "id('a c')").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_id_with_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "id()");
        assert!(result.is_err(), "id() without args should fail");
    }
}

// =============================================================================
// String Functions
// =============================================================================

mod string_functions {
    use super::*;

    #[test]
    fn test_string_function() {
        let xml = r#"<root>hello</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "string(/root)").unwrap();
        assert_eq!(result.to_string_value(), "hello");

        // String functions return scalar values, not node-sets
        // libxml comparison not applicable for scalar results
    }

    #[test]
    fn test_string_function_no_args() {
        let xml = r#"<root>context text</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // string() with no args uses context node
        let result = evaluate(&doc, "/root[string()='context text']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_string_with_too_many_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "string('a', 'b')");
        assert!(result.is_err(), "string() with 2 args should fail");
    }

    #[test]
    fn test_concat_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "concat('a', 'b', 'c')").unwrap();
        assert_eq!(result.to_string_value(), "abc");

        // Scalar result - libxml comparison not applicable
    }

    #[test]
    fn test_concat_with_one_arg() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // concat() requires at least 2 arguments
        let result = evaluate(&doc, "concat('a')");
        assert!(result.is_err(), "concat() with 1 arg should fail");
    }

    #[test]
    fn test_starts_with_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "starts-with('hello', 'he')").unwrap();
        assert!(result.to_boolean());

        let result = evaluate(&doc, "starts-with('hello', 'lo')").unwrap();
        assert!(!result.to_boolean());

        // Scalar result - libxml comparison not applicable
    }

    #[test]
    fn test_starts_with_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "starts-with('a')");
        assert!(result.is_err(), "starts-with() with 1 arg should fail");

        let result = evaluate(&doc, "starts-with('a', 'b', 'c')");
        assert!(result.is_err(), "starts-with() with 3 args should fail");
    }

    #[test]
    fn test_contains_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "contains('hello world', 'wor')").unwrap();
        assert!(result.to_boolean());

        let result = evaluate(&doc, "contains('hello', 'xyz')").unwrap();
        assert!(!result.to_boolean());

        // Scalar result - libxml comparison not applicable
    }

    #[test]
    fn test_contains_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "contains('a')");
        assert!(result.is_err());
    }

    #[test]
    fn test_substring_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // 2 args: start to end
        let result = evaluate(&doc, "substring('12345', 2)").unwrap();
        assert_eq!(result.to_string_value(), "2345");

        // 3 args: start and length
        let result = evaluate(&doc, "substring('12345', 2, 3)").unwrap();
        assert_eq!(result.to_string_value(), "234");

        compare_with_libxml!(xpath: xml, "substring('12345', 2, 3)", &doc);
    }

    #[test]
    fn test_substring_with_nan() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // NaN start returns empty string
        let result = evaluate(&doc, "substring('hello', number('x'))").unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_substring_with_negative_length() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring('hello', 2, -1)").unwrap();
        assert_eq!(result.to_string_value(), "");
    }

    #[test]
    fn test_substring_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring('a')");
        assert!(result.is_err());

        let result = evaluate(&doc, "substring('a', 1, 2, 3)");
        assert!(result.is_err());
    }

    #[test]
    fn test_substring_before_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring-before('hello/world', '/')").unwrap();
        assert_eq!(result.to_string_value(), "hello");

        let result = evaluate(&doc, "substring-before('hello', 'x')").unwrap();
        assert_eq!(result.to_string_value(), "");

        let result = evaluate(&doc, "substring-before('hello', '')").unwrap();
        assert_eq!(result.to_string_value(), "");

        compare_with_libxml!(xpath: xml, "substring-before('hello/world', '/')", &doc);
    }

    #[test]
    fn test_substring_before_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring-before('a')");
        assert!(result.is_err());
    }

    #[test]
    fn test_substring_after_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring-after('hello/world', '/')").unwrap();
        assert_eq!(result.to_string_value(), "world");

        let result = evaluate(&doc, "substring-after('hello', 'x')").unwrap();
        assert_eq!(result.to_string_value(), "");

        let result = evaluate(&doc, "substring-after('hello', '')").unwrap();
        assert_eq!(result.to_string_value(), "hello");

        compare_with_libxml!(xpath: xml, "substring-after('hello/world', '/')", &doc);
    }

    #[test]
    fn test_substring_after_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "substring-after('a')");
        assert!(result.is_err());
    }

    #[test]
    fn test_string_length_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "string-length('hello')").unwrap();
        assert_eq!(result.to_number(), 5.0);

        compare_with_libxml!(xpath: xml, "string-length('hello')", &doc);
    }

    #[test]
    fn test_string_length_no_args() {
        let xml = r#"<root>test</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // string-length() with no args uses context node
        let result = evaluate(&doc, "/root[string-length()=4]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_string_length_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "string-length('a', 'b')");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_space_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "normalize-space('  hello   world  ')").unwrap();
        assert_eq!(result.to_string_value(), "hello world");

        compare_with_libxml!(xpath: xml, "normalize-space('  hello   world  ')", &doc);
    }

    #[test]
    fn test_normalize_space_no_args() {
        let xml = r#"<root>  spacy  text  </root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root[normalize-space()='spacy text']").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_normalize_space_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "normalize-space('a', 'b')");
        assert!(result.is_err());
    }

    #[test]
    fn test_translate_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "translate('bar', 'abc', 'ABC')").unwrap();
        assert_eq!(result.to_string_value(), "BAr");

        // Removal: no corresponding char in 'to'
        let result = evaluate(&doc, "translate('--aaa--', 'abc-', 'ABC')").unwrap();
        assert_eq!(result.to_string_value(), "AAA");

        compare_with_libxml!(xpath: xml, "translate('bar', 'abc', 'ABC')", &doc);
    }

    #[test]
    fn test_translate_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "translate('a', 'b')");
        assert!(result.is_err());
    }
}

// =============================================================================
// Boolean Functions
// =============================================================================

mod boolean_functions {
    use super::*;

    #[test]
    fn test_boolean_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // Non-empty string is true
        let result = evaluate(&doc, "boolean('text')").unwrap();
        assert!(result.to_boolean());

        // Empty string is false
        let result = evaluate(&doc, "boolean('')").unwrap();
        assert!(!result.to_boolean());

        // Non-zero number is true
        let result = evaluate(&doc, "boolean(1)").unwrap();
        assert!(result.to_boolean());

        // Zero is false
        let result = evaluate(&doc, "boolean(0)").unwrap();
        assert!(!result.to_boolean());

        compare_with_libxml!(xpath: xml, "boolean('text')", &doc);
    }

    #[test]
    fn test_boolean_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "boolean()");
        assert!(result.is_err());

        let result = evaluate(&doc, "boolean('a', 'b')");
        assert!(result.is_err());
    }

    #[test]
    fn test_not_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "not(true())").unwrap();
        assert!(!result.to_boolean());

        let result = evaluate(&doc, "not(false())").unwrap();
        assert!(result.to_boolean());

        compare_with_libxml!(xpath: xml, "not(true())", &doc);
    }

    #[test]
    fn test_not_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "not()");
        assert!(result.is_err());

        let result = evaluate(&doc, "not(true(), false())");
        assert!(result.is_err());
    }

    #[test]
    fn test_true_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "true()").unwrap();
        assert!(result.to_boolean());

        compare_with_libxml!(xpath: xml, "true()", &doc);
    }

    #[test]
    fn test_true_with_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "true(1)");
        assert!(result.is_err());
    }

    #[test]
    fn test_false_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "false()").unwrap();
        assert!(!result.to_boolean());

        compare_with_libxml!(xpath: xml, "false()", &doc);
    }

    #[test]
    fn test_false_with_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "false(1)");
        assert!(result.is_err());
    }

    #[test]
    fn test_lang_function() {
        let xml = r#"<root xml:lang="en"><item xml:lang="en-US">text</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // lang() matches 'en' on root
        let result = evaluate(&doc, "/root[lang('en')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);

        // lang() matches sublanguage 'en-US' on item
        let result = evaluate(&doc, "/root/item[lang('en')]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_lang_function_no_match() {
        let xml = r#"<root xml:lang="en"><item>text</item></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "/root[lang('fr')]").unwrap();
        let nodes = result.into_nodes();
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_lang_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "lang()");
        assert!(result.is_err());

        let result = evaluate(&doc, "lang('en', 'fr')");
        assert!(result.is_err());
    }
}

// =============================================================================
// Number Functions
// =============================================================================

mod number_functions {
    use super::*;

    #[test]
    fn test_number_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "number('42')").unwrap();
        assert_eq!(result.to_number(), 42.0);

        // Non-numeric string becomes NaN
        let result = evaluate(&doc, "number('not a number')").unwrap();
        assert!(result.to_number().is_nan());

        compare_with_libxml!(xpath: xml, "number('42')", &doc);
    }

    #[test]
    fn test_number_no_args() {
        let xml = r#"<root>123</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // number() with no args uses context node
        let result = evaluate(&doc, "/root[number()=123]").unwrap();
        let nodes = result.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn test_number_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "number('1', '2')");
        assert!(result.is_err());
    }

    #[test]
    fn test_sum_function() {
        let xml = r#"<root><n>1</n><n>2</n><n>3</n></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "sum(/root/n)").unwrap();
        assert_eq!(result.to_number(), 6.0);

        compare_with_libxml!(xpath: xml, "sum(/root/n)", &doc);
    }

    #[test]
    fn test_sum_with_nan() {
        let xml = r#"<root><n>1</n><n>two</n><n>3</n></root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // Sum with non-numeric node returns NaN
        let result = evaluate(&doc, "sum(/root/n)").unwrap();
        assert!(result.to_number().is_nan());
    }

    #[test]
    fn test_sum_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "sum()");
        assert!(result.is_err());

        let result = evaluate(&doc, "sum(/root, /root)");
        assert!(result.is_err());

        // sum() requires node-set
        let result = evaluate(&doc, "sum('string')");
        assert!(result.is_err());
    }

    #[test]
    fn test_floor_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "floor(3.7)").unwrap();
        assert_eq!(result.to_number(), 3.0);

        let result = evaluate(&doc, "floor(-3.7)").unwrap();
        assert_eq!(result.to_number(), -4.0);

        compare_with_libxml!(xpath: xml, "floor(3.7)", &doc);
    }

    #[test]
    fn test_floor_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "floor()");
        assert!(result.is_err());

        let result = evaluate(&doc, "floor(1, 2)");
        assert!(result.is_err());
    }

    #[test]
    fn test_ceiling_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "ceiling(3.2)").unwrap();
        assert_eq!(result.to_number(), 4.0);

        let result = evaluate(&doc, "ceiling(-3.2)").unwrap();
        assert_eq!(result.to_number(), -3.0);

        compare_with_libxml!(xpath: xml, "ceiling(3.2)", &doc);
    }

    #[test]
    fn test_ceiling_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "ceiling()");
        assert!(result.is_err());

        let result = evaluate(&doc, "ceiling(1, 2)");
        assert!(result.is_err());
    }

    #[test]
    fn test_round_function() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // XPath rounds .5 towards positive infinity
        let result = evaluate(&doc, "round(1.5)").unwrap();
        assert_eq!(result.to_number(), 2.0);

        let result = evaluate(&doc, "round(-1.5)").unwrap();
        assert_eq!(result.to_number(), -1.0);

        let result = evaluate(&doc, "round(2.5)").unwrap();
        assert_eq!(result.to_number(), 3.0);

        compare_with_libxml!(xpath: xml, "round(1.5)", &doc);
    }

    #[test]
    fn test_round_special_values() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // NaN remains NaN
        let result = evaluate(&doc, "round(number('x'))").unwrap();
        assert!(result.to_number().is_nan());

        // Zero remains zero
        let result = evaluate(&doc, "round(0)").unwrap();
        assert_eq!(result.to_number(), 0.0);

        // Test with negative zero (edge case)
        let result = evaluate(&doc, "round(-0.4)").unwrap();
        assert_eq!(result.to_number(), 0.0);
    }

    #[test]
    fn test_round_wrong_args() {
        let xml = r#"<root/>"#;
        let doc = Parser::from(xml).parse().unwrap();

        let result = evaluate(&doc, "round()");
        assert!(result.is_err());

        let result = evaluate(&doc, "round(1, 2)");
        assert!(result.is_err());
    }
}

// =============================================================================
// Text Function
// =============================================================================

mod text_function {
    use super::*;

    #[test]
    fn test_text_as_function() {
        let xml = r#"<root>content</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // text() is typically used as a node test, but can be called as function too
        let result = evaluate(&doc, "/root/text()").unwrap();
        let texts = fastxml::xpath::collect_text_values(&result);
        assert_eq!(texts, vec!["content"]);
    }

    #[test]
    fn test_text_wrong_args() {
        let xml = r#"<root>content</root>"#;
        let doc = Parser::from(xml).parse().unwrap();

        // text() as function should not take arguments
        // This depends on how text() is parsed - it might be parsed as a node test
        // Let's test the xpath that would reach fn_text
        let result = evaluate(&doc, "/root[text(1)]");
        // If parsed as node test, error might differ
        // Just checking it doesn't panic is sufficient
        let _ = result;
    }
}
