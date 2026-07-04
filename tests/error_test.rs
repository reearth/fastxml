//! Tests for error handling with malformed XML and validation violations.

use fastxml::error::Error;
use fastxml::parser::error::ParseError;
use fastxml::xpath::error::{XPathEvalError, XPathSyntaxError};
use fastxml::{Parser, ParserOptions};

// =============================================================================
// Malformed XML Tests
// =============================================================================

mod malformed_xml {
    use super::*;

    #[test]
    fn test_unclosed_tag() {
        let xml = "<root><child>";
        let result = Parser::from(xml).parse();
        // An element left unclosed at EOF is not well-formed.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for an unclosed element, got: {result:?}"
        );
    }

    #[test]
    fn test_mismatched_tags() {
        let xml = "<root></roo>";
        //         0123456789AB (hex) = positions
        //         "<root>" = 6 bytes, "</roo>" = 6 bytes, total = 12 bytes
        let result = Parser::from(xml).parse();
        match &result {
            Err(Error::Parse(ParseError::AtPosition { message, position })) => {
                assert!(
                    message.contains("expected") || message.contains("mismatch"),
                    "Expected mismatch message, got: {}",
                    message
                );
                // Error detected at end of closing tag (byte 12)
                assert_eq!(
                    *position, 12,
                    "Error position should be at end of closing tag"
                );
            }
            _ => panic!(
                "Expected Parse error with mismatch message, got: {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_mismatched_nested_tags() {
        let xml = "<root><child></root></child>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_tag_name_starting_with_number() {
        let xml = "<1root/>";
        let result = Parser::from(xml).parse();
        // A digit is a NameChar but not a NameStartChar (XML 1.0 P5), so a name
        // beginning with one is not well-formed.
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for tag name starting with a digit, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_tag_name_illegal_char() {
        // U+00D7 (MULTIPLICATION SIGN) is a legal XML character but falls in a
        // gap of the BaseChar production, so it is not a NameChar.
        let xml = "<a\u{00D7}b/>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for illegal character in name, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_attribute_name_starting_with_number() {
        let xml = r#"<root 1attr="x"/>"#;
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for attribute name starting with a digit, got: {:?}",
            result
        );
    }

    #[test]
    fn test_valid_unicode_tag_name() {
        // Names built from Letters/Ideographics outside ASCII must be accepted
        // (regression guard against an overly strict NameChar table).
        for xml in ["<日本語/>", "<Élément/>", "<café/>", "<_x.y-z0/>"] {
            let result = Parser::from(xml).parse();
            assert!(
                result.is_ok(),
                "Expected valid Unicode name to parse, xml={:?}, got: {:?}",
                xml,
                result
            );
        }
    }

    #[test]
    fn test_invalid_tag_name_with_space() {
        let xml = "<root element/>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for tag with space, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_attribute_no_value() {
        let xml = r#"<root attr=>"#;
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for attribute without value, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_attribute_no_quotes() {
        let xml = r#"<root attr=value/>"#;
        let result = Parser::from(xml).parse();
        // quick-xml does NOT accept unquoted attribute values
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unquoted attribute, got: {:?}",
            result
        );
    }

    #[test]
    fn test_duplicate_attributes() {
        let xml = r#"<root attr="1" attr="2"/>"#;
        let result = Parser::from(xml).parse();
        // quick-xml returns error for duplicate attributes
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for duplicate attributes, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unescaped_ampersand() {
        let xml = "<root>a & b</root>";
        let result = Parser::from(xml).parse();
        // quick-xml error: "Cannot find ';' after '&'"
        assert!(
            matches!(&result, Err(Error::Parse(_))),
            "Expected Parse error for unescaped &, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unescaped_less_than() {
        let xml = "<root>a < b</root>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unescaped <, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_entity_reference() {
        let xml = "<root>&invalid;</root>";
        let result = Parser::from(xml).parse();
        // quick-xml returns error for unknown entities
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for invalid entity, got: {:?}",
            result
        );
    }

    #[test]
    fn test_incomplete_entity_reference() {
        let xml = "<root>&amp</root>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for incomplete entity, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xml_declaration() {
        let xml = r#"<?xml version="2.0"?><root/>"#;
        let result = Parser::from(xml).parse();
        // VersionNum is '1.' [0-9]+ (XML 1.0 P26); "2.0" is not well-formed.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for version 2.0, got: {result:?}"
        );
    }

    #[test]
    fn test_xml_declaration_not_at_start() {
        let xml = " <?xml version=\"1.0\"?><root/>";
        let result = Parser::from(xml).parse();
        // The XML declaration must be the very first thing in the document; even
        // leading white space makes it not well-formed.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for a declaration not at the start, got: {result:?}"
        );
    }

    #[test]
    fn test_multiple_root_elements() {
        let xml = "<root1/><root2/>";
        let result = Parser::from(xml).parse();
        // A document may contain only one root element.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for a second root element, got: {result:?}"
        );
    }

    #[test]
    fn test_empty_input() {
        let xml = "";
        let result = Parser::from(xml).parse();
        // A well-formed document must contain a root element.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Empty input should be rejected (no document element), got: {result:?}"
        );
    }

    #[test]
    fn test_whitespace_only() {
        let xml = "   \n\t  ";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Whitespace-only input should be rejected (no document element), got: {result:?}"
        );
    }

    #[test]
    fn test_comment_only() {
        let xml = "<!-- just a comment -->";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Comment-only input should be rejected (no document element), got: {result:?}"
        );
    }

    #[test]
    fn test_unclosed_comment() {
        let xml = "<root><!-- unclosed comment</root>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unclosed comment, got: {:?}",
            result
        );
    }

    #[test]
    fn test_double_hyphen_in_comment() {
        let xml = "<root><!-- invalid -- comment --></root>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for -- in comment, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unclosed_cdata() {
        let xml = "<root><![CDATA[unclosed</root>";
        let result = Parser::from(xml).parse();
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unclosed CDATA, got: {:?}",
            result
        );
    }

    #[test]
    fn test_cdata_end_in_cdata() {
        let xml = "<root><![CDATA[contains ]]> in middle]]></root>";
        let result = Parser::from(xml).parse();
        // The CDATA ends at the first ']]>'; the following ' in middle]]>' is
        // character data, and the literal ']]>' there is not well-formed.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for ']]>' in character data, got: {result:?}"
        );
    }

    #[test]
    fn test_invalid_namespace_prefix() {
        let xml = r#"<unknown:root/>"#;
        let result = Parser::from(xml).parse();
        // quick-xml accepts undeclared namespace prefixes
        assert!(result.is_ok(), "quick-xml accepts undeclared prefixes");
    }

    #[test]
    fn test_xml_reserved_prefix() {
        let xml = r#"<xml:element xmlns:xml="http://wrong.url"/>"#;
        let result = Parser::from(xml).parse();
        // The 'xml' prefix may only be bound to the XML namespace.
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for a misbound 'xml' prefix, got: {result:?}"
        );
    }

    #[test]
    fn test_deeply_nested_elements() {
        let depth = 1000;
        let open_tags: String = (0..depth).map(|i| format!("<e{}>", i)).collect();
        let close_tags: String = (0..depth).rev().map(|i| format!("</e{}>", i)).collect();
        let xml = format!("{}{}", open_tags, close_tags);

        let result = Parser::from(xml.as_str()).parse();
        // Should succeed - no stack overflow
        assert!(result.is_ok(), "Deep nesting should be handled");
    }

    #[test]
    fn test_extremely_long_tag_name() {
        let long_name = "a".repeat(10000);
        let xml = format!("<{}/>", long_name);

        let result = Parser::from(xml.as_str()).parse();
        assert!(result.is_ok());
        let doc = result.unwrap();
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name().len(), 10000);
    }

    #[test]
    fn test_extremely_long_attribute_value() {
        let long_value = "x".repeat(100000);
        let xml = format!(r#"<root attr="{}"/>"#, long_value);

        let result = Parser::from(xml.as_str()).parse();
        assert!(result.is_ok());
        let doc = result.unwrap();
        let root = doc.get_root_element().unwrap();
        let attr = root.get_attribute("attr");
        assert_eq!(attr.map(|s| s.len()), Some(100000));
    }

    #[test]
    fn test_null_byte_in_content() {
        let xml = "<root>hello\0world</root>";
        let result = Parser::from(xml).parse();
        // A literal NUL is not a legal XML character (Char production).
        assert!(result.is_err(), "NUL in content is not well-formed");
    }

    #[test]
    fn test_control_characters() {
        let xml = "<root>\x01\x02\x03</root>";
        let result = Parser::from(xml).parse();
        // C0 control characters (other than tab/LF/CR) violate the Char
        // production and must be rejected.
        assert!(
            result.is_err(),
            "control characters in content are not well-formed"
        );
    }

    #[test]
    fn test_invalid_utf8() {
        let invalid_bytes: &[u8] = &[
            0x3c, 0x72, 0x6f, 0x6f, 0x74, 0x3e, 0xff, 0xfe, 0x3c, 0x2f, 0x72, 0x6f, 0x6f, 0x74,
            0x3e,
        ];
        let result = Parser::from(invalid_bytes).parse();
        // Invalid UTF-8 should cause an error
        assert!(
            matches!(
                result,
                Err(Error::Parse(_) | Error::Utf8(_) | Error::FromUtf8(_))
            ),
            "Expected encoding error, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Parser Options Tests
// =============================================================================

mod parser_options {
    use super::*;

    #[test]
    fn test_memory_limit_exceeded() {
        let options = ParserOptions {
            max_memory: Some(100),
            ..Default::default()
        };

        let large_xml = format!("<root>{}</root>", "x".repeat(1000));
        let result = Parser::from(large_xml.as_str()).options(options).parse();
        assert!(
            matches!(
                &result,
                Err(Error::Parse(ParseError::MemoryLimitExceeded { .. }))
            ),
            "Expected memory limit error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_memory_limit_not_exceeded() {
        let options = ParserOptions {
            max_memory: Some(1_000_000),
            ..Default::default()
        };

        let xml = "<root>small content</root>";
        let result = Parser::from(xml).options(options).parse();
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_options() {
        let options = ParserOptions::default();
        let xml = "<root><child>text</child></root>";
        let result = Parser::from(xml).options(options).parse();
        assert!(result.is_ok());
    }
}

// =============================================================================
// XPath Error Tests
// =============================================================================

mod xpath_errors {
    use super::*;
    use fastxml::QueryExt;

    #[test]
    fn test_invalid_xpath_unclosed_bracket() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("/root[");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_unclosed_parenthesis() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("count(/root");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_unknown_function() {
        use super::XPathEvalError;
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("unknownfn()");
        // Unknown function returns error
        assert!(
            matches!(&result, Err(Error::XPathEval(XPathEvalError::UnknownFunction { name })) if name == "unknownfn"),
            "Expected XPathEval error for unknown function, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_unknown_axis() {
        use super::XPathSyntaxError;
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("unknownaxis::*");
        // Unknown axis returns error
        assert!(
            matches!(&result, Err(Error::XPathSyntax(XPathSyntaxError::UnknownAxis { name })) if name == "unknownaxis"),
            "Expected XPathSyntax error for unknown axis, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_empty() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error for empty expression, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xpath_double_slash_at_end() {
        let doc = Parser::from("<root><child/></root>").parse().unwrap();
        let result = doc.query("/root//");
        // Trailing // matches all descendants of root
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let nodes = result.unwrap().into_nodes();
        // Returns root and child (all descendants including self)
        assert!(!nodes.is_empty(), "// at end matches all descendants");
    }

    #[test]
    fn test_invalid_xpath_missing_operand() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("/root +");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xpath_division_by_zero() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("1 div 0");
        // XPath 1.0: division by zero returns Infinity, not an error
        assert!(result.is_ok(), "XPath division by zero returns Infinity");
    }

    #[test]
    fn test_xpath_invalid_number() {
        let doc = Parser::from("<root/>").parse().unwrap();
        let result = doc.query("number('not a number') + 1");
        // XPath: number('invalid') returns NaN, arithmetic with NaN is valid
        assert!(result.is_ok(), "XPath NaN arithmetic is valid");
    }

    #[test]
    fn test_xpath_on_empty_document() {
        // An empty document is now rejected at parse time (no root element).
        let result = Parser::from("").parse();
        assert!(
            matches!(result, Err(Error::Parse(ParseError::NotWellFormed { .. }))),
            "Expected NotWellFormed for an empty document, got: {result:?}"
        );
    }
}

// =============================================================================
// Streaming Parser Error Tests
// =============================================================================

mod streaming_errors {
    use fastxml::Parser;

    #[test]
    fn test_streaming_malformed_xml() {
        let xml = "<root><unclosed>";
        let result = Parser::from(xml).for_each_event(|_event| Ok(()));
        // An element left unclosed at EOF is not well-formed.
        assert!(
            result.is_err(),
            "Streaming should reject an unclosed element at EOF"
        );
    }

    #[test]
    fn test_streaming_valid_xml() {
        let xml = "<root><child>text</child></root>";
        let result = Parser::from(xml).for_each_event(|_event| Ok(()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_streaming_empty_input() {
        let xml = "";
        let result = Parser::from(xml).for_each_event(|_event| Ok(()));
        // A well-formed document must contain a root element.
        assert!(
            result.is_err(),
            "Streaming should reject empty input (no document element)"
        );
    }

    #[test]
    fn test_streaming_mismatched_tags() {
        use fastxml::error::Error;
        use fastxml::parser::error::ParseError;

        let xml = "<root></wrong>";
        //         01234567890123 = positions (14 bytes total)
        //         "<root>" = 6 bytes, "</wrong>" = 8 bytes
        let result = Parser::from(xml).for_each_event(|_event| Ok(()));
        match &result {
            Err(Error::Parse(ParseError::AtPosition { message, position })) => {
                assert!(
                    message.contains("expected")
                        || message.contains("mismatch")
                        || message.contains("EndTag"),
                    "Expected mismatch message, got: {}",
                    message
                );
                // Error detected at end of closing tag (byte 14)
                assert_eq!(
                    *position, 14,
                    "Error position should be at end of closing tag"
                );
            }
            _ => panic!(
                "Expected Parse error for mismatched tags, got: {:?}",
                result
            ),
        }
    }
}
