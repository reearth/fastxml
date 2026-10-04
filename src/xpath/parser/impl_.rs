//! XPath expression parser implementation.
//!
//! A recursive-descent parser for the XPath 1.0 expression grammar (§3):
//!
//! ```text
//! Expr           ::= OrExpr
//! OrExpr         ::= AndExpr ('or' AndExpr)*
//! AndExpr        ::= EqualityExpr ('and' EqualityExpr)*
//! EqualityExpr   ::= RelationalExpr (('=' | '!=') RelationalExpr)*
//! RelationalExpr ::= AdditiveExpr (('<' | '<=' | '>' | '>=') AdditiveExpr)*
//! AdditiveExpr   ::= MultiplicativeExpr (('+' | '-') MultiplicativeExpr)*
//! MultiplicativeExpr ::= UnaryExpr (('*' | 'div' | 'mod') UnaryExpr)*
//! UnaryExpr      ::= UnionExpr | '-' UnaryExpr
//! UnionExpr      ::= PathExpr ('|' PathExpr)*
//! PathExpr       ::= LocationPath
//!                  | FilterExpr (('/' | '//') RelativeLocationPath)?
//! FilterExpr     ::= PrimaryExpr Predicate*
//! PrimaryExpr    ::= VariableReference | '(' Expr ')' | Literal | Number
//!                  | FunctionCall
//! ```
//!
//! Predicates and function arguments use the same `Expr` grammar. The whole
//! input must be one `Expr`; trailing tokens are a syntax error.

use crate::error::Result;
use crate::xpath::error::XPathSyntaxError;
use crate::xpath::lexer::{Lexer, Token};

use super::ast::{Axis, ComparisonOp, Expr, NodeTest, PathExpr, Predicate, Step};

/// XPath expression parser.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Creates a new parser from an XPath expression string.
    pub fn new(xpath: &str) -> Result<Self> {
        let mut lexer = Lexer::new(xpath);
        let tokens = lexer.tokenize()?;
        Ok(Self { tokens, pos: 0 })
    }

    /// Parses the expression. The whole input must be consumed: trailing
    /// tokens are a syntax error rather than being silently ignored.
    pub fn parse(&mut self) -> Result<Expr> {
        let expr = self.parse_expr()?;
        if !matches!(self.current(), Token::Eof) {
            return Err(self.unexpected("end of expression"));
        }
        Ok(expr)
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1)
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        if self.current() == expected {
            self.advance();
            Ok(())
        } else {
            Err(self.unexpected(&format!("{:?}", expected)))
        }
    }

    fn unexpected(&self, expected: &str) -> crate::error::Error {
        XPathSyntaxError::UnexpectedToken {
            found: Some(self.current().clone()),
            expected: expected.to_string(),
        }
        .into()
    }

    /// Extracts a variable name from the current token.
    /// Accepts both Name tokens and function keyword tokens (e.g., name, text, position).
    fn extract_variable_name(&mut self) -> Result<String> {
        let name = match self.current() {
            Token::Name(n) => n.clone(),
            // Keywords can also be used as variable names
            token => match keyword_name(token) {
                Some(name) => name.to_string(),
                None => return Err(self.unexpected("variable name after $")),
            },
        };
        self.advance();
        Ok(name)
    }

    /// Whether the current token can start a location step.
    ///
    /// Per XPath 1.0 §3.7, keyword tokens (`div`, `and`, `count`, `text`, ...)
    /// that are not in operator position or followed by `(` are plain names.
    fn at_step_start(&self) -> bool {
        match self.current() {
            Token::Dot
            | Token::DoubleDot
            | Token::At
            | Token::Asterisk
            | Token::Name(_)
            | Token::ChildAxis
            | Token::DescendantAxis
            | Token::ParentAxis
            | Token::SelfAxis
            | Token::DescendantOrSelfAxis
            | Token::AncestorAxis
            | Token::AncestorOrSelfAxis
            | Token::FollowingSiblingAxis
            | Token::PrecedingSiblingAxis
            | Token::FollowingAxis
            | Token::PrecedingAxis
            | Token::AttributeAxis
            | Token::NamespaceAxis => true,
            token => keyword_name(token).is_some(),
        }
    }

    /// Whether the current token starts a `PrimaryExpr` (and so a
    /// `FilterExpr`) rather than a location path.
    fn at_primary_start(&self) -> bool {
        let followed_by_paren = self.peek() == Some(&Token::LeftParen);
        match self.current() {
            Token::String(_) | Token::Number(_) | Token::Dollar | Token::LeftParen => true,
            // A name followed by `(` is a function call, unless it is a node
            // type (`comment(`, `processing-instruction(`), which is a step.
            Token::Name(name) => followed_by_paren && !is_node_type_name(name),
            // `text(` / `node(` are node types; operator names never call.
            token => followed_by_paren && function_name(token).is_some(),
        }
    }

    // =========================================================================
    // Expressions
    // =========================================================================

    /// `Expr ::= OrExpr`
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and_expr()?;
        while matches!(self.current(), Token::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_equality_expr()?;
        while matches!(self.current(), Token::And) {
            self.advance();
            let right = self.parse_equality_expr()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_relational_expr()?;
        loop {
            let op = match self.current() {
                Token::Equals => ComparisonOp::Equal,
                Token::NotEquals => ComparisonOp::NotEqual,
                _ => break,
            };
            self.advance();
            let right = self.parse_relational_expr()?;
            left = Expr::Comparison {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_relational_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive_expr()?;
        loop {
            let op = match self.current() {
                Token::LessThan => ComparisonOp::LessThan,
                Token::LessOrEqual => ComparisonOp::LessOrEqual,
                Token::GreaterThan => ComparisonOp::GreaterThan,
                Token::GreaterOrEqual => ComparisonOp::GreaterOrEqual,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive_expr()?;
            left = Expr::Comparison {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative_expr()?;
        loop {
            match self.current() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_multiplicative_expr()?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_multiplicative_expr()?;
                    left = Expr::Subtract(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiplicative_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary_expr()?;
        loop {
            match self.current() {
                // After a complete operand, `*` is the multiply operator
                // (§3.7); as a name test it was already consumed by the path.
                Token::Asterisk => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Multiply(Box::new(left), Box::new(right));
                }
                Token::Div => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Divide(Box::new(left), Box::new(right));
                }
                Token::Mod => {
                    self.advance();
                    let right = self.parse_unary_expr()?;
                    left = Expr::Modulo(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr> {
        if matches!(self.current(), Token::Minus) {
            self.advance();
            let inner = self.parse_unary_expr()?;
            Ok(Expr::Negate(Box::new(inner)))
        } else {
            self.parse_union_expr()
        }
    }

    fn parse_union_expr(&mut self) -> Result<Expr> {
        let first = self.parse_path_expr()?;
        if !matches!(self.current(), Token::Pipe) {
            return Ok(first);
        }
        let mut operands = vec![first];
        while matches!(self.current(), Token::Pipe) {
            self.advance();
            operands.push(self.parse_path_expr()?);
        }
        Ok(Expr::Union(operands))
    }

    /// `PathExpr ::= LocationPath | FilterExpr (('/' | '//') RelativeLocationPath)?`
    fn parse_path_expr(&mut self) -> Result<Expr> {
        if self.at_primary_start() {
            let filter = self.parse_filter_expr()?;
            if matches!(self.current(), Token::Slash | Token::DoubleSlash) {
                let mut steps = Vec::new();
                if matches!(self.current(), Token::DoubleSlash) {
                    steps.push(Step::descendant_or_self_any());
                }
                self.advance();
                self.parse_relative_steps(&mut steps)?;
                return Ok(Expr::PathFrom {
                    base: Box::new(filter),
                    steps,
                });
            }
            return Ok(filter);
        }
        if matches!(self.current(), Token::Slash | Token::DoubleSlash) || self.at_step_start() {
            return Ok(Expr::Path(self.parse_location_path()?));
        }
        Err(self.unexpected("expression"))
    }

    /// `FilterExpr ::= PrimaryExpr Predicate*`. A primary expression without
    /// predicates is returned as is.
    fn parse_filter_expr(&mut self) -> Result<Expr> {
        let primary = self.parse_primary_expr()?;
        let predicates = self.parse_predicates()?;
        if predicates.is_empty() {
            Ok(primary)
        } else {
            Ok(Expr::Filter {
                expr: Box::new(primary),
                predicates,
            })
        }
    }

    fn parse_primary_expr(&mut self) -> Result<Expr> {
        match self.current() {
            Token::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Dollar => {
                self.advance();
                Ok(Expr::Variable(self.extract_variable_name()?))
            }
            Token::LeftParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(inner)
            }
            // Unknown (e.g. extension) function call
            Token::Name(name) => {
                let name = name.clone();
                self.advance();
                self.parse_function_args(name)
            }
            token => match function_name(token) {
                Some(name) => {
                    self.advance();
                    self.parse_function_args(name.to_string())
                }
                None => Err(self.unexpected("primary expression")),
            },
        }
    }

    /// Parses `( [Expr (',' Expr)*] )` after a function name.
    fn parse_function_args(&mut self, name: String) -> Result<Expr> {
        self.expect(&Token::LeftParen)?;

        let mut args = Vec::new();
        if !matches!(self.current(), Token::RightParen) {
            args.push(self.parse_expr()?);
            while matches!(self.current(), Token::Comma) {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }

        self.expect(&Token::RightParen)?;

        Ok(Expr::Function { name, args })
    }

    // =========================================================================
    // Location paths
    // =========================================================================

    fn parse_location_path(&mut self) -> Result<PathExpr> {
        let mut steps = Vec::new();
        let absolute = match self.current() {
            Token::Slash => {
                self.advance();
                // A bare `/` (the root node) has no steps.
                if self.at_step_start() {
                    self.parse_relative_steps(&mut steps)?;
                }
                true
            }
            Token::DoubleSlash => {
                self.advance();
                // `//` is shorthand for `/descendant-or-self::node()/`
                steps.push(Step::descendant_or_self_any());
                self.parse_relative_steps(&mut steps)?;
                true
            }
            _ => {
                self.parse_relative_steps(&mut steps)?;
                false
            }
        };
        Ok(PathExpr { absolute, steps })
    }

    /// `RelativeLocationPath ::= Step (('/' | '//') Step)*`, appended to `steps`.
    fn parse_relative_steps(&mut self, steps: &mut Vec<Step>) -> Result<()> {
        steps.push(self.parse_step()?);
        while matches!(self.current(), Token::Slash | Token::DoubleSlash) {
            if matches!(self.current(), Token::DoubleSlash) {
                steps.push(Step::descendant_or_self_any());
            }
            self.advance();
            // A step is mandatory after `/` or `//`.
            steps.push(self.parse_step()?);
        }
        Ok(())
    }

    fn parse_step(&mut self) -> Result<Step> {
        // Handle abbreviated syntax
        match self.current() {
            Token::Dot => {
                self.advance();
                return Ok(Step {
                    axis: Axis::SelfNode,
                    node_test: NodeTest::Node,
                    predicates: Vec::new(),
                });
            }
            Token::DoubleDot => {
                self.advance();
                return Ok(Step {
                    axis: Axis::Parent,
                    node_test: NodeTest::Node,
                    predicates: Vec::new(),
                });
            }
            Token::At => {
                self.advance();
                let node_test = self.parse_node_test()?;
                let predicates = self.parse_predicates()?;
                return Ok(Step {
                    axis: Axis::Attribute,
                    node_test,
                    predicates,
                });
            }
            _ => {}
        }

        // Check for axis specifier
        let axis = self.parse_axis()?;
        let node_test = self.parse_node_test()?;
        let predicates = self.parse_predicates()?;

        Ok(Step {
            axis,
            node_test,
            predicates,
        })
    }

    fn parse_axis(&mut self) -> Result<Axis> {
        let axis = match self.current() {
            Token::ChildAxis => Some(Axis::Child),
            Token::DescendantAxis => Some(Axis::Descendant),
            Token::ParentAxis => Some(Axis::Parent),
            Token::SelfAxis => Some(Axis::SelfNode),
            Token::DescendantOrSelfAxis => Some(Axis::DescendantOrSelf),
            Token::AncestorAxis => Some(Axis::Ancestor),
            Token::AncestorOrSelfAxis => Some(Axis::AncestorOrSelf),
            Token::FollowingSiblingAxis => Some(Axis::FollowingSibling),
            Token::PrecedingSiblingAxis => Some(Axis::PrecedingSibling),
            Token::FollowingAxis => Some(Axis::Following),
            Token::PrecedingAxis => Some(Axis::Preceding),
            Token::AttributeAxis => Some(Axis::Attribute),
            Token::NamespaceAxis => Some(Axis::Namespace),
            _ => None,
        };

        if let Some(axis) = axis {
            self.advance();
            self.expect(&Token::DoubleColon)?;
            Ok(axis)
        } else {
            // Default axis is child
            Ok(Axis::Child)
        }
    }

    fn parse_node_test(&mut self) -> Result<NodeTest> {
        let followed_by_paren = self.peek() == Some(&Token::LeftParen);
        match self.current() {
            Token::Asterisk => {
                self.advance();
                Ok(NodeTest::Any)
            }
            // Node types are recognised only when followed by `(`; otherwise
            // `comment`, `text`, ... are element names (§3.7).
            Token::Name(name) if followed_by_paren && name == "comment" => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Comment)
            }
            Token::Name(name) if followed_by_paren && name == "processing-instruction" => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                let target = match self.current() {
                    Token::String(s) => {
                        let s = s.clone();
                        self.advance();
                        Some(s)
                    }
                    _ => None,
                };
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::ProcessingInstruction(target))
            }
            Token::Name(name) => {
                let name = name.clone();
                self.advance();
                if let Some((prefix, local)) = name.split_once(':') {
                    Ok(NodeTest::QName {
                        prefix: prefix.to_string(),
                        local: local.to_string(),
                    })
                } else {
                    Ok(NodeTest::Name(name))
                }
            }
            Token::TextFn if followed_by_paren => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Text)
            }
            Token::NodeFn if followed_by_paren => {
                self.advance();
                self.expect(&Token::LeftParen)?;
                self.expect(&Token::RightParen)?;
                Ok(NodeTest::Node)
            }
            // Operator and function keywords are plain names in a node test
            // (e.g. `//div`, `@id`, `@name`, `self::and`).
            token => match keyword_name(token) {
                Some(name) => {
                    let name = name.to_string();
                    self.advance();
                    Ok(NodeTest::Name(name))
                }
                None => Err(self.unexpected("node test")),
            },
        }
    }

    fn parse_predicates(&mut self) -> Result<Vec<Predicate>> {
        let mut predicates = Vec::new();
        while matches!(self.current(), Token::LeftBracket) {
            self.advance();
            let expr = self.parse_expr()?;
            self.expect(&Token::RightBracket)?;
            predicates.push(lower_predicate(expr));
        }
        Ok(predicates)
    }
}

/// Lifts a parsed predicate expression into a [`Predicate`].
///
/// `[n]` abbreviates `[position() = n]`. Only a positive integer can equal a
/// position, so only that becomes [`Predicate::Position`]; any other number
/// (`[0]`, `[1.5]`) stays an expression and selects nothing.
fn lower_predicate(expr: Expr) -> Predicate {
    match expr {
        Expr::Number(n) if n >= 1.0 && n.fract() == 0.0 && n <= u32::MAX as f64 => {
            Predicate::Position(n as usize)
        }
        other => lower_condition(other),
    }
}

/// Lifts the boolean structure (`or` / `and` / comparison / `not()`) of an
/// expression into a [`Predicate`]. Numbers here are booleans, not positions.
fn lower_condition(expr: Expr) -> Predicate {
    match expr {
        Expr::Or(a, b) => {
            Predicate::Or(Box::new(lower_condition(*a)), Box::new(lower_condition(*b)))
        }
        Expr::And(a, b) => {
            Predicate::And(Box::new(lower_condition(*a)), Box::new(lower_condition(*b)))
        }
        Expr::Comparison { left, op, right } => Predicate::Comparison { left, op, right },
        Expr::Function { name, mut args } if name == "not" && args.len() == 1 => {
            Predicate::Not(Box::new(lower_condition(args.remove(0))))
        }
        other => Predicate::Expr(Box::new(other)),
    }
}

/// Whether an unprefixed name is a `NodeType` that `Name` tokens can spell
/// (`text` and `node` have their own tokens).
fn is_node_type_name(name: &str) -> bool {
    matches!(name, "comment" | "processing-instruction")
}

/// The function a keyword token names in a function call. Operator names and
/// the node types `text` / `node` are not functions: `text()` is a node test.
fn function_name(token: &Token) -> Option<&'static str> {
    match token {
        Token::And | Token::Or | Token::Div | Token::Mod | Token::TextFn | Token::NodeFn => None,
        token => keyword_name(token),
    }
}

/// The source spelling of a keyword token. Outside operator / function-call
/// position every keyword is an ordinary name (XPath 1.0 §3.7).
fn keyword_name(token: &Token) -> Option<&'static str> {
    Some(match token {
        // Operator names
        Token::And => "and",
        Token::Or => "or",
        Token::Div => "div",
        Token::Mod => "mod",

        // Node set functions
        Token::NameFn => "name",
        Token::LocalNameFn => "local-name",
        Token::NamespaceUriFn => "namespace-uri",
        Token::PositionFn => "position",
        Token::LastFn => "last",
        Token::CountFn => "count",
        Token::IdFn => "id",

        // String functions
        Token::StringFn => "string",
        Token::ConcatFn => "concat",
        Token::ContainsFn => "contains",
        Token::StartsWithFn => "starts-with",
        Token::SubstringFn => "substring",
        Token::SubstringBeforeFn => "substring-before",
        Token::SubstringAfterFn => "substring-after",
        Token::StringLengthFn => "string-length",
        Token::NormalizeSpaceFn => "normalize-space",
        Token::TranslateFn => "translate",

        // Boolean functions
        Token::Not => "not",
        Token::TrueFn => "true",
        Token::FalseFn => "false",
        Token::BooleanFn => "boolean",
        Token::LangFn => "lang",

        // Number functions
        Token::NumberFn => "number",
        Token::SumFn => "sum",
        Token::FloorFn => "floor",
        Token::CeilingFn => "ceiling",
        Token::RoundFn => "round",

        // Node types
        Token::TextFn => "text",
        Token::NodeFn => "node",

        _ => return None,
    })
}

/// Parses an XPath expression string into an AST.
pub fn parse_xpath(xpath: &str) -> Result<Expr> {
    let mut parser = Parser::new(xpath)?;
    parser.parse()
}
