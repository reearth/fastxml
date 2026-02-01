//! XPath error types.

use crate::xpath::lexer::Token;

/// XPath syntax errors that occur during parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathSyntaxError {
    /// Unexpected token encountered
    UnexpectedToken {
        /// The token that was found
        found: Option<Token>,
        /// Description of what was expected
        expected: String,
    },
    /// Unexpected character in input
    UnexpectedCharacter {
        /// The unexpected character
        char: char,
        /// Position in the input string
        position: usize,
    },
    /// Invalid number literal
    InvalidNumber {
        /// The invalid number string
        value: String,
    },
    /// Unknown axis name
    UnknownAxis {
        /// The unknown axis name
        name: String,
    },
    /// Unknown function name
    UnknownFunction {
        /// The unknown function name
        name: String,
    },
    /// Unclosed bracket
    UnclosedBracket,
    /// Unclosed parenthesis
    UnclosedParenthesis,
    /// Unclosed string literal
    UnclosedString,
    /// Empty expression
    EmptyExpression,
    /// Missing operand in expression
    MissingOperand {
        /// The operator that is missing an operand
        operator: String,
    },
}

impl std::fmt::Display for XPathSyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XPathSyntaxError::UnexpectedToken { found, expected } => match found {
                Some(token) => write!(f, "expected {}, found {:?}", expected, token),
                None => write!(f, "expected {}, found end of input", expected),
            },
            XPathSyntaxError::UnexpectedCharacter { char, position } => {
                write!(
                    f,
                    "unexpected character '{}' at position {}",
                    char, position
                )
            }
            XPathSyntaxError::InvalidNumber { value } => {
                write!(f, "invalid number '{}'", value)
            }
            XPathSyntaxError::UnknownAxis { name } => {
                write!(
                    f,
                    "unknown axis '{}'. Valid axes: child, descendant, parent, self, \
                     descendant-or-self, ancestor, ancestor-or-self, following-sibling, \
                     preceding-sibling, following, preceding, attribute, namespace",
                    name
                )
            }
            XPathSyntaxError::UnknownFunction { name } => {
                write!(f, "unknown function '{}'", name)
            }
            XPathSyntaxError::UnclosedBracket => {
                write!(f, "unclosed bracket '['")
            }
            XPathSyntaxError::UnclosedParenthesis => {
                write!(f, "unclosed parenthesis '('")
            }
            XPathSyntaxError::UnclosedString => {
                write!(f, "unclosed string literal")
            }
            XPathSyntaxError::EmptyExpression => {
                write!(f, "empty expression")
            }
            XPathSyntaxError::MissingOperand { operator } => {
                write!(f, "missing operand for operator '{}'", operator)
            }
        }
    }
}

impl std::error::Error for XPathSyntaxError {}

/// XPath evaluation errors that occur during expression evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathEvalError {
    /// Unknown function name
    UnknownFunction {
        /// The unknown function name
        name: String,
    },
    /// Wrong number of arguments for a function
    WrongArgumentCount {
        /// The function name
        function: String,
        /// Expected argument count description (e.g., "1", "0 or 1", "at least 2")
        expected: String,
        /// Actual number of arguments provided
        found: usize,
    },
    /// Invalid argument type for a function
    InvalidArgumentType {
        /// The function name
        function: String,
        /// Expected type description (e.g., "node-set")
        expected: String,
    },
}

impl std::fmt::Display for XPathEvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XPathEvalError::UnknownFunction { name } => {
                write!(f, "unknown function: {}", name)
            }
            XPathEvalError::WrongArgumentCount {
                function,
                expected,
                found,
            } => {
                write!(
                    f,
                    "{}() requires {} argument(s), got {}",
                    function, expected, found
                )
            }
            XPathEvalError::InvalidArgumentType { function, expected } => {
                write!(f, "{}() requires a {} argument", function, expected)
            }
        }
    }
}

impl std::error::Error for XPathEvalError {}
