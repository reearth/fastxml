//! Tests for error type Display implementations.
//! This improves coverage for error types that have 0% coverage.

use fastxml::error::{Error, ErrorLevel, StructuredError, ValidationErrorType};
use fastxml::namespace::error::NamespaceError;
use fastxml::node::error::NodeError;
use fastxml::parser::error::ParseError;
use fastxml::xpath::error::{XPathEvalError, XPathSyntaxError};
use fastxml::xpath::lexer::Token;

// =============================================================================
// NodeError Tests
// =============================================================================

mod node_error_tests {
    use super::*;

    #[test]
    fn test_no_root_element_display() {
        let err = NodeError::NoRootElement;
        assert_eq!(format!("{}", err), "no root element");
    }

    #[test]
    fn test_node_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(NodeError::NoRootElement);
        assert!(err.source().is_none());
    }

    #[test]
    fn test_node_error_eq() {
        assert_eq!(NodeError::NoRootElement, NodeError::NoRootElement);
    }
}

// =============================================================================
// ParseError Tests
// =============================================================================

mod parse_error_tests {
    use super::*;

    #[test]
    fn test_at_position_display() {
        let err = ParseError::AtPosition {
            position: 42,
            message: "unexpected token".to_string(),
        };
        assert!(format!("{}", err).contains("42"));
        assert!(format!("{}", err).contains("unexpected token"));
    }

    #[test]
    fn test_memory_limit_exceeded_display() {
        let err = ParseError::MemoryLimitExceeded {
            used: 1000,
            max: 500,
        };
        let display = format!("{}", err);
        assert!(display.contains("1000"));
        assert!(display.contains("500"));
    }

    #[test]
    fn test_text_decode_error_display() {
        let err = ParseError::TextDecodeError {
            message: "invalid utf-8".to_string(),
        };
        assert!(format!("{}", err).contains("invalid utf-8"));
    }

    #[test]
    fn test_attribute_decode_error_display() {
        let err = ParseError::AttributeDecodeError {
            message: "bad attribute".to_string(),
        };
        assert!(format!("{}", err).contains("bad attribute"));
    }

    #[test]
    fn test_attribute_error_display() {
        let err = ParseError::AttributeError {
            message: "duplicate attribute".to_string(),
        };
        assert!(format!("{}", err).contains("duplicate attribute"));
    }

    #[test]
    fn test_generic_display() {
        let err = ParseError::Generic {
            message: "generic error".to_string(),
        };
        assert!(format!("{}", err).contains("generic error"));
    }

    #[test]
    fn test_parse_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(ParseError::Generic {
            message: "test".to_string(),
        });
        assert!(err.source().is_none());
    }

    #[test]
    fn test_parse_error_eq() {
        let err1 = ParseError::AtPosition {
            position: 10,
            message: "test".to_string(),
        };
        let err2 = ParseError::AtPosition {
            position: 10,
            message: "test".to_string(),
        };
        assert_eq!(err1, err2);
    }
}

// =============================================================================
// XPathSyntaxError Tests
// =============================================================================

mod xpath_syntax_error_tests {
    use super::*;

    #[test]
    fn test_unexpected_token_with_token() {
        let err = XPathSyntaxError::UnexpectedToken {
            found: Some(Token::Number(42.0)),
            expected: "operator".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("expected operator"));
        assert!(display.contains("Number"));
    }

    #[test]
    fn test_unexpected_token_eof() {
        let err = XPathSyntaxError::UnexpectedToken {
            found: None,
            expected: "expression".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("expected expression"));
        assert!(display.contains("end of input"));
    }

    #[test]
    fn test_unexpected_character() {
        let err = XPathSyntaxError::UnexpectedCharacter {
            char: '§',
            position: 15,
        };
        let display = format!("{}", err);
        assert!(display.contains("§"));
        assert!(display.contains("15"));
    }

    #[test]
    fn test_invalid_number() {
        let err = XPathSyntaxError::InvalidNumber {
            value: "12.34.56".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("12.34.56"));
    }

    #[test]
    fn test_unknown_axis() {
        let err = XPathSyntaxError::UnknownAxis {
            name: "badaxis".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("badaxis"));
        assert!(display.contains("child")); // Should list valid axes
    }

    #[test]
    fn test_unknown_function_syntax() {
        let err = XPathSyntaxError::UnknownFunction {
            name: "badfunc".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("badfunc"));
    }

    #[test]
    fn test_unclosed_bracket() {
        let err = XPathSyntaxError::UnclosedBracket;
        let display = format!("{}", err);
        assert!(display.contains("["));
    }

    #[test]
    fn test_unclosed_parenthesis() {
        let err = XPathSyntaxError::UnclosedParenthesis;
        let display = format!("{}", err);
        assert!(display.contains("("));
    }

    #[test]
    fn test_unclosed_string() {
        let err = XPathSyntaxError::UnclosedString;
        let display = format!("{}", err);
        assert!(display.contains("string"));
    }

    #[test]
    fn test_empty_expression() {
        let err = XPathSyntaxError::EmptyExpression;
        let display = format!("{}", err);
        assert!(display.contains("empty"));
    }

    #[test]
    fn test_missing_operand() {
        let err = XPathSyntaxError::MissingOperand {
            operator: "+".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("+"));
        assert!(display.contains("operand"));
    }

    #[test]
    fn test_xpath_syntax_error_eq() {
        assert_eq!(
            XPathSyntaxError::UnclosedBracket,
            XPathSyntaxError::UnclosedBracket
        );
    }
}

// =============================================================================
// XPathEvalError Tests
// =============================================================================

mod xpath_eval_error_tests {
    use super::*;

    #[test]
    fn test_unknown_function_eval() {
        let err = XPathEvalError::UnknownFunction {
            name: "myfunc".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("myfunc"));
    }

    #[test]
    fn test_wrong_argument_count() {
        let err = XPathEvalError::WrongArgumentCount {
            function: "concat".to_string(),
            expected: "at least 2".to_string(),
            found: 1,
        };
        let display = format!("{}", err);
        assert!(display.contains("concat"));
        assert!(display.contains("at least 2"));
        assert!(display.contains("1"));
    }

    #[test]
    fn test_invalid_argument_type() {
        let err = XPathEvalError::InvalidArgumentType {
            function: "count".to_string(),
            expected: "node-set".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("count"));
        assert!(display.contains("node-set"));
    }

    #[test]
    fn test_undefined_variable() {
        let err = XPathEvalError::UndefinedVariable("myvar".to_string());
        let display = format!("{}", err);
        assert!(display.contains("$myvar"));
    }

    #[test]
    fn test_xpath_eval_error_eq() {
        assert_eq!(
            XPathEvalError::UndefinedVariable("x".to_string()),
            XPathEvalError::UndefinedVariable("x".to_string())
        );
    }
}

// =============================================================================
// NamespaceError Tests
// =============================================================================

mod namespace_error_tests {
    use super::*;

    #[test]
    fn test_unknown_prefix() {
        let err = NamespaceError::UnknownPrefix {
            prefix: "ns".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("ns"));
        assert!(display.contains("unknown"));
    }

    #[test]
    fn test_uri_mismatch() {
        let err = NamespaceError::UriMismatch {
            expected: "http://expected.com".to_string(),
            found: "http://found.com".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("http://expected.com"));
        assert!(display.contains("http://found.com"));
    }

    #[test]
    fn test_missing_declaration() {
        let err = NamespaceError::MissingDeclaration {
            uri: "http://missing.com".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("http://missing.com"));
    }

    #[test]
    fn test_namespace_error_eq() {
        assert_eq!(
            NamespaceError::UnknownPrefix {
                prefix: "x".to_string()
            },
            NamespaceError::UnknownPrefix {
                prefix: "x".to_string()
            }
        );
    }
}

// =============================================================================
// ErrorLevel Tests
// =============================================================================

mod error_level_tests {
    use super::*;

    #[test]
    fn test_error_level_display() {
        assert_eq!(format!("{}", ErrorLevel::Warning), "warning");
        assert_eq!(format!("{}", ErrorLevel::Error), "error");
        assert_eq!(format!("{}", ErrorLevel::Fatal), "fatal");
    }

    #[test]
    fn test_error_level_default() {
        assert_eq!(ErrorLevel::default(), ErrorLevel::Error);
    }

    #[test]
    fn test_error_level_ord() {
        assert!(ErrorLevel::Warning < ErrorLevel::Error);
        assert!(ErrorLevel::Error < ErrorLevel::Fatal);
    }
}

// =============================================================================
// StructuredError Tests
// =============================================================================

mod structured_error_tests {
    use super::*;

    #[test]
    fn test_structured_error_new() {
        let err = StructuredError::new("test error", ValidationErrorType::UnknownElement);
        assert_eq!(err.message.as_ref(), "test error");
        assert_eq!(err.error_type, ValidationErrorType::UnknownElement);
    }

    #[test]
    fn test_structured_error_builders() {
        let err = StructuredError::new("test", ValidationErrorType::InvalidContent)
            .with_line(10)
            .with_column(5)
            .with_level(ErrorLevel::Warning)
            .with_element_path("/root/child")
            .with_node_name("child")
            .with_expected("string")
            .with_found("number");

        assert_eq!(err.line(), Some(10));
        assert_eq!(err.column(), Some(5));
        assert_eq!(err.level, ErrorLevel::Warning);
        assert_eq!(err.element_path(), Some("/root/child"));
        assert_eq!(err.node_name.as_deref(), Some("child"));
        assert_eq!(err.expected.as_deref(), Some("string"));
        assert_eq!(err.found.as_deref(), Some("number"));
    }

    #[test]
    fn test_structured_error_is_warning() {
        let warning = StructuredError::new("warn", ValidationErrorType::Other)
            .with_level(ErrorLevel::Warning);
        let error =
            StructuredError::new("err", ValidationErrorType::Other).with_level(ErrorLevel::Error);

        assert!(warning.is_warning());
        assert!(!error.is_warning());
    }

    #[test]
    fn test_structured_error_is_error() {
        let warning = StructuredError::new("warn", ValidationErrorType::Other)
            .with_level(ErrorLevel::Warning);
        let error =
            StructuredError::new("err", ValidationErrorType::Other).with_level(ErrorLevel::Error);
        let fatal =
            StructuredError::new("fatal", ValidationErrorType::Other).with_level(ErrorLevel::Fatal);

        assert!(!warning.is_error());
        assert!(error.is_error());
        assert!(fatal.is_error());
    }

    #[test]
    fn test_structured_error_display_with_path() {
        let err = StructuredError::new("element not found", ValidationErrorType::UnknownElement)
            .with_element_path("/root/child")
            .with_line(10);
        let display = format!("{}", err);
        assert!(display.contains("/root/child"));
        assert!(display.contains("line 10"));
    }

    #[test]
    fn test_structured_error_display_with_line_col() {
        let err = StructuredError::new("syntax error", ValidationErrorType::Other)
            .with_line(5)
            .with_column(20);
        let display = format!("{}", err);
        assert!(display.contains("5:20"));
    }

    #[test]
    fn test_structured_error_display_with_line_only() {
        let err = StructuredError::new("error", ValidationErrorType::Other).with_line(42);
        let display = format!("{}", err);
        assert!(display.contains("line 42"));
    }

    #[test]
    fn test_structured_error_display_with_expected_found() {
        let err = StructuredError::new("type mismatch", ValidationErrorType::InvalidTextContent)
            .with_expected("integer")
            .with_found("string");
        let display = format!("{}", err);
        assert!(display.contains("expected: integer"));
        assert!(display.contains("found: string"));
    }

    #[test]
    fn test_structured_error_default() {
        let err = StructuredError::default();
        assert_eq!(err.message.as_ref(), "");
        assert_eq!(err.line(), None);
        assert_eq!(err.column(), None);
        assert_eq!(err.error_type, ValidationErrorType::Other);
        assert_eq!(err.level, ErrorLevel::Error);
    }
}

// =============================================================================
// ValidationErrorType Tests
// =============================================================================

mod validation_error_type_tests {
    use super::*;

    #[test]
    fn test_validation_error_type_eq() {
        assert_eq!(
            ValidationErrorType::UnknownElement,
            ValidationErrorType::UnknownElement
        );
        assert_ne!(
            ValidationErrorType::UnknownElement,
            ValidationErrorType::UnknownAttribute
        );
    }

    #[test]
    fn test_all_validation_error_types() {
        // Just ensure all variants can be created
        let _types = [
            ValidationErrorType::UnknownElement,
            ValidationErrorType::UnknownAttribute,
            ValidationErrorType::MissingRequiredElement,
            ValidationErrorType::MissingRequiredAttribute,
            ValidationErrorType::InvalidAttributeValue,
            ValidationErrorType::InvalidContent,
            ValidationErrorType::InvalidTextContent,
            ValidationErrorType::TooManyOccurrences,
            ValidationErrorType::TooFewOccurrences,
            ValidationErrorType::ElementOutOfOrder,
            ValidationErrorType::UnexpectedElement,
            ValidationErrorType::NamespaceMismatch,
            ValidationErrorType::SchemaNotFound,
            ValidationErrorType::IdentityConstraint,
            ValidationErrorType::TypeNotFound,
            ValidationErrorType::FacetViolation,
            ValidationErrorType::ContentModelViolation,
            ValidationErrorType::UnclosedElement,
            ValidationErrorType::Other,
        ];
    }
}

// =============================================================================
// Error Conversion Tests
// =============================================================================

mod error_conversion_tests {
    use super::*;

    #[test]
    fn test_error_from_parse_error() {
        let parse_err = ParseError::Generic {
            message: "test".to_string(),
        };
        let err: Error = parse_err.into();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn test_error_from_node_error() {
        let node_err = NodeError::NoRootElement;
        let err: Error = node_err.into();
        assert!(matches!(err, Error::Node(_)));
    }

    #[test]
    fn test_error_from_xpath_syntax_error() {
        let xpath_err = XPathSyntaxError::EmptyExpression;
        let err: Error = xpath_err.into();
        assert!(matches!(err, Error::XPathSyntax(_)));
    }

    #[test]
    fn test_error_from_xpath_eval_error() {
        let xpath_err = XPathEvalError::UnknownFunction {
            name: "test".to_string(),
        };
        let err: Error = xpath_err.into();
        assert!(matches!(err, Error::XPathEval(_)));
    }

    #[test]
    fn test_error_from_namespace_error() {
        let ns_err = NamespaceError::UnknownPrefix {
            prefix: "ns".to_string(),
        };
        let err: Error = ns_err.into();
        assert!(matches!(err, Error::Namespace(_)));
    }

    #[test]
    fn test_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn test_error_from_quick_xml_error() {
        // Create a quick_xml error through parsing
        let bad_xml = "<root></roo>";
        let result = fastxml::Parser::from(bad_xml).parse();
        assert!(matches!(result, Err(Error::Parse(_))));
    }

    #[test]
    fn test_error_display() {
        let err = Error::InvalidOperation("cannot do that".to_string());
        let display = format!("{}", err);
        assert!(display.contains("cannot do that"));
    }

    #[test]
    fn test_validation_error() {
        let err = Error::Validation {
            message: "invalid value".to_string(),
            line: Some(10),
            column: Some(5),
        };
        let display = format!("{}", err);
        assert!(display.contains("invalid value"));
    }
}

// =============================================================================
// TransformError Tests
// =============================================================================

mod transform_error_tests {
    use fastxml::transform::TransformError;

    #[test]
    fn test_invalid_xpath_display() {
        let err = TransformError::InvalidXPath("bad xpath".to_string());
        let display = format!("{}", err);
        assert!(display.contains("invalid xpath"));
        assert!(display.contains("bad xpath"));
    }

    #[test]
    fn test_xml_parse_display() {
        let err = TransformError::XmlParse("unexpected tag".to_string());
        let display = format!("{}", err);
        assert!(display.contains("xml parse error"));
        assert!(display.contains("unexpected tag"));
    }

    #[test]
    fn test_io_error_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err: TransformError = io_err.into();
        assert!(matches!(err, TransformError::Io(_)));
        let display = format!("{}", err);
        assert!(display.contains("io error"));
    }

    #[test]
    fn test_serialization_display() {
        let err = TransformError::Serialization("failed to serialize".to_string());
        let display = format!("{}", err);
        assert!(display.contains("serialization error"));
        assert!(display.contains("failed to serialize"));
    }

    #[test]
    fn test_modification_display() {
        let err = TransformError::Modification("cannot modify".to_string());
        let display = format!("{}", err);
        assert!(display.contains("modification error"));
        assert!(display.contains("cannot modify"));
    }

    #[test]
    fn test_from_quick_xml_error() {
        // quick_xml::Error can be converted to TransformError
        // Create a quick_xml error with mismatched end tag
        use quick_xml::Reader;
        let bad_xml = "<root></wrong>";
        let mut reader = Reader::from_str(bad_xml);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => {
                    panic!("Expected quick_xml::Error but got EOF");
                }
                Err(e) => {
                    let err: TransformError = e.into();
                    assert!(matches!(err, TransformError::XmlParse(_)));
                    let display = format!("{}", err);
                    assert!(display.contains("xml parse error"));
                    return;
                }
                _ => buf.clear(),
            }
        }
    }

    #[test]
    fn test_other_error_from() {
        let xpath_err = fastxml::xpath::error::XPathSyntaxError::EmptyExpression;
        let crate_err: fastxml::error::Error = xpath_err.into();
        let err: TransformError = crate_err.into();
        assert!(matches!(err, TransformError::Other(_)));
    }
}

// =============================================================================
// SchemaError Tests
// =============================================================================

mod schema_error_tests {
    use fastxml::schema::error::SchemaError;

    #[test]
    fn test_invalid_occurs_display() {
        let err = SchemaError::InvalidOccurs {
            value: "abc".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("invalid occurs"));
        assert!(display.contains("abc"));
    }

    #[test]
    fn test_min_occurs_greater_than_max_display() {
        let err = SchemaError::MinOccursGreaterThanMaxOccurs { min: 10, max: 5 };
        let display = format!("{}", err);
        assert!(display.contains("minOccurs"));
        assert!(display.contains("10"));
        assert!(display.contains("5"));
    }

    #[test]
    fn test_invalid_facet_value_display() {
        let err = SchemaError::InvalidFacetValue {
            facet: "minLength".to_string(),
            value: "-5".to_string(),
            reason: "must be non-negative".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("minLength"));
        assert!(display.contains("-5"));
        assert!(display.contains("must be non-negative"));
    }

    #[test]
    fn test_min_length_greater_than_max_display() {
        let err = SchemaError::MinLengthGreaterThanMaxLength {
            min_length: 100,
            max_length: 50,
        };
        let display = format!("{}", err);
        assert!(display.contains("minLength"));
        assert!(display.contains("100"));
        assert!(display.contains("50"));
    }

    #[test]
    fn test_fraction_digits_greater_than_total_display() {
        let err = SchemaError::FractionDigitsGreaterThanTotalDigits {
            fraction_digits: 10,
            total_digits: 5,
        };
        let display = format!("{}", err);
        assert!(display.contains("fractionDigits"));
        assert!(display.contains("10"));
        assert!(display.contains("5"));
    }

    #[test]
    fn test_schema_not_found_display() {
        let err = SchemaError::SchemaNotFound {
            uri: "http://example.com/schema.xsd".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("schema not found"));
        assert!(display.contains("http://example.com/schema.xsd"));
    }

    #[test]
    fn test_invalid_base_uri_display() {
        let err = SchemaError::InvalidBaseUri {
            uri: "not-a-uri".to_string(),
            message: "invalid format".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("invalid base URI"));
        assert!(display.contains("not-a-uri"));
        assert!(display.contains("invalid format"));
    }

    #[test]
    fn test_url_resolution_failed_display() {
        let err = SchemaError::UrlResolutionFailed {
            relative: "sub/schema.xsd".to_string(),
            base: "file:///root".to_string(),
            message: "cannot resolve".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("sub/schema.xsd"));
        assert!(display.contains("file:///root"));
        assert!(display.contains("cannot resolve"));
    }

    #[test]
    fn test_circular_dependency_display() {
        let err = SchemaError::CircularDependency {
            uri: "http://example.com/a.xsd".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("circular dependency"));
        assert!(display.contains("http://example.com/a.xsd"));
    }

    #[test]
    fn test_schema_error_eq() {
        let err1 = SchemaError::InvalidOccurs {
            value: "x".to_string(),
        };
        let err2 = SchemaError::InvalidOccurs {
            value: "x".to_string(),
        };
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_schema_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(SchemaError::SchemaNotFound {
            uri: "test".to_string(),
        });
        assert!(err.source().is_none());
    }
}

// =============================================================================
// FetchError Tests
// =============================================================================

mod fetch_error_tests {
    use fastxml::schema::fetcher::error::FetchError;

    #[test]
    fn test_request_failed_display() {
        let err = FetchError::RequestFailed {
            url: "http://example.com".to_string(),
            message: "connection refused".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to fetch"));
        assert!(display.contains("http://example.com"));
        assert!(display.contains("connection refused"));
    }

    #[test]
    fn test_http_error_display() {
        let err = FetchError::HttpError {
            status: 404,
            url: "http://example.com/missing".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("HTTP error"));
        assert!(display.contains("404"));
        assert!(display.contains("http://example.com/missing"));
    }

    #[test]
    fn test_read_response_failed_display() {
        let err = FetchError::ReadResponseFailed {
            message: "incomplete body".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to read response"));
        assert!(display.contains("incomplete body"));
    }

    #[test]
    fn test_client_creation_failed_display() {
        let err = FetchError::ClientCreationFailed {
            message: "TLS error".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("failed to create HTTP client"));
        assert!(display.contains("TLS error"));
    }

    #[test]
    fn test_network_disabled_display() {
        let err = FetchError::NetworkDisabled {
            url: "http://example.com/schema.xsd".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("network access disabled"));
        assert!(display.contains("http://example.com/schema.xsd"));
    }

    #[test]
    fn test_fetch_error_eq() {
        let err1 = FetchError::HttpError {
            status: 500,
            url: "test".to_string(),
        };
        let err2 = FetchError::HttpError {
            status: 500,
            url: "test".to_string(),
        };
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_fetch_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(FetchError::NetworkDisabled {
            url: "test".to_string(),
        });
        assert!(err.source().is_none());
    }
}

// =============================================================================
// XsdParseError Tests
// =============================================================================

mod xsd_parse_error_tests {
    use fastxml::schema::xsd::error::XsdParseError;

    #[test]
    fn test_unexpected_end_of_schema_display() {
        let err = XsdParseError::UnexpectedEndOfSchema {
            remaining_frames: 3,
        };
        let display = format!("{}", err);
        assert!(display.contains("unexpected end of schema"));
        assert!(display.contains("3"));
        assert!(display.contains("frames remaining"));
    }

    #[test]
    fn test_parse_error_display() {
        let err = XsdParseError::ParseError {
            position: 42,
            message: "invalid element".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("parse error"));
        assert!(display.contains("42"));
        assert!(display.contains("invalid element"));
    }

    #[test]
    fn test_xsd_parse_error_eq() {
        let err1 = XsdParseError::UnexpectedEndOfSchema {
            remaining_frames: 5,
        };
        let err2 = XsdParseError::UnexpectedEndOfSchema {
            remaining_frames: 5,
        };
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_xsd_parse_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(XsdParseError::ParseError {
            position: 0,
            message: "test".to_string(),
        });
        assert!(err.source().is_none());
    }
}
