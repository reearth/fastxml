//! Tests for error handling with malformed XML and validation violations.

use fastxml::error::Error;
use fastxml::node_error::NodeError;
use fastxml::parse_error::ParseError;
use fastxml::xpath::error::{XPathEvalError, XPathSyntaxError};
use fastxml::{ParserOptions, parse, parse_with_options};

// =============================================================================
// Malformed XML Tests
// =============================================================================

mod malformed_xml {
    use super::*;

    #[test]
    fn test_unclosed_tag() {
        let xml = "<root><child>";
        let result = parse(xml);
        // quick-xml is lenient with unclosed tags at EOF
        // Document parses but may have incomplete structure
        assert!(result.is_ok());
    }

    #[test]
    fn test_mismatched_tags() {
        let xml = "<root></roo>";
        //         0123456789AB (hex) = positions
        //         "<root>" = 6 bytes, "</roo>" = 6 bytes, total = 12 bytes
        let result = parse(xml);
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
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_tag_name_starting_with_number() {
        let xml = "<1root/>";
        let result = parse(xml);
        // quick-xml is lenient and accepts tags starting with numbers
        assert!(
            result.is_ok(),
            "quick-xml accepts tags starting with numbers"
        );
    }

    #[test]
    fn test_invalid_tag_name_with_space() {
        let xml = "<root element/>";
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for tag with space, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_attribute_no_value() {
        let xml = r#"<root attr=>"#;
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for attribute without value, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_attribute_no_quotes() {
        let xml = r#"<root attr=value/>"#;
        let result = parse(xml);
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
        let result = parse(xml);
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
        let result = parse(xml);
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
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unescaped <, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_entity_reference() {
        let xml = "<root>&invalid;</root>";
        let result = parse(xml);
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
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for incomplete entity, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xml_declaration() {
        let xml = r#"<?xml version="2.0"?><root/>"#;
        let result = parse(xml);
        // quick-xml doesn't validate XML version
        assert!(result.is_ok(), "quick-xml accepts version 2.0");
    }

    #[test]
    fn test_xml_declaration_not_at_start() {
        let xml = " <?xml version=\"1.0\"?><root/>";
        let result = parse(xml);
        // quick-xml is lenient about whitespace before declaration
        assert!(
            result.is_ok(),
            "quick-xml accepts whitespace before declaration"
        );
    }

    #[test]
    fn test_multiple_root_elements() {
        let xml = "<root1/><root2/>";
        let result = parse(xml);
        // quick-xml only parses the first root
        assert!(result.is_ok());
        let doc = result.unwrap();
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name(), "root1");
    }

    #[test]
    fn test_empty_input() {
        let xml = "";
        let result = parse(xml);
        // Empty input is accepted but has no root
        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            matches!(
                doc.get_root_element(),
                Err(Error::Node(NodeError::NoRootElement))
            ),
            "Empty doc should have no root"
        );
    }

    #[test]
    fn test_whitespace_only() {
        let xml = "   \n\t  ";
        let result = parse(xml);
        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            matches!(
                doc.get_root_element(),
                Err(Error::Node(NodeError::NoRootElement))
            ),
            "Whitespace-only doc should have no root"
        );
    }

    #[test]
    fn test_comment_only() {
        let xml = "<!-- just a comment -->";
        let result = parse(xml);
        assert!(result.is_ok());
        let doc = result.unwrap();
        assert!(
            matches!(
                doc.get_root_element(),
                Err(Error::Node(NodeError::NoRootElement))
            ),
            "Comment-only doc should have no root"
        );
    }

    #[test]
    fn test_unclosed_comment() {
        let xml = "<root><!-- unclosed comment</root>";
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unclosed comment, got: {:?}",
            result
        );
    }

    #[test]
    fn test_double_hyphen_in_comment() {
        let xml = "<root><!-- invalid -- comment --></root>";
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for -- in comment, got: {:?}",
            result
        );
    }

    #[test]
    fn test_unclosed_cdata() {
        let xml = "<root><![CDATA[unclosed</root>";
        let result = parse(xml);
        assert!(
            matches!(result, Err(Error::Parse(_))),
            "Expected Parse error for unclosed CDATA, got: {:?}",
            result
        );
    }

    #[test]
    fn test_cdata_end_in_cdata() {
        let xml = "<root><![CDATA[contains ]]> in middle]]></root>";
        let result = parse(xml);
        // ]]> inside CDATA ends it early, but quick-xml handles this gracefully
        // and parses the rest as text/another CDATA
        assert!(
            result.is_ok(),
            "CDATA with ]]> inside is handled gracefully"
        );
    }

    #[test]
    fn test_invalid_namespace_prefix() {
        let xml = r#"<unknown:root/>"#;
        let result = parse(xml);
        // quick-xml accepts undeclared namespace prefixes
        assert!(result.is_ok(), "quick-xml accepts undeclared prefixes");
    }

    #[test]
    fn test_xml_reserved_prefix() {
        let xml = r#"<xml:element xmlns:xml="http://wrong.url"/>"#;
        let result = parse(xml);
        // quick-xml doesn't validate xml prefix namespace
        assert!(result.is_ok(), "quick-xml doesn't validate xml prefix");
    }

    #[test]
    fn test_deeply_nested_elements() {
        let depth = 1000;
        let open_tags: String = (0..depth).map(|i| format!("<e{}>", i)).collect();
        let close_tags: String = (0..depth).rev().map(|i| format!("</e{}>", i)).collect();
        let xml = format!("{}{}", open_tags, close_tags);

        let result = parse(&xml);
        // Should succeed - no stack overflow
        assert!(result.is_ok(), "Deep nesting should be handled");
    }

    #[test]
    fn test_extremely_long_tag_name() {
        let long_name = "a".repeat(10000);
        let xml = format!("<{}/>", long_name);

        let result = parse(&xml);
        assert!(result.is_ok());
        let doc = result.unwrap();
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_name().len(), 10000);
    }

    #[test]
    fn test_extremely_long_attribute_value() {
        let long_value = "x".repeat(100000);
        let xml = format!(r#"<root attr="{}"/>"#, long_value);

        let result = parse(&xml);
        assert!(result.is_ok());
        let doc = result.unwrap();
        let root = doc.get_root_element().unwrap();
        let attr = root.get_attribute("attr");
        assert_eq!(attr.map(|s| s.len()), Some(100000));
    }

    #[test]
    fn test_null_byte_in_content() {
        let xml = "<root>hello\0world</root>";
        let result = parse(xml);
        // quick-xml accepts null bytes in content
        assert!(result.is_ok(), "quick-xml accepts null bytes");
    }

    #[test]
    fn test_control_characters() {
        let xml = "<root>\x01\x02\x03</root>";
        let result = parse(xml);
        // quick-xml accepts control characters in content
        assert!(result.is_ok(), "quick-xml accepts control characters");
    }

    #[test]
    fn test_invalid_utf8() {
        let invalid_bytes: &[u8] = &[
            0x3c, 0x72, 0x6f, 0x6f, 0x74, 0x3e, 0xff, 0xfe, 0x3c, 0x2f, 0x72, 0x6f, 0x6f, 0x74,
            0x3e,
        ];
        let result = parse(invalid_bytes);
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
        let result = parse_with_options(&large_xml, &options);
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
        let result = parse_with_options(xml, &options);
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_options() {
        let options = ParserOptions::default();
        let xml = "<root><child>text</child></root>";
        let result = parse_with_options(xml, &options);
        assert!(result.is_ok());
    }
}

// =============================================================================
// XPath Error Tests
// =============================================================================

mod xpath_errors {
    use super::*;
    use fastxml::evaluate;

    #[test]
    fn test_invalid_xpath_unclosed_bracket() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "/root[");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_unclosed_parenthesis() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "count(/root");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_unknown_function() {
        use super::XPathEvalError;
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "unknownfn()");
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
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "unknownaxis::*");
        // Unknown axis returns error
        assert!(
            matches!(&result, Err(Error::XPathSyntax(XPathSyntaxError::UnknownAxis { name })) if name == "unknownaxis"),
            "Expected XPathSyntax error for unknown axis, got: {:?}",
            result
        );
    }

    #[test]
    fn test_invalid_xpath_empty() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error for empty expression, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xpath_double_slash_at_end() {
        let doc = parse("<root><child/></root>").unwrap();
        let result = evaluate(&doc, "/root//");
        // Trailing // matches all descendants of root
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
        let nodes = result.unwrap().into_nodes();
        // Returns root and child (all descendants including self)
        assert!(!nodes.is_empty(), "// at end matches all descendants");
    }

    #[test]
    fn test_invalid_xpath_missing_operand() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "/root +");
        assert!(
            matches!(result, Err(Error::XPathSyntax(_))),
            "Expected XPathSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_xpath_division_by_zero() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "1 div 0");
        // XPath 1.0: division by zero returns Infinity, not an error
        assert!(result.is_ok(), "XPath division by zero returns Infinity");
    }

    #[test]
    fn test_xpath_invalid_number() {
        let doc = parse("<root/>").unwrap();
        let result = evaluate(&doc, "number('not a number') + 1");
        // XPath: number('invalid') returns NaN, arithmetic with NaN is valid
        assert!(result.is_ok(), "XPath NaN arithmetic is valid");
    }

    #[test]
    fn test_xpath_on_empty_document() {
        let doc = parse("").unwrap();
        let result = evaluate(&doc, "/root");
        // XPath on empty doc returns NodeNotFound error (no root element)
        assert!(
            matches!(result, Err(Error::Node(NodeError::NoRootElement))),
            "Expected Node error, got: {:?}",
            result
        );
    }
}

// =============================================================================
// Streaming Parser Error Tests
// =============================================================================

mod streaming_errors {
    use fastxml::error::Result;
    use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};

    struct EventCounter {
        count: usize,
    }

    impl XmlEventHandler for EventCounter {
        fn handle(&mut self, _event: &XmlEvent) -> Result<()> {
            self.count += 1;
            Ok(())
        }

        fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
            self
        }
    }

    #[test]
    fn test_streaming_malformed_xml() {
        let xml = "<root><unclosed>";
        let mut parser = StreamingParser::new(xml.as_bytes());
        let handler = EventCounter { count: 0 };
        parser.add_handler(Box::new(handler));

        let result = parser.parse();
        // Unclosed tags at EOF: quick-xml is lenient
        assert!(result.is_ok(), "Streaming accepts unclosed tags at EOF");
    }

    #[test]
    fn test_streaming_valid_xml() {
        let xml = "<root><child>text</child></root>";
        let mut parser = StreamingParser::new(xml.as_bytes());
        let handler = EventCounter { count: 0 };
        parser.add_handler(Box::new(handler));

        let result = parser.parse();
        assert!(result.is_ok());
    }

    #[test]
    fn test_streaming_empty_input() {
        let xml = "";
        let mut parser = StreamingParser::new(xml.as_bytes());
        let handler = EventCounter { count: 0 };
        parser.add_handler(Box::new(handler));

        let result = parser.parse();
        assert!(result.is_ok(), "Empty input should be handled gracefully");
    }

    #[test]
    fn test_streaming_mismatched_tags() {
        use fastxml::error::Error;
        use fastxml::parse_error::ParseError;

        let xml = "<root></wrong>";
        //         01234567890123 = positions (14 bytes total)
        //         "<root>" = 6 bytes, "</wrong>" = 8 bytes
        let mut parser = StreamingParser::new(xml.as_bytes());
        let handler = EventCounter { count: 0 };
        parser.add_handler(Box::new(handler));

        let result = parser.parse();
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
